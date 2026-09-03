//! Deterministic option contracts, European valuation, expiration lifecycle,
//! scenarios, and book reconciliation.
//!
//! All economic inputs and outputs are [`Decimal`] values. The Black–Scholes
//! implementation uses a bounded fixed-point approximation for logarithm,
//! exponential, normal density, and normal CDF before quantizing every result
//! to Follon's eight decimal places. It therefore does not consult a platform
//! math library or wall clock. This v1 model deliberately accepts only
//! European valuation; an American-style contract must be rejected rather than
//! silently priced with the wrong model. Expiration exercise/assignment and
//! cash/physical settlement are modeled separately from valuation style.

use std::collections::{BTreeMap, BTreeSet};

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const ONE: Decimal = Decimal::from_scaled(100_000_000);
const TWO: Decimal = Decimal::from_scaled(200_000_000);
const HALF: Decimal = Decimal::from_scaled(50_000_000);
const LN_2: Decimal = Decimal::from_scaled(69_314_718);
const INV_SQRT_TWO_PI: Decimal = Decimal::from_scaled(39_894_228);
const CDF_P: Decimal = Decimal::from_scaled(23_164_190);
const CDF_A1: Decimal = Decimal::from_scaled(31_938_153);
const CDF_A2: Decimal = Decimal::from_scaled(-35_678_818);
const CDF_A3: Decimal = Decimal::from_scaled(178_147_793);
const CDF_A4: Decimal = Decimal::from_scaled(-182_125_598);
const CDF_A5: Decimal = Decimal::from_scaled(133_027_442);
const MIN_VOLATILITY: Decimal = Decimal::from_scaled(10_000);
const MAX_VOLATILITY: Decimal = Decimal::from_scaled(500_000_000);
const MAX_ABSOLUTE_RATE: Decimal = Decimal::from_scaled(100_000_000);
const MAX_YEARS: Decimal = Decimal::from_scaled(1_000_000_000);

/// Version of the deterministic option valuation model.
pub const OPTION_MODEL_VERSION: &str = "follon-european-black-scholes-fixed-v1";

/// Option-reference, market-data, model, or reconciliation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionError(pub String);

impl std::fmt::Display for OptionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OptionError {}

impl From<follon_domain::DomainError> for OptionError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for OptionError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

/// The payoff right represented by a European option contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OptionRight {
    /// Right to buy the underlying at the strike.
    Call,
    /// Right to sell the underlying at the strike.
    Put,
}

impl OptionRight {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, OptionError> {
        match value {
            "CALL" => Ok(Self::Call),
            "PUT" => Ok(Self::Put),
            _ => Err(OptionError("option right must be CALL or PUT".to_owned())),
        }
    }

    /// Stable external representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Call => "CALL",
            Self::Put => "PUT",
        }
    }
}

/// One versioned European option contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionContract {
    /// Canonical option instrument identity.
    pub option_id: String,
    /// Canonical underlying instrument identity.
    pub underlying_instrument_id: String,
    /// UTC expiration instant at which this European contract settles.
    pub expiration_at: String,
    /// Exact strike in the contract currency.
    pub strike: Decimal,
    /// Call or put right.
    pub right: OptionRight,
    /// Exact economic multiplier per one option contract.
    pub multiplier: Decimal,
    /// Three-letter settlement currency.
    pub currency: String,
    /// Immutable reference-data/vendor revision.
    pub reference_version: String,
}

impl OptionContract {
    /// Validates a durable option contract without consulting a clock.
    pub fn validate(&self) -> Result<(), OptionError> {
        validate_canonical_id("option_id", &self.option_id)?;
        validate_canonical_id(
            "option underlying_instrument_id",
            &self.underlying_instrument_id,
        )?;
        validate_utc_timestamp("option expiration_at", &self.expiration_at)?;
        validate_canonical_id("option reference_version", &self.reference_version)?;
        if self.strike <= Decimal::ZERO
            || self.multiplier <= Decimal::ZERO
            || self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.reference_version.is_empty()
        {
            return Err(OptionError(
                "option contract requires positive economics, currency, and reference version"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    /// Exact intrinsic value per one underlying unit at expiration.
    pub fn intrinsic_value(&self, underlying_price: Decimal) -> Result<Decimal, OptionError> {
        if underlying_price < Decimal::ZERO {
            return Err(OptionError(
                "underlying scenario price cannot be negative".to_owned(),
            ));
        }
        Ok(match self.right {
            OptionRight::Call => positive_part(underlying_price.checked_sub(self.strike)?)?,
            OptionRight::Put => positive_part(self.strike.checked_sub(underlying_price)?)?,
        })
    }
}

/// One bid/ask quote selected from an immutable chain snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionQuote {
    /// Quoted contract identity.
    pub option_id: String,
    /// Canonical observation time, equal to the enclosing chain snapshot time.
    pub observed_at: String,
    /// Non-negative bid premium per underlying unit.
    pub bid: Decimal,
    /// Positive ask premium per underlying unit.
    pub ask: Decimal,
    /// Non-negative last-traded premium; zero means no included last trade.
    pub last: Decimal,
    /// Non-negative reported trade count.
    pub volume: u64,
    /// Non-negative reported open interest.
    pub open_interest: u64,
}

impl OptionQuote {
    /// Validates quote economics and returns the exact midpoint used for model comparison.
    pub fn midpoint(&self) -> Result<Decimal, OptionError> {
        validate_canonical_id("option quote option_id", &self.option_id)?;
        validate_utc_timestamp("option quote observed_at", &self.observed_at)?;
        if self.bid < Decimal::ZERO
            || self.ask <= Decimal::ZERO
            || self.bid > self.ask
            || self.last < Decimal::ZERO
        {
            return Err(OptionError(
                "option quote has invalid bid/ask/last economics".to_owned(),
            ));
        }
        Ok(self.bid.checked_add(self.ask)?.checked_div(TWO)?)
    }
}

/// One immutable option-chain snapshot with all contracts and quotes required to reproduce it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionChain {
    /// Canonical chain snapshot identity.
    pub chain_id: String,
    /// Canonical underlying identity shared by every included contract.
    pub underlying_instrument_id: String,
    /// Canonical chain observation time.
    pub snapshot_at: String,
    /// Positive underlying mark selected at the snapshot time.
    pub underlying_mark: Decimal,
    /// Immutable vendor/reference-data version.
    pub reference_version: String,
    /// Complete listed contract definitions selected for this chain.
    pub contracts: Vec<OptionContract>,
    /// Quotes, exactly one per included contract.
    pub quotes: Vec<OptionQuote>,
}

impl OptionChain {
    /// Validates time, reference identity, contracts, and one-to-one quotes.
    pub fn validate(&self) -> Result<(), OptionError> {
        validate_canonical_id("option chain_id", &self.chain_id)?;
        validate_canonical_id(
            "option chain underlying_instrument_id",
            &self.underlying_instrument_id,
        )?;
        validate_utc_timestamp("option chain snapshot_at", &self.snapshot_at)?;
        validate_canonical_id("option chain reference_version", &self.reference_version)?;
        if self.underlying_mark <= Decimal::ZERO || self.contracts.is_empty() {
            return Err(OptionError(
                "option chain requires a positive underlying mark and contracts".to_owned(),
            ));
        }
        let mut contract_ids = BTreeSet::new();
        for contract in &self.contracts {
            contract.validate()?;
            if contract.underlying_instrument_id != self.underlying_instrument_id
                || contract.currency != self.currency()?
                || contract.reference_version != self.reference_version
                || contract.expiration_at <= self.snapshot_at
                || !contract_ids.insert(&contract.option_id)
            {
                return Err(OptionError(
                    "option chain contracts must be unique, live, and share underlying/currency/reference version"
                        .to_owned(),
                ));
            }
        }
        let mut quote_ids = BTreeSet::new();
        for quote in &self.quotes {
            quote.midpoint()?;
            if quote.observed_at != self.snapshot_at
                || !contract_ids.contains(&quote.option_id)
                || !quote_ids.insert(&quote.option_id)
            {
                return Err(OptionError(
                    "option chain quotes must be unique snapshot-time contract quotes".to_owned(),
                ));
            }
        }
        if quote_ids != contract_ids {
            return Err(OptionError(
                "option chain requires exactly one quote for every contract".to_owned(),
            ));
        }
        Ok(())
    }

    /// Returns the contract selected by a canonical option id.
    pub fn contract(&self, option_id: &str) -> Option<&OptionContract> {
        self.contracts
            .iter()
            .find(|contract| contract.option_id == option_id)
    }

    /// Returns the quote selected by a canonical option id.
    pub fn quote(&self, option_id: &str) -> Option<&OptionQuote> {
        self.quotes
            .iter()
            .find(|quote| quote.option_id == option_id)
    }

    /// Stable SHA-256 identity of the complete contracts, quotes, and selected underlying mark.
    pub fn fingerprint(&self) -> Result<String, OptionError> {
        self.validate()?;
        let mut contracts = self.contracts.clone();
        contracts.sort_by(|left, right| left.option_id.cmp(&right.option_id));
        let mut quotes = self.quotes.clone();
        quotes.sort_by(|left, right| left.option_id.cmp(&right.option_id));
        let mut canonical = format!(
            "chain_id={}\nunderlying={}\nsnapshot_at={}\nunderlying_mark={}\nreference_version={}\n",
            self.chain_id,
            self.underlying_instrument_id,
            self.snapshot_at,
            self.underlying_mark,
            self.reference_version,
        );
        for contract in contracts {
            canonical.push_str(&format!(
                "contract={}\nunderlying={}\nexpiration={}\nstrike={}\nright={}\nmultiplier={}\ncurrency={}\nreference_version={}\n",
                contract.option_id,
                contract.underlying_instrument_id,
                contract.expiration_at,
                contract.strike,
                contract.right.as_str(),
                contract.multiplier,
                contract.currency,
                contract.reference_version,
            ));
        }
        for quote in quotes {
            canonical.push_str(&format!(
                "quote={}\nobserved_at={}\nbid={}\nask={}\nlast={}\nvolume={}\nopen_interest={}\n",
                quote.option_id,
                quote.observed_at,
                quote.bid,
                quote.ask,
                quote.last,
                quote.volume,
                quote.open_interest,
            ));
        }
        Ok(sha256(&canonical))
    }

    fn currency(&self) -> Result<String, OptionError> {
        self.contracts
            .first()
            .map(|contract| contract.currency.clone())
            .ok_or_else(|| OptionError("option chain has no contracts".to_owned()))
    }
}

/// Exact Black–Scholes input for one European option at an explicit valuation instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EuropeanModelInput {
    /// Underlying mark, strictly positive before expiry.
    pub underlying_price: Decimal,
    /// Contract strike, strictly positive.
    pub strike: Decimal,
    /// Annualized continuously compounded risk-free rate.
    pub risk_free_rate: Decimal,
    /// Annualized implied volatility, expressed as `0.20` for 20%.
    pub volatility: Decimal,
    /// Remaining exact model time in years.
    pub time_to_expiry_years: Decimal,
    /// Call or put payoff.
    pub right: OptionRight,
}

impl EuropeanModelInput {
    /// Validates bounded deterministic model inputs.
    pub fn validate(&self) -> Result<(), OptionError> {
        if self.underlying_price <= Decimal::ZERO
            || self.strike <= Decimal::ZERO
            || self.volatility < Decimal::ZERO
            || self.volatility > MAX_VOLATILITY
            || absolute(self.risk_free_rate)? > MAX_ABSOLUTE_RATE
            || self.time_to_expiry_years < Decimal::ZERO
            || self.time_to_expiry_years > MAX_YEARS
        {
            return Err(OptionError(
                "invalid European option model input".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Model price and greeks, all per one underlying unit of option premium.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionGreeks {
    /// Exact model premium.
    pub model_price: Decimal,
    /// Change in premium per one unit change in underlying mark.
    pub delta: Decimal,
    /// Change in delta per one unit change in underlying mark.
    pub gamma: Decimal,
    /// Change in premium per one `1.00` volatility change.
    pub vega: Decimal,
    /// Annualized passage-of-time change in premium.
    pub theta: Decimal,
    /// Change in premium per one `1.00` rate change.
    pub rho: Decimal,
    /// Deterministic `d1` supporting the model evidence.
    pub d1: Decimal,
    /// Deterministic `d2` supporting the model evidence.
    pub d2: Decimal,
}

/// Prices a European option and derives greeks with deterministic fixed-point approximations.
pub fn european_greeks(input: &EuropeanModelInput) -> Result<OptionGreeks, OptionError> {
    input.validate()?;
    if input.time_to_expiry_years == Decimal::ZERO || input.volatility == Decimal::ZERO {
        let intrinsic = match input.right {
            OptionRight::Call => positive_part(input.underlying_price.checked_sub(input.strike)?)?,
            OptionRight::Put => positive_part(input.strike.checked_sub(input.underlying_price)?)?,
        };
        let delta = match input.right {
            OptionRight::Call if input.underlying_price > input.strike => ONE,
            OptionRight::Put if input.underlying_price < input.strike => {
                Decimal::ZERO.checked_sub(ONE)?
            }
            _ => Decimal::ZERO,
        };
        return Ok(OptionGreeks {
            model_price: intrinsic,
            delta,
            gamma: Decimal::ZERO,
            vega: Decimal::ZERO,
            theta: Decimal::ZERO,
            rho: Decimal::ZERO,
            d1: Decimal::ZERO,
            d2: Decimal::ZERO,
        });
    }
    let sqrt_time = sqrt(input.time_to_expiry_years)?;
    let volatility_sqrt_time = input.volatility.checked_mul(sqrt_time)?;
    if volatility_sqrt_time < MIN_VOLATILITY {
        return Err(OptionError(
            "option model volatility-time product is too small".to_owned(),
        ));
    }
    let variance_half = input
        .volatility
        .checked_mul(input.volatility)?
        .checked_mul(HALF)?;
    let numerator = ln(input.underlying_price.checked_div(input.strike)?)?.checked_add(
        input
            .risk_free_rate
            .checked_add(variance_half)?
            .checked_mul(input.time_to_expiry_years)?,
    )?;
    let d1 = numerator.checked_div(volatility_sqrt_time)?;
    let d2 = d1.checked_sub(volatility_sqrt_time)?;
    let discount = exp(Decimal::ZERO.checked_sub(
        input
            .risk_free_rate
            .checked_mul(input.time_to_expiry_years)?,
    )?)?;
    let density = normal_density(d1)?;
    let common_gamma =
        density.checked_div(input.underlying_price.checked_mul(volatility_sqrt_time)?)?;
    let vega = input
        .underlying_price
        .checked_mul(density)?
        .checked_mul(sqrt_time)?;
    let time_decay = input
        .underlying_price
        .checked_mul(density)?
        .checked_mul(input.volatility)?
        .checked_div(TWO.checked_mul(sqrt_time)?)?;
    let (model_price, delta, theta, rho) = match input.right {
        OptionRight::Call => {
            let nd1 = normal_cdf(d1)?;
            let nd2 = normal_cdf(d2)?;
            (
                input
                    .underlying_price
                    .checked_mul(nd1)?
                    .checked_sub(input.strike.checked_mul(discount)?.checked_mul(nd2)?)?,
                nd1,
                Decimal::ZERO.checked_sub(time_decay)?.checked_sub(
                    input
                        .risk_free_rate
                        .checked_mul(input.strike)?
                        .checked_mul(discount)?
                        .checked_mul(nd2)?,
                )?,
                input
                    .strike
                    .checked_mul(input.time_to_expiry_years)?
                    .checked_mul(discount)?
                    .checked_mul(nd2)?,
            )
        }
        OptionRight::Put => {
            let negative_d1 = Decimal::ZERO.checked_sub(d1)?;
            let negative_d2 = Decimal::ZERO.checked_sub(d2)?;
            let n_minus_d1 = normal_cdf(negative_d1)?;
            let n_minus_d2 = normal_cdf(negative_d2)?;
            (
                input
                    .strike
                    .checked_mul(discount)?
                    .checked_mul(n_minus_d2)?
                    .checked_sub(input.underlying_price.checked_mul(n_minus_d1)?)?,
                Decimal::ZERO.checked_sub(n_minus_d1)?,
                Decimal::ZERO.checked_sub(time_decay)?.checked_add(
                    input
                        .risk_free_rate
                        .checked_mul(input.strike)?
                        .checked_mul(discount)?
                        .checked_mul(n_minus_d2)?,
                )?,
                Decimal::ZERO.checked_sub(
                    input
                        .strike
                        .checked_mul(input.time_to_expiry_years)?
                        .checked_mul(discount)?
                        .checked_mul(n_minus_d2)?,
                )?,
            )
        }
    };
    Ok(OptionGreeks {
        model_price: positive_part(model_price)?,
        delta,
        gamma: common_gamma,
        vega,
        theta,
        rho,
        d1,
        d2,
    })
}

/// Implies volatility from a market premium using deterministic bounded bisection.
pub fn implied_volatility(
    mut input: EuropeanModelInput,
    market_premium: Decimal,
) -> Result<Decimal, OptionError> {
    input.validate()?;
    if input.time_to_expiry_years == Decimal::ZERO || market_premium < Decimal::ZERO {
        return Err(OptionError(
            "implied volatility requires positive remaining time and premium".to_owned(),
        ));
    }
    let discount = exp(Decimal::ZERO.checked_sub(
        input
            .risk_free_rate
            .checked_mul(input.time_to_expiry_years)?,
    )?)?;
    let lower = match input.right {
        OptionRight::Call => positive_part(
            input
                .underlying_price
                .checked_sub(input.strike.checked_mul(discount)?)?,
        )?,
        OptionRight::Put => positive_part(
            input
                .strike
                .checked_mul(discount)?
                .checked_sub(input.underlying_price)?,
        )?,
    };
    let upper = match input.right {
        OptionRight::Call => input.underlying_price,
        OptionRight::Put => input.strike.checked_mul(discount)?,
    };
    if market_premium < lower || market_premium > upper {
        return Err(OptionError(
            "market premium violates European no-arbitrage bounds".to_owned(),
        ));
    }
    let mut low = MIN_VOLATILITY;
    let mut high = MAX_VOLATILITY;
    for _ in 0..80 {
        let midpoint = low.checked_add(high)?.checked_div(TWO)?;
        input.volatility = midpoint;
        let price = european_greeks(&input)?.model_price;
        if price < market_premium {
            low = midpoint;
        } else {
            high = midpoint;
        }
    }
    low.checked_add(high)?.checked_div(TWO).map_err(Into::into)
}

/// Contract analytics derived from a frozen chain quote and model rate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionAnalytics {
    /// Canonical contract identity.
    pub option_id: String,
    /// Exact quote midpoint used for implied volatility.
    pub market_premium: Decimal,
    /// Fixed-point implied volatility produced from the midpoint.
    pub implied_volatility: Decimal,
    /// Black–Scholes price and greeks at the implied volatility.
    pub greeks: OptionGreeks,
}

/// Derives reproducible implied volatilities and greeks for every chain contract.
pub fn analyze_chain(
    chain: &OptionChain,
    risk_free_rate: Decimal,
) -> Result<Vec<OptionAnalytics>, OptionError> {
    chain.validate()?;
    let mut analytics = Vec::with_capacity(chain.contracts.len());
    for contract in &chain.contracts {
        let contract_time = time_to_expiry_years(&chain.snapshot_at, &contract.expiration_at)?;
        let quote = chain
            .quote(&contract.option_id)
            .ok_or_else(|| OptionError("validated chain has no contract quote".to_owned()))?;
        let market_premium = quote.midpoint()?;
        let input = EuropeanModelInput {
            underlying_price: chain.underlying_mark,
            strike: contract.strike,
            risk_free_rate,
            volatility: MIN_VOLATILITY,
            time_to_expiry_years: contract_time,
            right: contract.right,
        };
        let implied_volatility = implied_volatility(input.clone(), market_premium)?;
        let greeks = european_greeks(&EuropeanModelInput {
            volatility: implied_volatility,
            ..input
        })?;
        analytics.push(OptionAnalytics {
            option_id: contract.option_id.clone(),
            market_premium,
            implied_volatility,
            greeks,
        });
    }
    analytics.sort_by(|left, right| left.option_id.cmp(&right.option_id));
    Ok(analytics)
}

/// Individual point on a 2D implied volatility surface (Strike x Expiration).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolatilitySurfacePoint {
    /// Option contract identity.
    pub option_id: String,
    /// Strike price.
    pub strike: Decimal,
    /// UTC expiration timestamp.
    pub expiration_at: String,
    /// Call or Put right.
    pub right: OptionRight,
    /// Solved implied volatility.
    pub implied_volatility: Decimal,
}

/// Bounded 2D volatility surface constructed from an OptionChain snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolatilitySurface {
    /// Originating chain identity.
    pub chain_id: String,
    /// Underlying instrument identity.
    pub underlying_instrument_id: String,
    /// Underlying mark price.
    pub underlying_mark: Decimal,
    /// UTC snapshot timestamp.
    pub snapshot_at: String,
    /// Surface points.
    pub points: Vec<VolatilitySurfacePoint>,
}

impl VolatilitySurface {
    /// Returns stable SHA-256 fingerprint of the volatility surface.
    pub fn fingerprint(&self) -> Result<String, OptionError> {
        validate_canonical_id("volatility surface chain_id", &self.chain_id)?;
        validate_canonical_id(
            "volatility surface underlying",
            &self.underlying_instrument_id,
        )?;
        let mut points = self.points.clone();
        points.sort_by(|left, right| left.option_id.cmp(&right.option_id));
        let mut canonical = format!(
            "chain_id={}\nunderlying={}\nsnapshot_at={}\nunderlying_mark={}\n",
            self.chain_id, self.underlying_instrument_id, self.snapshot_at, self.underlying_mark
        );
        for pt in points {
            canonical.push_str(&format!(
                "point={}\nstrike={}\nexpiration={}\nright={}\niv={}\n",
                pt.option_id,
                pt.strike,
                pt.expiration_at,
                pt.right.as_str(),
                pt.implied_volatility
            ));
        }
        Ok(sha256(&canonical))
    }
}

/// Generates a VolatilitySurface from an OptionChain and risk-free rate.
pub fn generate_volatility_surface(
    chain: &OptionChain,
    risk_free_rate: Decimal,
) -> Result<VolatilitySurface, OptionError> {
    let analytics = analyze_chain(chain, risk_free_rate)?;
    let mut points = Vec::with_capacity(analytics.len());
    for item in analytics {
        let contract = chain
            .contract(&item.option_id)
            .ok_or_else(|| OptionError("contract missing from chain".to_owned()))?;
        points.push(VolatilitySurfacePoint {
            option_id: item.option_id,
            strike: contract.strike,
            expiration_at: contract.expiration_at.clone(),
            right: contract.right,
            implied_volatility: item.implied_volatility,
        });
    }
    points.sort_by(|left, right| left.option_id.cmp(&right.option_id));
    Ok(VolatilitySurface {
        chain_id: chain.chain_id.clone(),
        underlying_instrument_id: chain.underlying_instrument_id.clone(),
        underlying_mark: chain.underlying_mark,
        snapshot_at: chain.snapshot_at.clone(),
        points,
    })
}

/// Result of a news event volatility shock scenario evaluation on an OptionChain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewsVolatilityShockResult {
    /// Baseline total model value across all chain options before shock.
    pub pre_shock_model_value: Decimal,
    /// Post-shock total model value after applying vol_shock_bps.
    pub post_shock_model_value: Decimal,
    /// Total vega P&L shift in account currency.
    pub vega_pnl: Decimal,
    /// Average implied volatility before shock.
    pub mean_pre_shock_iv: Decimal,
    /// Average implied volatility after shock.
    pub mean_post_shock_iv: Decimal,
}

/// Evaluates portfolio option chain value shifts under a news event volatility shock.
pub fn evaluate_news_volatility_shock(
    chain: &OptionChain,
    vol_shock_bps: Decimal,
    risk_free_rate: Decimal,
) -> Result<NewsVolatilityShockResult, OptionError> {
    let analytics = analyze_chain(chain, risk_free_rate)?;
    if analytics.is_empty() {
        return Err(OptionError("chain has no analytics".to_owned()));
    }
    let shock_decimal = vol_shock_bps.checked_div(Decimal::from_integer(10_000)?)?;

    let mut pre_shock_total = Decimal::ZERO;
    let mut post_shock_total = Decimal::ZERO;
    let mut sum_pre_iv = Decimal::ZERO;
    let mut sum_post_iv = Decimal::ZERO;

    for item in &analytics {
        let contract = chain
            .contract(&item.option_id)
            .ok_or_else(|| OptionError("contract missing".to_owned()))?;
        let contract_time = time_to_expiry_years(&chain.snapshot_at, &contract.expiration_at)?;

        let raw_post_iv = item.implied_volatility.checked_add(shock_decimal)?;
        let post_iv = if raw_post_iv < MIN_VOLATILITY {
            MIN_VOLATILITY
        } else if raw_post_iv > MAX_VOLATILITY {
            MAX_VOLATILITY
        } else {
            raw_post_iv
        };

        let post_greeks = european_greeks(&EuropeanModelInput {
            underlying_price: chain.underlying_mark,
            strike: contract.strike,
            risk_free_rate,
            volatility: post_iv,
            time_to_expiry_years: contract_time,
            right: contract.right,
        })?;

        let contract_pre_val = item.greeks.model_price.checked_mul(contract.multiplier)?;
        let contract_post_val = post_greeks.model_price.checked_mul(contract.multiplier)?;

        pre_shock_total = pre_shock_total.checked_add(contract_pre_val)?;
        post_shock_total = post_shock_total.checked_add(contract_post_val)?;
        sum_pre_iv = sum_pre_iv.checked_add(item.implied_volatility)?;
        sum_post_iv = sum_post_iv.checked_add(post_iv)?;
    }

    let count = Decimal::from_integer(analytics.len() as i64)?;
    let vega_pnl = post_shock_total.checked_sub(pre_shock_total)?;
    Ok(NewsVolatilityShockResult {
        pre_shock_model_value: pre_shock_total,
        post_shock_model_value: post_shock_total,
        vega_pnl,
        mean_pre_shock_iv: sum_pre_iv.checked_div(count)?,
        mean_post_shock_iv: sum_post_iv.checked_div(count)?,
    })
}

/// One option strategy-leg economic direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionLegSide {
    /// The strategy paid premium and owns the option payoff.
    Long,
    /// The strategy received premium and owes the option payoff.
    Short,
}

impl OptionLegSide {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, OptionError> {
        match value {
            "LONG" => Ok(Self::Long),
            "SHORT" => Ok(Self::Short),
            _ => Err(OptionError(
                "option leg side must be LONG or SHORT".to_owned(),
            )),
        }
    }

    /// Stable external representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
        }
    }

    fn sign(self) -> Result<Decimal, OptionError> {
        match self {
            Self::Long => Ok(ONE),
            Self::Short => Decimal::ZERO.checked_sub(ONE).map_err(Into::into),
        }
    }
}

/// One immutable option leg with its exact entry premium.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionStrategyLeg {
    /// Canonical leg identity.
    pub leg_id: String,
    /// Referenced option contract.
    pub option_id: String,
    /// Long or short premium/payoff direction.
    pub side: OptionLegSide,
    /// Positive exact number of contracts; fractional contracts are rejected.
    pub quantity: Decimal,
    /// Exact premium per underlying unit at entry.
    pub entry_premium: Decimal,
}

/// Immutable multi-leg option strategy definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionStrategy {
    /// Canonical strategy identity.
    pub strategy_id: String,
    /// Immutable strategy revision.
    pub strategy_version: String,
    /// Legs evaluated as one economic portfolio.
    pub legs: Vec<OptionStrategyLeg>,
}

impl OptionStrategy {
    /// Validates strategy/leg identity and availability against the selected chain.
    pub fn validate(&self, chain: &OptionChain) -> Result<(), OptionError> {
        chain.validate()?;
        validate_canonical_id("option strategy_id", &self.strategy_id)?;
        if self.strategy_version.is_empty() || self.legs.is_empty() {
            return Err(OptionError(
                "option strategy needs a version and at least one leg".to_owned(),
            ));
        }
        let mut leg_ids = BTreeSet::new();
        for leg in &self.legs {
            validate_canonical_id("option leg_id", &leg.leg_id)?;
            if !leg_ids.insert(&leg.leg_id)
                || chain.contract(&leg.option_id).is_none()
                || leg.quantity <= Decimal::ZERO
                || leg.quantity.scaled() % 100_000_000 != 0
                || leg.entry_premium < Decimal::ZERO
            {
                return Err(OptionError(
                    "option strategy legs require unique IDs, listed whole contracts, and valid premiums"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Expiry P&L for one leg and total strategy at a supplied underlying scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiryScenarioResult {
    /// Exact non-negative underlying price at expiration.
    pub underlying_price: Decimal,
    /// Exact total strategy P&L in the option currency.
    pub total_pnl: Decimal,
    /// Deterministically ordered per-leg P&L.
    pub legs: Vec<ExpiryLegResult>,
}

/// P&L contribution for one strategy leg at one expiry scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiryLegResult {
    /// Canonical leg identity.
    pub leg_id: String,
    /// Canonical option contract identity.
    pub option_id: String,
    /// Intrinsic value per underlying unit.
    pub intrinsic_value: Decimal,
    /// Exact total P&L after multiplier, quantity, entry premium, and direction.
    pub pnl: Decimal,
}

/// Evaluates a complete multi-leg strategy at option expiration.
pub fn evaluate_expiry_scenario(
    chain: &OptionChain,
    strategy: &OptionStrategy,
    underlying_price: Decimal,
) -> Result<ExpiryScenarioResult, OptionError> {
    strategy.validate(chain)?;
    if underlying_price < Decimal::ZERO {
        return Err(OptionError(
            "expiry underlying scenario cannot be negative".to_owned(),
        ));
    }
    let mut total_pnl = Decimal::ZERO;
    let mut legs = Vec::with_capacity(strategy.legs.len());
    for leg in &strategy.legs {
        let contract = chain
            .contract(&leg.option_id)
            .ok_or_else(|| OptionError("validated strategy contract is missing".to_owned()))?;
        let intrinsic_value = contract.intrinsic_value(underlying_price)?;
        let pnl = intrinsic_value
            .checked_sub(leg.entry_premium)?
            .checked_mul(contract.multiplier)?
            .checked_mul(leg.quantity)?
            .checked_mul(leg.side.sign()?)?;
        total_pnl = total_pnl.checked_add(pnl)?;
        legs.push(ExpiryLegResult {
            leg_id: leg.leg_id.clone(),
            option_id: leg.option_id.clone(),
            intrinsic_value,
            pnl,
        });
    }
    legs.sort_by(|left, right| left.leg_id.cmp(&right.leg_id));
    Ok(ExpiryScenarioResult {
        underlying_price,
        total_pnl,
        legs,
    })
}

/// Evaluates a sorted set of expiry scenarios without inventing an unbounded loss/profit claim.
pub fn evaluate_expiry_scenarios(
    chain: &OptionChain,
    strategy: &OptionStrategy,
    underlying_prices: &[Decimal],
) -> Result<Vec<ExpiryScenarioResult>, OptionError> {
    if underlying_prices.is_empty() {
        return Err(OptionError(
            "at least one expiry scenario is required".to_owned(),
        ));
    }
    let mut prices = underlying_prices.to_vec();
    prices.sort();
    if prices.windows(2).any(|window| window[0] == window[1]) {
        return Err(OptionError(
            "expiry scenarios must have unique underlying prices".to_owned(),
        ));
    }
    prices
        .into_iter()
        .map(|price| evaluate_expiry_scenario(chain, strategy, price))
        .collect()
}

/// Settlement method for exercise and assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionSettlementMethod {
    /// Intrinsic value is posted as cash with no underlying delivery.
    Cash,
    /// Underlying units and strike cash are exchanged.
    Physical,
}

/// Final lifecycle outcome for an expiring position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionLifecycleOutcome {
    /// Position expired below the automatic-exercise threshold.
    Expired,
    /// A positive holder position exercised.
    Exercised,
    /// A negative writer position was assigned.
    Assigned,
}

/// Exact option, underlying, and cash changes at expiration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionLifecycleSettlement {
    /// Stable event identity supplied by the caller.
    pub lifecycle_event_id: String,
    /// Option instrument settled.
    pub option_id: String,
    /// Underlying instrument affected by physical settlement.
    pub underlying_instrument_id: String,
    /// Exercise, assignment, or worthless expiry.
    pub outcome: OptionLifecycleOutcome,
    /// Exact signed option quantity required to close the position.
    pub option_quantity_delta: Decimal,
    /// Exact signed underlying-unit delivery.
    pub underlying_quantity_delta: Decimal,
    /// Exact settlement-currency cash movement.
    pub cash_delta: Decimal,
    /// Intrinsic value per underlying unit.
    pub intrinsic_value: Decimal,
    /// Canonical recognition time.
    pub occurred_at: String,
}

/// Settles a complete signed option position at or after expiration.
///
/// Positive quantities are holder positions; negative quantities are writer
/// positions. Fractional contracts and pre-expiration settlement are rejected.
#[allow(clippy::too_many_arguments)]
pub fn settle_expired_option_position(
    lifecycle_event_id: &str,
    contract: &OptionContract,
    signed_contract_quantity: Decimal,
    underlying_price: Decimal,
    automatic_exercise_threshold: Decimal,
    settlement_method: OptionSettlementMethod,
    occurred_at: &str,
) -> Result<OptionLifecycleSettlement, OptionError> {
    validate_canonical_id("option lifecycle_event_id", lifecycle_event_id)?;
    contract.validate()?;
    validate_utc_timestamp("option lifecycle occurred_at", occurred_at)?;
    if parse_utc(occurred_at)? < parse_utc(&contract.expiration_at)?
        || signed_contract_quantity == Decimal::ZERO
        || signed_contract_quantity.scaled() % 100_000_000 != 0
        || underlying_price < Decimal::ZERO
        || automatic_exercise_threshold < Decimal::ZERO
    {
        return Err(OptionError(
            "invalid option expiration settlement request".to_owned(),
        ));
    }
    let intrinsic_value = contract.intrinsic_value(underlying_price)?;
    let option_quantity_delta = Decimal::ZERO.checked_sub(signed_contract_quantity)?;
    if intrinsic_value < automatic_exercise_threshold || intrinsic_value == Decimal::ZERO {
        return Ok(OptionLifecycleSettlement {
            lifecycle_event_id: lifecycle_event_id.to_owned(),
            option_id: contract.option_id.clone(),
            underlying_instrument_id: contract.underlying_instrument_id.clone(),
            outcome: OptionLifecycleOutcome::Expired,
            option_quantity_delta,
            underlying_quantity_delta: Decimal::ZERO,
            cash_delta: Decimal::ZERO,
            intrinsic_value,
            occurred_at: occurred_at.to_owned(),
        });
    }
    let outcome = if signed_contract_quantity > Decimal::ZERO {
        OptionLifecycleOutcome::Exercised
    } else {
        OptionLifecycleOutcome::Assigned
    };
    let signed_underlying_units = signed_contract_quantity.checked_mul(contract.multiplier)?;
    let (underlying_quantity_delta, cash_delta) = match settlement_method {
        OptionSettlementMethod::Cash => (
            Decimal::ZERO,
            intrinsic_value.checked_mul(signed_underlying_units)?,
        ),
        OptionSettlementMethod::Physical => {
            let underlying_quantity_delta = match contract.right {
                OptionRight::Call => signed_underlying_units,
                OptionRight::Put => Decimal::ZERO.checked_sub(signed_underlying_units)?,
            };
            let cash_delta = Decimal::ZERO
                .checked_sub(underlying_quantity_delta.checked_mul(contract.strike)?)?;
            (underlying_quantity_delta, cash_delta)
        }
    };
    Ok(OptionLifecycleSettlement {
        lifecycle_event_id: lifecycle_event_id.to_owned(),
        option_id: contract.option_id.clone(),
        underlying_instrument_id: contract.underlying_instrument_id.clone(),
        outcome,
        option_quantity_delta,
        underlying_quantity_delta,
        cash_delta,
        intrinsic_value,
        occurred_at: occurred_at.to_owned(),
    })
}

/// Immutable strategy/data/config/model identities that every environment must carry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionRunIdentity {
    /// SHA-256 of selected strategy bundle source.
    pub strategy_bundle_hash: String,
    /// SHA-256 of exact configuration bytes.
    pub configuration_hash: String,
    /// SHA-256 of selected input dataset.
    pub dataset_hash: String,
    /// SHA-256 of selected replay event output.
    pub replay_event_hash: String,
    /// SHA-256 of the complete frozen chain snapshot.
    pub chain_snapshot_hash: String,
    /// Deterministic pricing-model version.
    pub model_version: String,
}

impl OptionRunIdentity {
    /// Validates replay identities before book comparison.
    pub fn validate(&self) -> Result<(), OptionError> {
        for (name, value) in [
            ("strategy_bundle_hash", self.strategy_bundle_hash.as_str()),
            ("configuration_hash", self.configuration_hash.as_str()),
            ("dataset_hash", self.dataset_hash.as_str()),
            ("replay_event_hash", self.replay_event_hash.as_str()),
            ("chain_snapshot_hash", self.chain_snapshot_hash.as_str()),
        ] {
            validate_sha256(name, value)?;
        }
        if self.model_version != OPTION_MODEL_VERSION {
            return Err(OptionError(
                "unsupported option model version in run identity".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable SHA-256 of the complete immutable strategy/data/config/replay,
    /// chain, and model identity carried by one independent environment.
    pub fn fingerprint(&self) -> Result<String, OptionError> {
        self.validate()?;
        Ok(sha256(&format!(
            "strategy_bundle_hash={}\nconfiguration_hash={}\ndataset_hash={}\nreplay_event_hash={}\nchain_snapshot_hash={}\nmodel_version={}\n",
            self.strategy_bundle_hash,
            self.configuration_hash,
            self.dataset_hash,
            self.replay_event_hash,
            self.chain_snapshot_hash,
            self.model_version,
        )))
    }
}

/// Environment represented by a separately produced option-book projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionEnvironment {
    /// Deterministic simulation/backtest export.
    Backtest,
    /// Independently accounted paper-trading export.
    Paper,
    /// Independently accounted controlled-live export.
    Live,
}

impl OptionEnvironment {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, OptionError> {
        match value {
            "BACKTEST" => Ok(Self::Backtest),
            "PAPER" => Ok(Self::Paper),
            "LIVE" => Ok(Self::Live),
            _ => Err(OptionError(
                "option environment must be BACKTEST, PAPER, or LIVE".to_owned(),
            )),
        }
    }

    /// Stable external representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Backtest => "BACKTEST",
            Self::Paper => "PAPER",
            Self::Live => "LIVE",
        }
    }
}

/// One independently accounted option position for cross-environment comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionBookPosition {
    /// Canonical option identity.
    pub option_id: String,
    /// Signed whole contract quantity.
    pub quantity: Decimal,
    /// Exact average entry premium per underlying unit.
    pub average_entry_premium: Decimal,
    /// Exact selected mark premium per underlying unit.
    pub mark_premium: Decimal,
    /// Exact realized P&L in book currency.
    pub realized_pnl: Decimal,
}

/// One isolated backtest, paper, or live option-book projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionBook {
    /// Environment that produced this independent book.
    pub environment: OptionEnvironment,
    /// Canonical account identity from the independently exported source.
    pub account_id: String,
    /// Canonical immutable identity assigned to the source export.
    pub source_export_id: String,
    /// SHA-256 of the normalized source export used to construct this book.
    pub source_export_hash: String,
    /// Canonical common snapshot/reconciliation time.
    pub as_of: String,
    /// Single reporting currency.
    pub currency: String,
    /// Exact cash reported by that book.
    pub cash: Decimal,
    /// Strategy/data/config/model/chain identity carried by that book.
    pub identity: OptionRunIdentity,
    /// Independently accounted positions.
    pub positions: Vec<OptionBookPosition>,
}

impl OptionBook {
    /// Validates an isolated book; comparison never silently repairs it.
    pub fn validate(&self, chain: &OptionChain) -> Result<(), OptionError> {
        chain.validate()?;
        validate_canonical_id("option book account_id", &self.account_id)?;
        validate_canonical_id("option book source_export_id", &self.source_export_id)?;
        validate_sha256("option book source_export_hash", &self.source_export_hash)?;
        validate_utc_timestamp("option book as_of", &self.as_of)?;
        if self.as_of != chain.snapshot_at
            || self.currency != chain.currency()?
            || self.cash < Decimal::ZERO
        {
            return Err(OptionError(
                "option book must use chain snapshot time/currency and non-negative cash"
                    .to_owned(),
            ));
        }
        self.identity.validate()?;
        if self.identity.chain_snapshot_hash != chain.fingerprint()? {
            return Err(OptionError(
                "option book identity does not bind the selected chain".to_owned(),
            ));
        }
        let mut ids = BTreeSet::new();
        for position in &self.positions {
            validate_canonical_id("option book position option_id", &position.option_id)?;
            if !ids.insert(&position.option_id)
                || chain.contract(&position.option_id).is_none()
                || position.quantity.scaled() % 100_000_000 != 0
                || position.average_entry_premium < Decimal::ZERO
                || position.mark_premium < Decimal::ZERO
            {
                return Err(OptionError(
                    "option book positions must be unique listed whole contracts with valid premiums"
                        .to_owned(),
                ));
            }
        }
        let expected_source_export_hash = normalized_source_export_hash(self);
        if self.source_export_hash != expected_source_export_hash {
            return Err(OptionError(format!(
                "option book source_export_hash does not match the normalized source export: expected {expected_source_export_hash}"
            )));
        }
        Ok(())
    }

    /// Stable SHA-256 fingerprint of one fully validated environment-specific
    /// account export. The environment is deliberately included: matching
    /// economics across BACKTEST, PAPER, and LIVE must remain independently
    /// attributable rather than being represented by one shared blob.
    pub fn fingerprint(&self, chain: &OptionChain) -> Result<String, OptionError> {
        self.validate(chain)?;
        let mut positions = self.positions.clone();
        positions.sort_by(|left, right| left.option_id.cmp(&right.option_id));
        let mut canonical = format!(
            "environment={}\naccount_id={}\nsource_export_id={}\nsource_export_hash={}\nas_of={}\ncurrency={}\ncash={}\nstrategy_bundle_hash={}\nconfiguration_hash={}\ndataset_hash={}\nreplay_event_hash={}\nchain_snapshot_hash={}\nmodel_version={}\n",
            self.environment.as_str(),
            self.account_id,
            self.source_export_id,
            self.source_export_hash,
            self.as_of,
            self.currency,
            self.cash,
            self.identity.strategy_bundle_hash,
            self.identity.configuration_hash,
            self.identity.dataset_hash,
            self.identity.replay_event_hash,
            self.identity.chain_snapshot_hash,
            self.identity.model_version,
        );
        for position in positions {
            canonical.push_str(&format!(
                "option_id={}\nquantity={}\naverage_entry_premium={}\nmark_premium={}\nrealized_pnl={}\n",
                position.option_id,
                position.quantity,
                position.average_entry_premium,
                position.mark_premium,
                position.realized_pnl,
            ));
        }
        Ok(sha256(&canonical))
    }
}

fn normalized_source_export_hash(book: &OptionBook) -> String {
    let mut positions = book.positions.clone();
    positions.sort_by(|left, right| left.option_id.cmp(&right.option_id));
    let mut canonical = format!(
        "environment={}\naccount_id={}\nsource_export_id={}\nas_of={}\ncurrency={}\ncash={}\nstrategy_bundle_hash={}\nconfiguration_hash={}\ndataset_hash={}\nreplay_event_hash={}\nchain_snapshot_hash={}\nmodel_version={}\n",
        book.environment.as_str(),
        book.account_id,
        book.source_export_id,
        book.as_of,
        book.currency,
        book.cash,
        book.identity.strategy_bundle_hash,
        book.identity.configuration_hash,
        book.identity.dataset_hash,
        book.identity.replay_event_hash,
        book.identity.chain_snapshot_hash,
        book.identity.model_version,
    );
    for position in positions {
        canonical.push_str(&format!(
            "option_id={}\nquantity={}\naverage_entry_premium={}\nmark_premium={}\nrealized_pnl={}\n",
            position.option_id,
            position.quantity,
            position.average_entry_premium,
            position.mark_premium,
            position.realized_pnl,
        ));
    }
    sha256(&canonical)
}

/// Immutable provenance for one separately declared option-book export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionBookEvidence {
    /// Environment asserted by the source export.
    pub environment: OptionEnvironment,
    /// Canonical source account identity.
    pub account_id: String,
    /// Canonical immutable source-export identity.
    pub source_export_id: String,
    /// SHA-256 of the normalized source export.
    pub source_export_hash: String,
    /// SHA-256 of the complete validated book projection.
    pub book_hash: String,
    /// Complete run identity declared by this independently sourced export.
    pub run_identity: OptionRunIdentity,
    /// SHA-256 of the independently asserted strategy/data/config/replay,
    /// chain, and model identity.
    pub run_identity_hash: String,
}

/// One exact unresolved cross-environment difference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionReconciliationIssue {
    /// `IDENTITY_MISMATCH`, `CASH_MISMATCH`, or `POSITION_MISMATCH`.
    pub category: String,
    /// Environment/field or option contract being compared.
    pub subject: String,
    /// Backtest-side expected value.
    pub expected: String,
    /// Independently observed paper/live value.
    pub observed: String,
}

/// Result of exact option-book comparisons against one backtest source of truth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionReconciliationReport {
    /// Reconciliation snapshot instant.
    pub reconciled_at: String,
    /// Selected frozen chain fingerprint.
    pub chain_snapshot_hash: String,
    /// Provenance and fingerprint of the independently produced BACKTEST book.
    pub backtest_book: OptionBookEvidence,
    /// Provenance and fingerprint of the independently produced PAPER book.
    pub paper_book: OptionBookEvidence,
    /// Provenance and fingerprint of the independently produced LIVE book.
    pub live_book: OptionBookEvidence,
    /// Differences in stable category/subject/environment order.
    pub issues: Vec<OptionReconciliationIssue>,
}

impl OptionReconciliationReport {
    /// Whether backtest, paper, and live representations all agree exactly.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// Portable canonical JSON evidence for the reconciliation result.
    pub fn canonical_json(&self) -> String {
        let issues = self
            .issues
            .iter()
            .map(|issue| {
                format!(
                    "{{\"category\":{},\"expected\":{},\"observed\":{},\"subject\":{}}}",
                    json_string(&issue.category),
                    json_string(&issue.expected),
                    json_string(&issue.observed),
                    json_string(&issue.subject),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"backtest_book\":{},\"clean\":{},\"issues\":[{}],\"live_book\":{},\"paper_book\":{},\"reconciled_at\":{}}}",
            canonical_book_evidence_json(&self.backtest_book),
            self.is_clean(),
            issues,
            canonical_book_evidence_json(&self.live_book),
            canonical_book_evidence_json(&self.paper_book),
            json_string(&self.reconciled_at),
        )
    }
}

/// Reconciles exact independent option books without modifying any of them at
/// an explicitly supplied reconciliation instant. The instant must not precede
/// the frozen chain/book observation time.
pub fn reconcile_option_books_at(
    chain: &OptionChain,
    backtest: &OptionBook,
    paper: &OptionBook,
    live: &OptionBook,
    reconciled_at: &str,
) -> Result<OptionReconciliationReport, OptionError> {
    chain.validate()?;
    validate_utc_timestamp("options reconciled_at", reconciled_at)?;
    if parse_utc(reconciled_at)? < parse_utc(&chain.snapshot_at)? {
        return Err(OptionError(
            "options reconciled_at cannot precede the frozen chain snapshot".to_owned(),
        ));
    }
    backtest.validate(chain)?;
    paper.validate(chain)?;
    live.validate(chain)?;
    if backtest.environment != OptionEnvironment::Backtest
        || paper.environment != OptionEnvironment::Paper
        || live.environment != OptionEnvironment::Live
    {
        return Err(OptionError(
            "option reconciliation requires BACKTEST, PAPER, and LIVE books in fixed roles"
                .to_owned(),
        ));
    }
    let mut issues = Vec::new();
    compare_book_identity(backtest, paper, "PAPER", &mut issues);
    compare_book_identity(backtest, live, "LIVE", &mut issues);
    compare_book_economics(backtest, paper, "PAPER", &mut issues);
    compare_book_economics(backtest, live, "LIVE", &mut issues);
    issues.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.subject.cmp(&right.subject))
            .then_with(|| left.observed.cmp(&right.observed))
    });
    Ok(OptionReconciliationReport {
        reconciled_at: reconciled_at.to_owned(),
        chain_snapshot_hash: chain.fingerprint()?,
        backtest_book: book_evidence(backtest, chain)?,
        paper_book: book_evidence(paper, chain)?,
        live_book: book_evidence(live, chain)?,
        issues,
    })
}

fn book_evidence(
    book: &OptionBook,
    chain: &OptionChain,
) -> Result<OptionBookEvidence, OptionError> {
    Ok(OptionBookEvidence {
        environment: book.environment,
        account_id: book.account_id.clone(),
        source_export_id: book.source_export_id.clone(),
        source_export_hash: book.source_export_hash.clone(),
        book_hash: book.fingerprint(chain)?,
        run_identity: book.identity.clone(),
        run_identity_hash: book.identity.fingerprint()?,
    })
}

fn canonical_book_evidence_json(evidence: &OptionBookEvidence) -> String {
    format!(
        "{{\"account_id\":{},\"book_hash\":{},\"environment\":{},\"run_identity\":{},\"run_identity_hash\":{},\"source_export_hash\":{},\"source_export_id\":{}}}",
        json_string(&evidence.account_id),
        json_string(&evidence.book_hash),
        json_string(evidence.environment.as_str()),
        canonical_run_identity_json(&evidence.run_identity),
        json_string(&evidence.run_identity_hash),
        json_string(&evidence.source_export_hash),
        json_string(&evidence.source_export_id),
    )
}

fn canonical_run_identity_json(identity: &OptionRunIdentity) -> String {
    format!(
        "{{\"chain_snapshot_hash\":{},\"configuration_hash\":{},\"dataset_hash\":{},\"model_version\":{},\"replay_event_hash\":{},\"strategy_bundle_hash\":{}}}",
        json_string(&identity.chain_snapshot_hash),
        json_string(&identity.configuration_hash),
        json_string(&identity.dataset_hash),
        json_string(&identity.model_version),
        json_string(&identity.replay_event_hash),
        json_string(&identity.strategy_bundle_hash),
    )
}

fn compare_book_identity(
    expected: &OptionBook,
    observed: &OptionBook,
    environment: &str,
    issues: &mut Vec<OptionReconciliationIssue>,
) {
    for (field, left, right) in [
        (
            "strategy_bundle_hash",
            expected.identity.strategy_bundle_hash.as_str(),
            observed.identity.strategy_bundle_hash.as_str(),
        ),
        (
            "configuration_hash",
            expected.identity.configuration_hash.as_str(),
            observed.identity.configuration_hash.as_str(),
        ),
        (
            "dataset_hash",
            expected.identity.dataset_hash.as_str(),
            observed.identity.dataset_hash.as_str(),
        ),
        (
            "replay_event_hash",
            expected.identity.replay_event_hash.as_str(),
            observed.identity.replay_event_hash.as_str(),
        ),
        (
            "chain_snapshot_hash",
            expected.identity.chain_snapshot_hash.as_str(),
            observed.identity.chain_snapshot_hash.as_str(),
        ),
        (
            "model_version",
            expected.identity.model_version.as_str(),
            observed.identity.model_version.as_str(),
        ),
    ] {
        if left != right {
            issues.push(OptionReconciliationIssue {
                category: "IDENTITY_MISMATCH".to_owned(),
                subject: format!("{environment}.{field}"),
                expected: left.to_owned(),
                observed: right.to_owned(),
            });
        }
    }
}

fn compare_book_economics(
    expected: &OptionBook,
    observed: &OptionBook,
    environment: &str,
    issues: &mut Vec<OptionReconciliationIssue>,
) {
    if expected.cash != observed.cash {
        issues.push(OptionReconciliationIssue {
            category: "CASH_MISMATCH".to_owned(),
            subject: format!("{environment}.cash"),
            expected: expected.cash.to_string(),
            observed: observed.cash.to_string(),
        });
    }
    let expected_positions: BTreeMap<_, _> = expected
        .positions
        .iter()
        .map(|position| (position.option_id.as_str(), position))
        .collect();
    let observed_positions: BTreeMap<_, _> = observed
        .positions
        .iter()
        .map(|position| (position.option_id.as_str(), position))
        .collect();
    let option_ids: BTreeSet<_> = expected_positions
        .keys()
        .chain(observed_positions.keys())
        .copied()
        .collect();
    for option_id in option_ids {
        match (
            expected_positions.get(option_id),
            observed_positions.get(option_id),
        ) {
            (Some(left), Some(right)) => {
                for (field, expected_value, observed_value) in [
                    ("quantity", left.quantity, right.quantity),
                    (
                        "average_entry_premium",
                        left.average_entry_premium,
                        right.average_entry_premium,
                    ),
                    ("mark_premium", left.mark_premium, right.mark_premium),
                    ("realized_pnl", left.realized_pnl, right.realized_pnl),
                ] {
                    if expected_value != observed_value {
                        issues.push(OptionReconciliationIssue {
                            category: "POSITION_MISMATCH".to_owned(),
                            subject: format!("{environment}.{option_id}.{field}"),
                            expected: expected_value.to_string(),
                            observed: observed_value.to_string(),
                        });
                    }
                }
            }
            (Some(_), None) => issues.push(OptionReconciliationIssue {
                category: "POSITION_MISMATCH".to_owned(),
                subject: format!("{environment}.{option_id}"),
                expected: "present".to_owned(),
                observed: "missing".to_owned(),
            }),
            (None, Some(_)) => issues.push(OptionReconciliationIssue {
                category: "POSITION_MISMATCH".to_owned(),
                subject: format!("{environment}.{option_id}"),
                expected: "missing".to_owned(),
                observed: "present".to_owned(),
            }),
            (None, None) => unreachable!("option ID originated from one of the books"),
        }
    }
}

fn time_to_expiry_years(as_of: &str, expiration_at: &str) -> Result<Decimal, OptionError> {
    validate_utc_timestamp("option valuation as_of", as_of)?;
    validate_utc_timestamp("option expiration_at", expiration_at)?;
    let as_of = OffsetDateTime::parse(as_of, &Rfc3339)
        .map_err(|_| OptionError("invalid option valuation time".to_owned()))?;
    let expiration = OffsetDateTime::parse(expiration_at, &Rfc3339)
        .map_err(|_| OptionError("invalid option expiration time".to_owned()))?;
    let seconds = (expiration - as_of).whole_seconds();
    if seconds <= 0 {
        return Err(OptionError(
            "option valuation must precede expiration".to_owned(),
        ));
    }
    Decimal::from_integer(seconds)?
        .checked_div(Decimal::from_integer(31_536_000)?)
        .map_err(Into::into)
}

fn parse_utc(value: &str) -> Result<OffsetDateTime, OptionError> {
    validate_utc_timestamp("option UTC time", value)?;
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| OptionError("invalid option UTC time".to_owned()))
}

fn ln(value: Decimal) -> Result<Decimal, OptionError> {
    if value <= Decimal::ZERO {
        return Err(OptionError(
            "natural logarithm requires a positive value".to_owned(),
        ));
    }
    let upper = Decimal::from_scaled(150_000_000);
    let lower = Decimal::from_scaled(75_000_000);
    let mut normalized = value;
    let mut exponent: i64 = 0;
    while normalized > upper {
        normalized = normalized.checked_div(TWO)?;
        exponent += 1;
        if exponent > 128 {
            return Err(OptionError(
                "logarithm input is outside model range".to_owned(),
            ));
        }
    }
    while normalized < lower {
        normalized = normalized.checked_mul(TWO)?;
        exponent -= 1;
        if exponent < -128 {
            return Err(OptionError(
                "logarithm input is outside model range".to_owned(),
            ));
        }
    }
    let y = normalized
        .checked_sub(ONE)?
        .checked_div(normalized.checked_add(ONE)?)?;
    let y_squared = y.checked_mul(y)?;
    let mut term = y;
    let mut sum = y;
    for divisor in (3_i64..=63).step_by(2) {
        term = term.checked_mul(y_squared)?;
        sum = sum.checked_add(term.checked_div(Decimal::from_integer(divisor)?)?)?;
    }
    TWO.checked_mul(sum)?
        .checked_add(Decimal::from_integer(exponent)?.checked_mul(LN_2)?)
        .map_err(Into::into)
}

fn exp(value: Decimal) -> Result<Decimal, OptionError> {
    let exponent = value
        .scaled()
        .checked_div(LN_2.scaled())
        .ok_or_else(|| OptionError("exponential range reduction failed".to_owned()))?;
    let exponent = i64::try_from(exponent)
        .map_err(|_| OptionError("exponential input is outside model range".to_owned()))?;
    if !(-128..=128).contains(&exponent) {
        return Err(OptionError(
            "exponential input is outside model range".to_owned(),
        ));
    }
    let remainder = value.checked_sub(Decimal::from_integer(exponent)?.checked_mul(LN_2)?)?;
    let mut term = ONE;
    let mut sum = ONE;
    for divisor in 1_i64..=48 {
        term = term
            .checked_mul(remainder)?
            .checked_div(Decimal::from_integer(divisor)?)?;
        sum = sum.checked_add(term)?;
    }
    if exponent >= 0 {
        for _ in 0..exponent {
            sum = sum.checked_mul(TWO)?;
        }
    } else {
        for _ in exponent..0 {
            sum = sum.checked_div(TWO)?;
        }
    }
    Ok(sum)
}

fn sqrt(value: Decimal) -> Result<Decimal, OptionError> {
    if value < Decimal::ZERO {
        return Err(OptionError(
            "square root requires a non-negative value".to_owned(),
        ));
    }
    if value == Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }
    let mut guess = if value > ONE {
        value.checked_div(TWO)?
    } else {
        ONE
    };
    for _ in 0..48 {
        guess = guess
            .checked_add(value.checked_div(guess)?)?
            .checked_div(TWO)?;
    }
    Ok(guess)
}

fn normal_density(value: Decimal) -> Result<Decimal, OptionError> {
    if absolute(value)? >= Decimal::from_integer(8)? {
        return Ok(Decimal::ZERO);
    }
    let exponent = Decimal::ZERO.checked_sub(value.checked_mul(value)?.checked_div(TWO)?)?;
    INV_SQRT_TWO_PI
        .checked_mul(exp(exponent)?)
        .map_err(Into::into)
}

fn normal_cdf(value: Decimal) -> Result<Decimal, OptionError> {
    let boundary = Decimal::from_integer(8)?;
    if value <= Decimal::ZERO.checked_sub(boundary)? {
        return Ok(Decimal::ZERO);
    }
    if value >= boundary {
        return Ok(ONE);
    }
    let negative = value < Decimal::ZERO;
    let absolute_value = absolute(value)?;
    let t = ONE.checked_div(ONE.checked_add(CDF_P.checked_mul(absolute_value)?)?)?;
    let polynomial = CDF_A5
        .checked_mul(t)?
        .checked_add(CDF_A4)?
        .checked_mul(t)?
        .checked_add(CDF_A3)?
        .checked_mul(t)?
        .checked_add(CDF_A2)?
        .checked_mul(t)?
        .checked_add(CDF_A1)?
        .checked_mul(t)?;
    let positive = ONE.checked_sub(normal_density(absolute_value)?.checked_mul(polynomial)?)?;
    if negative {
        ONE.checked_sub(positive).map_err(Into::into)
    } else {
        Ok(positive)
    }
}

fn positive_part(value: Decimal) -> Result<Decimal, OptionError> {
    if value < Decimal::ZERO {
        Ok(Decimal::ZERO)
    } else {
        Ok(value)
    }
}

fn absolute(value: Decimal) -> Result<Decimal, OptionError> {
    if value < Decimal::ZERO {
        Decimal::ZERO.checked_sub(value).map_err(Into::into)
    } else {
        Ok(value)
    }
}

fn validate_sha256(name: &str, value: &str) -> Result<(), OptionError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OptionError(format!(
            "{name} must be a lowercase SHA-256 hash"
        )));
    }
    Ok(())
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn decimal(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn chain() -> OptionChain {
        OptionChain {
            chain_id: "chain.spy.2026.12.18".to_owned(),
            underlying_instrument_id: "inst.us_equity.spy".to_owned(),
            snapshot_at: "2026-08-10T16:30:00Z".to_owned(),
            underlying_mark: decimal("100"),
            reference_version: "ref.options.1".to_owned(),
            contracts: vec![
                OptionContract {
                    option_id: "opt.spy.20261218.c100".to_owned(),
                    underlying_instrument_id: "inst.us_equity.spy".to_owned(),
                    expiration_at: "2026-12-18T21:00:00Z".to_owned(),
                    strike: decimal("100"),
                    right: OptionRight::Call,
                    multiplier: decimal("100"),
                    currency: "USD".to_owned(),
                    reference_version: "ref.options.1".to_owned(),
                },
                OptionContract {
                    option_id: "opt.spy.20261218.p95".to_owned(),
                    underlying_instrument_id: "inst.us_equity.spy".to_owned(),
                    expiration_at: "2026-12-18T21:00:00Z".to_owned(),
                    strike: decimal("95"),
                    right: OptionRight::Put,
                    multiplier: decimal("100"),
                    currency: "USD".to_owned(),
                    reference_version: "ref.options.1".to_owned(),
                },
            ],
            quotes: vec![
                OptionQuote {
                    option_id: "opt.spy.20261218.c100".to_owned(),
                    observed_at: "2026-08-10T16:30:00Z".to_owned(),
                    bid: decimal("5.10"),
                    ask: decimal("5.30"),
                    last: decimal("5.20"),
                    volume: 12,
                    open_interest: 100,
                },
                OptionQuote {
                    option_id: "opt.spy.20261218.p95".to_owned(),
                    observed_at: "2026-08-10T16:30:00Z".to_owned(),
                    bid: decimal("3.50"),
                    ask: decimal("3.70"),
                    last: decimal("3.60"),
                    volume: 8,
                    open_interest: 90,
                },
            ],
        }
    }

    fn identity(chain: &OptionChain) -> OptionRunIdentity {
        OptionRunIdentity {
            strategy_bundle_hash: "a".repeat(64),
            configuration_hash: "b".repeat(64),
            dataset_hash: "c".repeat(64),
            replay_event_hash: "d".repeat(64),
            chain_snapshot_hash: chain.fingerprint().unwrap(),
            model_version: OPTION_MODEL_VERSION.to_owned(),
        }
    }

    fn book(chain: &OptionChain, environment: OptionEnvironment) -> OptionBook {
        let source_suffix = environment.as_str().to_ascii_lowercase();
        let mut book = OptionBook {
            environment,
            account_id: format!("acct.options.{source_suffix}"),
            source_export_id: format!("export.options.{source_suffix}.001"),
            source_export_hash: String::new(),
            as_of: chain.snapshot_at.clone(),
            currency: "USD".to_owned(),
            cash: decimal("10000"),
            identity: identity(chain),
            positions: vec![OptionBookPosition {
                option_id: "opt.spy.20261218.c100".to_owned(),
                quantity: decimal("1"),
                average_entry_premium: decimal("5.20"),
                mark_premium: decimal("5.20"),
                realized_pnl: Decimal::ZERO,
            }],
        };
        book.source_export_hash = normalized_source_export_hash(&book);
        book
    }

    #[test]
    fn chain_analytics_are_repeatable_with_implied_volatility_and_greeks() {
        let chain = chain();
        let first = analyze_chain(&chain, decimal("0.04")).unwrap();
        let second = analyze_chain(&chain, decimal("0.04")).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert!(first[0].implied_volatility > Decimal::ZERO);
        assert!(first[0].greeks.model_price > Decimal::ZERO);
        assert_eq!(chain.fingerprint().unwrap(), chain.fingerprint().unwrap());
    }

    #[test]
    fn expiry_multi_leg_scenarios_are_exact_and_sorted() {
        let chain = chain();
        let strategy = OptionStrategy {
            strategy_id: "strategy.spy.call_spread".to_owned(),
            strategy_version: "1".to_owned(),
            legs: vec![OptionStrategyLeg {
                leg_id: "leg.long_call".to_owned(),
                option_id: "opt.spy.20261218.c100".to_owned(),
                side: OptionLegSide::Long,
                quantity: decimal("1"),
                entry_premium: decimal("5.20"),
            }],
        };
        let scenarios = evaluate_expiry_scenarios(
            &chain,
            &strategy,
            &[decimal("110"), decimal("90"), decimal("100")],
        )
        .unwrap();
        assert_eq!(scenarios[0].underlying_price, decimal("90"));
        assert_eq!(scenarios[0].total_pnl, decimal("-520"));
        assert_eq!(scenarios[2].total_pnl, decimal("480"));
    }

    #[test]
    fn expiration_lifecycle_handles_exercise_assignment_and_worthless_expiry() {
        let chain = chain();
        let call = &chain.contracts[0];
        let exercised = settle_expired_option_position(
            "lifecycle.call.long",
            call,
            decimal("2"),
            decimal("110"),
            decimal("0.01"),
            OptionSettlementMethod::Physical,
            &call.expiration_at,
        )
        .expect("exercise");
        assert_eq!(exercised.outcome, OptionLifecycleOutcome::Exercised);
        assert_eq!(exercised.option_quantity_delta, decimal("-2"));
        assert_eq!(exercised.underlying_quantity_delta, decimal("200"));
        assert_eq!(exercised.cash_delta, decimal("-20000"));

        let put = &chain.contracts[1];
        let assigned = settle_expired_option_position(
            "lifecycle.put.short",
            put,
            decimal("-1"),
            decimal("90"),
            decimal("0.01"),
            OptionSettlementMethod::Physical,
            &put.expiration_at,
        )
        .expect("assignment");
        assert_eq!(assigned.outcome, OptionLifecycleOutcome::Assigned);
        assert_eq!(assigned.underlying_quantity_delta, decimal("100"));
        assert_eq!(assigned.cash_delta, decimal("-9500"));

        let expired = settle_expired_option_position(
            "lifecycle.call.expired",
            call,
            decimal("1"),
            decimal("90"),
            decimal("0.01"),
            OptionSettlementMethod::Cash,
            &call.expiration_at,
        )
        .expect("expiry");
        assert_eq!(expired.outcome, OptionLifecycleOutcome::Expired);
        assert_eq!(expired.cash_delta, Decimal::ZERO);
    }

    #[test]
    fn independently_equal_books_reconcile_and_a_difference_stays_visible() {
        let chain = chain();
        let backtest = book(&chain, OptionEnvironment::Backtest);
        let paper = book(&chain, OptionEnvironment::Paper);
        let mut live = book(&chain, OptionEnvironment::Live);
        let clean =
            reconcile_option_books_at(&chain, &backtest, &paper, &live, "2026-08-10T16:31:00Z")
                .unwrap();
        assert!(clean.is_clean());
        assert_ne!(clean.backtest_book.book_hash, clean.paper_book.book_hash);
        assert_ne!(clean.paper_book.book_hash, clean.live_book.book_hash);
        assert_eq!(
            clean.backtest_book.run_identity_hash,
            clean.paper_book.run_identity_hash
        );
        assert_eq!(
            clean.backtest_book.book_hash,
            backtest.fingerprint(&chain).unwrap()
        );
        live.positions[0].quantity = decimal("2");
        live.source_export_hash = normalized_source_export_hash(&live);
        let report =
            reconcile_option_books_at(&chain, &backtest, &paper, &live, "2026-08-10T16:31:00Z")
                .unwrap();
        assert!(!report.is_clean());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.subject == "LIVE.opt.spy.20261218.c100.quantity"));
    }

    #[test]
    fn invalid_option_chain_or_identity_fails_closed() {
        let mut invalid_chain = chain();
        invalid_chain.quotes[0].ask = decimal("1");
        invalid_chain.quotes[0].bid = decimal("2");
        assert!(invalid_chain.validate().is_err());
        let mut mixed_reference_chain = chain();
        mixed_reference_chain.contracts[0].reference_version = "ref.options.2".to_owned();
        assert!(mixed_reference_chain.validate().is_err());
        let valid_chain = chain();
        let mut mismatched_paper = book(&valid_chain, OptionEnvironment::Paper);
        mismatched_paper.identity.dataset_hash = "e".repeat(64);
        mismatched_paper.source_export_hash = normalized_source_export_hash(&mismatched_paper);
        assert!(mismatched_paper.validate(&valid_chain).is_ok());
        let mut tampered_source = book(&valid_chain, OptionEnvironment::Live);
        tampered_source.positions[0].quantity = decimal("2");
        assert!(tampered_source.validate(&valid_chain).is_err());
        let expected = book(&valid_chain, OptionEnvironment::Backtest);
        let paper = book(&valid_chain, OptionEnvironment::Paper);
        let live = book(&valid_chain, OptionEnvironment::Live);
        assert!(!reconcile_option_books_at(
            &valid_chain,
            &expected,
            &mismatched_paper,
            &live,
            "2026-08-10T16:31:00Z",
        )
        .unwrap()
        .is_clean());
        let identity_report = reconcile_option_books_at(
            &valid_chain,
            &expected,
            &mismatched_paper,
            &live,
            "2026-08-10T16:31:00Z",
        )
        .unwrap();
        assert!(identity_report
            .issues
            .iter()
            .any(|issue| issue.subject == "PAPER.dataset_hash"));
        assert!(reconcile_option_books_at(
            &valid_chain,
            &expected,
            &paper,
            &live,
            "2026-08-10T16:31:00Z",
        )
        .unwrap()
        .is_clean());
        assert!(reconcile_option_books_at(
            &valid_chain,
            &expected,
            &paper,
            &live,
            "2026-08-10T16:31:00Z",
        )
        .is_ok());
        assert!(reconcile_option_books_at(
            &valid_chain,
            &expected,
            &paper,
            &live,
            "2026-08-10T16:29:59Z",
        )
        .is_err());
    }

    #[test]
    fn test_volatility_surface_and_news_shock() {
        let chain = chain();
        let rate = decimal("0.05");

        // 1. Surface generation & fingerprint stability
        let surface = generate_volatility_surface(&chain, rate).expect("surface");
        assert_eq!(surface.points.len(), 2);
        let fp1 = surface.fingerprint().expect("fingerprint");
        let fp2 = surface.fingerprint().expect("fingerprint");
        assert_eq!(fp1, fp2);

        // 2. News volatility shock scenario (+500 BPS = +5.0% IV)
        let shock_res =
            evaluate_news_volatility_shock(&chain, decimal("500"), rate).expect("shock");
        assert!(shock_res.post_shock_model_value > shock_res.pre_shock_model_value);
        assert!(shock_res.vega_pnl > Decimal::ZERO);
        assert!(shock_res.mean_post_shock_iv > shock_res.mean_pre_shock_iv);
    }
}

//! Exact, broker-neutral multi-currency accounting and margin contracts.
//!
//! Journal entries balance independently in every currency. Valuation never
//! invents an FX rate: a missing or stale quote fails the entire snapshot.

pub mod statement;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};

/// Accounting or valuation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountingError(pub String);

impl fmt::Display for AccountingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AccountingError {}

impl From<follon_domain::DecimalError> for AccountingError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

/// ISO-4217-style uppercase three-letter currency code.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Currency(String);

impl Currency {
    /// Validates and creates a currency.
    pub fn new(code: impl Into<String>) -> Result<Self, AccountingError> {
        let code = code.into();
        if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(AccountingError(
                "currency must be a three-letter uppercase code".to_owned(),
            ));
        }
        Ok(Self(code))
    }

    /// Returns the canonical code.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One double-entry journal line. Exactly one side must be positive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalLine {
    /// Canonical general-ledger account ID.
    pub account_id: String,
    /// Posting currency.
    pub currency: Currency,
    /// Debit amount.
    pub debit: Decimal,
    /// Credit amount.
    pub credit: Decimal,
}

impl JournalLine {
    fn validate(&self) -> Result<(), AccountingError> {
        validate_canonical_id("ledger account_id", &self.account_id)
            .map_err(|error| AccountingError(error.0))?;
        let debit_positive = self.debit > Decimal::ZERO;
        let credit_positive = self.credit > Decimal::ZERO;
        if self.debit < Decimal::ZERO
            || self.credit < Decimal::ZERO
            || debit_positive == credit_positive
        {
            return Err(AccountingError(
                "journal line must have exactly one positive debit or credit".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Atomic, idempotent accounting transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalTransaction {
    /// Globally unique canonical transaction ID.
    pub transaction_id: String,
    /// Tenant owning every line in this transaction.
    pub tenant_id: String,
    /// Durable business reference, such as a fill or transfer ID.
    pub reference_id: String,
    /// Balanced lines, grouped independently by currency.
    pub lines: Vec<JournalLine>,
}

/// In-memory projection of an append-only double-entry journal.
#[derive(Clone, Debug, Default)]
pub struct MultiCurrencyLedger {
    balances: BTreeMap<(String, Currency), Decimal>,
    applied_transactions: BTreeSet<String>,
}

impl MultiCurrencyLedger {
    /// Posts a balanced transaction exactly once.
    ///
    /// The duplicate case is an idempotent no-op. Reusing the ID with changed
    /// content must be prevented by the durable event store fingerprint.
    pub fn post(&mut self, transaction: &JournalTransaction) -> Result<bool, AccountingError> {
        validate_canonical_id("transaction_id", &transaction.transaction_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_canonical_id("tenant_id", &transaction.tenant_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_canonical_id("reference_id", &transaction.reference_id)
            .map_err(|error| AccountingError(error.0))?;
        if self
            .applied_transactions
            .contains(&transaction.transaction_id)
        {
            return Ok(false);
        }
        if transaction.lines.len() < 2 {
            return Err(AccountingError(
                "journal transaction requires at least two lines".to_owned(),
            ));
        }

        let mut currency_totals: BTreeMap<Currency, (Decimal, Decimal)> = BTreeMap::new();
        for line in &transaction.lines {
            line.validate()?;
            let totals = currency_totals
                .entry(line.currency.clone())
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            totals.0 = totals.0.checked_add(line.debit)?;
            totals.1 = totals.1.checked_add(line.credit)?;
        }
        if currency_totals
            .values()
            .any(|(debits, credits)| debits != credits)
        {
            return Err(AccountingError(
                "journal transaction is not balanced in every currency".to_owned(),
            ));
        }

        let mut projected = self.balances.clone();
        for line in &transaction.lines {
            let key = (line.account_id.clone(), line.currency.clone());
            let current = projected.get(&key).copied().unwrap_or(Decimal::ZERO);
            let signed = line.debit.checked_sub(line.credit)?;
            projected.insert(key, current.checked_add(signed)?);
        }
        self.balances = projected;
        self.applied_transactions
            .insert(transaction.transaction_id.clone());
        Ok(true)
    }

    /// Returns the debit-positive balance for an account and currency.
    pub fn balance(&self, account_id: &str, currency: &Currency) -> Decimal {
        self.balances
            .get(&(account_id.to_owned(), currency.clone()))
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    /// Returns a stable snapshot of all ledger balances.
    pub fn balances(&self) -> &BTreeMap<(String, Currency), Decimal> {
        &self.balances
    }
}

/// A timestamped direct FX quote: one unit of `base` costs `quote_rate` units
/// of `quote`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxQuote {
    /// Base currency.
    pub base: Currency,
    /// Quote currency.
    pub quote: Currency,
    /// Exact positive conversion rate.
    pub quote_rate: Decimal,
    /// Observation time as Unix seconds.
    pub observed_at_epoch_seconds: i64,
}

/// Explicit FX rate book supporting direct and inverse conversions.
#[derive(Clone, Debug, Default)]
pub struct FxBook {
    quotes: BTreeMap<(Currency, Currency), FxQuote>,
}

impl FxBook {
    /// Inserts or replaces one direct quote.
    pub fn upsert(&mut self, quote: FxQuote) -> Result<(), AccountingError> {
        if quote.base == quote.quote || quote.quote_rate <= Decimal::ZERO {
            return Err(AccountingError("invalid FX quote".to_owned()));
        }
        self.quotes
            .insert((quote.base.clone(), quote.quote.clone()), quote);
        Ok(())
    }

    /// Converts an amount and fails if no fresh direct or inverse rate exists.
    pub fn convert(
        &self,
        amount: Decimal,
        from: &Currency,
        to: &Currency,
        as_of_epoch_seconds: i64,
        maximum_age_seconds: i64,
    ) -> Result<Decimal, AccountingError> {
        if from == to {
            return Ok(amount);
        }
        if maximum_age_seconds < 0 {
            return Err(AccountingError(
                "FX maximum age cannot be negative".to_owned(),
            ));
        }
        if let Some(quote) = self.quotes.get(&(from.clone(), to.clone())) {
            ensure_fresh(quote, as_of_epoch_seconds, maximum_age_seconds)?;
            return Ok(amount.checked_mul(quote.quote_rate)?);
        }
        if let Some(quote) = self.quotes.get(&(to.clone(), from.clone())) {
            ensure_fresh(quote, as_of_epoch_seconds, maximum_age_seconds)?;
            return Ok(amount.checked_div(quote.quote_rate)?);
        }
        Err(AccountingError(format!(
            "missing FX rate {}->{}",
            from.as_str(),
            to.as_str()
        )))
    }
}

fn ensure_fresh(
    quote: &FxQuote,
    as_of_epoch_seconds: i64,
    maximum_age_seconds: i64,
) -> Result<(), AccountingError> {
    let age = as_of_epoch_seconds
        .checked_sub(quote.observed_at_epoch_seconds)
        .ok_or_else(|| AccountingError("FX quote age overflow".to_owned()))?;
    if age < 0 || age > maximum_age_seconds {
        return Err(AccountingError(format!(
            "stale FX rate {}->{}",
            quote.base.as_str(),
            quote.quote.as_str()
        )));
    }
    Ok(())
}

/// Marked position used for portfolio margin calculation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarginPosition {
    /// Canonical instrument identifier.
    pub instrument_id: String,
    /// Margin class (for example `equity`, `future`, or `option`).
    pub asset_class: String,
    /// Settlement currency.
    pub currency: Currency,
    /// Signed quantity.
    pub quantity: Decimal,
    /// Positive mark price.
    pub mark_price: Decimal,
    /// Positive contract multiplier.
    pub multiplier: Decimal,
}

/// One independently reconciled account snapshot eligible for aggregation.
///
/// This is deliberately a read-only projection input. It does not imply that
/// balances can be transferred, positions can be netted for settlement, or an
/// order approved for one account may be submitted for another.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountPortfolioSnapshot {
    /// Canonical account identity.
    pub account_id: String,
    /// Canonical point-in-time at which every cash and position value is valid.
    pub as_of: String,
    /// Immutable reconciliation evidence identity for this account snapshot.
    pub reconciliation_id: String,
    /// SHA-256 fingerprint of the shared valuation/configuration inputs.
    pub configuration_fingerprint: String,
    /// Exact available or debit cash by native currency.
    pub cash_by_currency: BTreeMap<Currency, Decimal>,
    /// Independently marked positions for this account.
    pub positions: Vec<MarginPosition>,
}

/// Deterministic cross-account position projection for one complete identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregatedPosition {
    /// Canonical instrument identifier.
    pub instrument_id: String,
    /// Margin/asset class supplied by the account snapshots.
    pub asset_class: String,
    /// Settlement currency.
    pub currency: Currency,
    /// Exact signed quantity summed across contributing accounts.
    pub quantity: Decimal,
    /// Common positive contract multiplier validated across all sources.
    pub multiplier: Decimal,
    /// Exact signed native-currency marked value summed from each source mark.
    ///
    /// The projection intentionally does not manufacture a single consolidated
    /// mark price when venues supplied different authoritative marks.
    pub market_value: Decimal,
    /// Canonical source accounts in stable order for audit attribution.
    pub contributing_account_ids: Vec<String>,
}

/// Read-only aggregate of reconciled account snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiAccountPortfolioSnapshot {
    /// Shared canonical point-in-time accepted for every source snapshot.
    pub as_of: String,
    /// Shared SHA-256 valuation/configuration fingerprint.
    pub configuration_fingerprint: String,
    /// Number of independently supplied accounts.
    pub account_count: u32,
    /// Reconciliation evidence identity retained by source account.
    pub reconciliation_ids_by_account: BTreeMap<String, String>,
    /// Exact total cash by native currency, including debit balances.
    pub cash_by_currency: BTreeMap<Currency, Decimal>,
    /// Positions in stable `(instrument, asset class, currency)` order.
    pub positions: Vec<AggregatedPosition>,
}

#[derive(Clone, Debug)]
struct AggregatedPositionAccumulator {
    quantity: Decimal,
    multiplier: Decimal,
    market_value: Decimal,
    contributing_account_ids: BTreeSet<String>,
}

/// Aggregates cash and marked positions from independently reconciled accounts.
///
/// The result is deterministic irrespective of input snapshot order. It uses
/// fixed-point values only and retains native currencies and source-account
/// attribution, so consumers cannot silently invent an FX rate or a blended
/// venue mark. An account or source position may occur only once per snapshot.
pub fn aggregate_account_portfolios(
    accounts: &[AccountPortfolioSnapshot],
) -> Result<MultiAccountPortfolioSnapshot, AccountingError> {
    if accounts.is_empty() || accounts.len() > u32::MAX as usize {
        return Err(AccountingError(
            "multi-account aggregation requires between one and u32::MAX accounts".to_owned(),
        ));
    }
    let mut account_ids = BTreeSet::new();
    let mut as_of = None;
    let mut configuration_fingerprint = None;
    let mut reconciliation_ids_by_account = BTreeMap::new();
    let mut cash_by_currency = BTreeMap::new();
    let mut positions: BTreeMap<(String, String, Currency), AggregatedPositionAccumulator> =
        BTreeMap::new();

    for account in accounts {
        validate_canonical_id("aggregate account_id", &account.account_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_utc_timestamp("aggregate snapshot as_of", &account.as_of)
            .map_err(|error| AccountingError(error.0))?;
        validate_canonical_id("aggregate reconciliation_id", &account.reconciliation_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_sha256_fingerprint(
            "aggregate configuration_fingerprint",
            &account.configuration_fingerprint,
        )?;
        if !account_ids.insert(account.account_id.clone()) {
            return Err(AccountingError(
                "multi-account aggregation received duplicate account_id".to_owned(),
            ));
        }
        match as_of.as_deref() {
            Some(existing) if existing != account.as_of => {
                return Err(AccountingError(
                    "multi-account aggregation requires one common as_of timestamp".to_owned(),
                ));
            }
            None => as_of = Some(account.as_of.clone()),
            _ => {}
        }
        match configuration_fingerprint.as_deref() {
            Some(existing) if existing != account.configuration_fingerprint => {
                return Err(AccountingError(
                    "multi-account aggregation requires one common configuration fingerprint"
                        .to_owned(),
                ));
            }
            None => configuration_fingerprint = Some(account.configuration_fingerprint.clone()),
            _ => {}
        }
        reconciliation_ids_by_account.insert(
            account.account_id.clone(),
            account.reconciliation_id.clone(),
        );
        for (currency, cash) in &account.cash_by_currency {
            let total = cash_by_currency
                .get(currency)
                .copied()
                .unwrap_or(Decimal::ZERO)
                .checked_add(*cash)?;
            cash_by_currency.insert(currency.clone(), total);
        }

        let mut source_position_ids = BTreeSet::new();
        for position in &account.positions {
            validate_canonical_id("aggregate instrument_id", &position.instrument_id)
                .map_err(|error| AccountingError(error.0))?;
            validate_canonical_id("aggregate asset_class", &position.asset_class)
                .map_err(|error| AccountingError(error.0))?;
            if position.quantity == Decimal::ZERO
                || position.mark_price <= Decimal::ZERO
                || position.multiplier <= Decimal::ZERO
            {
                return Err(AccountingError(
                    "invalid position in multi-account aggregation".to_owned(),
                ));
            }
            let key = (
                position.instrument_id.clone(),
                position.asset_class.clone(),
                position.currency.clone(),
            );
            if !source_position_ids.insert(key.clone()) {
                return Err(AccountingError(
                    "account snapshot has duplicate position identity".to_owned(),
                ));
            }
            let market_value = position
                .quantity
                .checked_mul(position.mark_price)?
                .checked_mul(position.multiplier)?;
            let aggregate = positions
                .entry(key)
                .or_insert(AggregatedPositionAccumulator {
                    quantity: Decimal::ZERO,
                    multiplier: position.multiplier,
                    market_value: Decimal::ZERO,
                    contributing_account_ids: BTreeSet::new(),
                });
            if aggregate.multiplier != position.multiplier {
                return Err(AccountingError(
                    "account snapshots disagree on a position contract multiplier".to_owned(),
                ));
            }
            aggregate.quantity = aggregate.quantity.checked_add(position.quantity)?;
            aggregate.market_value = aggregate.market_value.checked_add(market_value)?;
            aggregate
                .contributing_account_ids
                .insert(account.account_id.clone());
        }
    }

    let positions = positions
        .into_iter()
        .map(
            |((instrument_id, asset_class, currency), aggregate)| AggregatedPosition {
                instrument_id,
                asset_class,
                currency,
                quantity: aggregate.quantity,
                multiplier: aggregate.multiplier,
                market_value: aggregate.market_value,
                contributing_account_ids: aggregate.contributing_account_ids.into_iter().collect(),
            },
        )
        .collect();
    Ok(MultiAccountPortfolioSnapshot {
        as_of: as_of.expect("non-empty account aggregation has an as_of timestamp"),
        configuration_fingerprint: configuration_fingerprint
            .expect("non-empty account aggregation has a configuration fingerprint"),
        account_count: accounts.len() as u32,
        reconciliation_ids_by_account,
        cash_by_currency,
        positions,
    })
}

fn validate_sha256_fingerprint(name: &str, value: &str) -> Result<(), AccountingError> {
    if value.len() != 64
        || !value.bytes().all(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'a' | b'b' | b'c' | b'd' | b'e' | b'f')
        })
    {
        return Err(AccountingError(format!(
            "{name} must be a lowercase SHA-256 hex digest"
        )));
    }
    Ok(())
}

/// Initial and maintenance requirements in basis points of absolute marked value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarginRate {
    /// Initial margin basis points.
    pub initial_bps: u32,
    /// Maintenance margin basis points.
    pub maintenance_bps: u32,
}

/// Fail-closed margin rules by asset class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarginPolicy {
    /// Base reporting currency.
    pub base_currency: Currency,
    /// Maximum accepted FX quote age.
    pub maximum_fx_age_seconds: i64,
    /// Rates keyed by exact asset class.
    pub rates: BTreeMap<String, MarginRate>,
}

/// Portfolio-wide multi-currency equity and margin snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarginSnapshot {
    /// Reporting currency.
    pub base_currency: Currency,
    /// Converted cash including debit balances and margin loans.
    pub cash_value: Decimal,
    /// Signed converted position market value.
    pub position_market_value: Decimal,
    /// Net liquidation value.
    pub net_liquidation_value: Decimal,
    /// Initial margin requirement.
    pub initial_margin: Decimal,
    /// Maintenance margin requirement.
    pub maintenance_margin: Decimal,
    /// Equity remaining above initial margin.
    pub excess_liquidity: Decimal,
    /// True when equity is below maintenance requirement.
    pub margin_call: bool,
    /// Net exposure before FX conversion, by currency.
    pub exposure_by_currency: BTreeMap<Currency, Decimal>,
}

/// Values cash and every position under explicit FX and margin policies.
pub fn value_margin_account(
    cash_by_currency: &BTreeMap<Currency, Decimal>,
    positions: &[MarginPosition],
    fx: &FxBook,
    policy: &MarginPolicy,
    as_of_epoch_seconds: i64,
) -> Result<MarginSnapshot, AccountingError> {
    if policy.maximum_fx_age_seconds < 0 || policy.rates.is_empty() {
        return Err(AccountingError("invalid margin policy".to_owned()));
    }
    let ten_thousand = Decimal::from_integer(10_000)?;
    let mut cash_value = Decimal::ZERO;
    let mut position_market_value = Decimal::ZERO;
    let mut initial_margin = Decimal::ZERO;
    let mut maintenance_margin = Decimal::ZERO;
    let mut exposure_by_currency = cash_by_currency.clone();

    for (currency, cash) in cash_by_currency {
        cash_value = cash_value.checked_add(fx.convert(
            *cash,
            currency,
            &policy.base_currency,
            as_of_epoch_seconds,
            policy.maximum_fx_age_seconds,
        )?)?;
    }

    for position in positions {
        validate_canonical_id("instrument_id", &position.instrument_id)
            .map_err(|error| AccountingError(error.0))?;
        if position.quantity == Decimal::ZERO
            || position.mark_price <= Decimal::ZERO
            || position.multiplier <= Decimal::ZERO
        {
            return Err(AccountingError("invalid margin position".to_owned()));
        }
        let rate = policy.rates.get(&position.asset_class).ok_or_else(|| {
            AccountingError(format!(
                "missing margin policy for {}",
                position.asset_class
            ))
        })?;
        if rate.initial_bps > 10_000
            || rate.maintenance_bps > rate.initial_bps
            || rate.maintenance_bps == 0
        {
            return Err(AccountingError(format!(
                "invalid margin rates for {}",
                position.asset_class
            )));
        }
        let local_value = position
            .quantity
            .checked_mul(position.mark_price)?
            .checked_mul(position.multiplier)?;
        let local_exposure = exposure_by_currency
            .get(&position.currency)
            .copied()
            .unwrap_or(Decimal::ZERO)
            .checked_add(local_value)?;
        exposure_by_currency.insert(position.currency.clone(), local_exposure);
        let base_value = fx.convert(
            local_value,
            &position.currency,
            &policy.base_currency,
            as_of_epoch_seconds,
            policy.maximum_fx_age_seconds,
        )?;
        position_market_value = position_market_value.checked_add(base_value)?;
        let absolute_value = absolute(base_value)?;
        initial_margin = initial_margin.checked_add(
            absolute_value
                .checked_mul(Decimal::from_integer(i64::from(rate.initial_bps))?)?
                .checked_div(ten_thousand)?,
        )?;
        maintenance_margin = maintenance_margin.checked_add(
            absolute_value
                .checked_mul(Decimal::from_integer(i64::from(rate.maintenance_bps))?)?
                .checked_div(ten_thousand)?,
        )?;
    }

    let net_liquidation_value = cash_value.checked_add(position_market_value)?;
    let excess_liquidity = net_liquidation_value.checked_sub(initial_margin)?;
    Ok(MarginSnapshot {
        base_currency: policy.base_currency.clone(),
        cash_value,
        position_market_value,
        net_liquidation_value,
        initial_margin,
        maintenance_margin,
        excess_liquidity,
        margin_call: net_liquidation_value < maintenance_margin,
        exposure_by_currency,
    })
}

fn absolute(value: Decimal) -> Result<Decimal, AccountingError> {
    if value < Decimal::ZERO {
        Ok(Decimal::ZERO.checked_sub(value)?)
    } else {
        Ok(value)
    }
}

/// Deterministic tax-lot selection policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaxLotSelection {
    /// Oldest acquisition first.
    Fifo,
    /// Newest acquisition first.
    Lifo,
    /// Highest exact unit cost first, then oldest lot identity.
    HighestCost,
}

/// One remaining long tax lot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxLot {
    /// Immutable lot identity.
    pub lot_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Settlement currency.
    pub currency: Currency,
    /// Canonical UTC acquisition time.
    pub opened_at: String,
    /// Remaining positive quantity.
    pub remaining_quantity: Decimal,
    /// Exact all-in unit cost.
    pub unit_cost: Decimal,
}

/// Exact realized disposal result with auditable lot allocations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxLotDisposal {
    /// Idempotent disposal identity.
    pub disposal_id: String,
    /// Instrument disposed.
    pub instrument_id: String,
    /// Settlement currency.
    pub currency: Currency,
    /// Total disposed quantity.
    pub quantity: Decimal,
    /// Gross proceeds before the disposal fee.
    pub gross_proceeds: Decimal,
    /// Exact selected cost basis.
    pub cost_basis: Decimal,
    /// Exact fee applied once to the complete disposal.
    pub fee: Decimal,
    /// Net realized P&L.
    pub realized_pnl: Decimal,
    /// `(lot_id, quantity)` allocations in deterministic selection order.
    pub allocations: Vec<(String, Decimal)>,
}

/// Idempotent exact long-lot accounting projection.
#[derive(Clone, Debug, Default)]
pub struct TaxLotBook {
    lots: BTreeMap<String, Vec<TaxLot>>,
    applied_lot_ids: BTreeSet<String>,
    applied_disposal_ids: BTreeSet<String>,
    realized_by_currency: BTreeMap<Currency, Decimal>,
}

impl TaxLotBook {
    /// Adds a validated acquisition lot exactly once.
    pub fn acquire(&mut self, lot: TaxLot) -> Result<bool, AccountingError> {
        validate_canonical_id("tax lot_id", &lot.lot_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_canonical_id("tax lot instrument_id", &lot.instrument_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_utc_timestamp("tax lot opened_at", &lot.opened_at)
            .map_err(|error| AccountingError(error.0))?;
        if lot.remaining_quantity <= Decimal::ZERO || lot.unit_cost <= Decimal::ZERO {
            return Err(AccountingError("invalid tax lot economics".to_owned()));
        }
        if self.applied_lot_ids.contains(&lot.lot_id) {
            return Ok(false);
        }
        self.applied_lot_ids.insert(lot.lot_id.clone());
        let instrument_lots = self.lots.entry(lot.instrument_id.clone()).or_default();
        instrument_lots.push(lot);
        instrument_lots.sort_by(|left, right| {
            left.opened_at
                .cmp(&right.opened_at)
                .then_with(|| left.lot_id.cmp(&right.lot_id))
        });
        Ok(true)
    }

    /// Disposes long inventory under an explicit selection policy.
    #[allow(clippy::too_many_arguments)]
    pub fn dispose(
        &mut self,
        disposal_id: &str,
        instrument_id: &str,
        currency: &Currency,
        quantity: Decimal,
        unit_proceeds: Decimal,
        fee: Decimal,
        occurred_at: &str,
        selection: TaxLotSelection,
    ) -> Result<Option<TaxLotDisposal>, AccountingError> {
        validate_canonical_id("tax disposal_id", disposal_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_canonical_id("tax disposal instrument_id", instrument_id)
            .map_err(|error| AccountingError(error.0))?;
        validate_utc_timestamp("tax disposal occurred_at", occurred_at)
            .map_err(|error| AccountingError(error.0))?;
        if quantity <= Decimal::ZERO || unit_proceeds <= Decimal::ZERO || fee < Decimal::ZERO {
            return Err(AccountingError("invalid tax disposal economics".to_owned()));
        }
        if self.applied_disposal_ids.contains(disposal_id) {
            return Ok(None);
        }
        let existing = self
            .lots
            .get(instrument_id)
            .ok_or_else(|| AccountingError("no tax lots for disposal".to_owned()))?;
        if existing.iter().any(|lot| lot.currency != *currency) {
            return Err(AccountingError(
                "tax lots for one instrument must use the disposal currency".to_owned(),
            ));
        }
        let available = existing.iter().try_fold(Decimal::ZERO, |total, lot| {
            total
                .checked_add(lot.remaining_quantity)
                .map_err(AccountingError::from)
        })?;
        if quantity > available {
            return Err(AccountingError(
                "tax disposal exceeds available long lots".to_owned(),
            ));
        }
        let mut order: Vec<usize> = (0..existing.len()).collect();
        match selection {
            TaxLotSelection::Fifo => {}
            TaxLotSelection::Lifo => order.reverse(),
            TaxLotSelection::HighestCost => order.sort_by(|left, right| {
                existing[*right]
                    .unit_cost
                    .cmp(&existing[*left].unit_cost)
                    .then_with(|| existing[*left].lot_id.cmp(&existing[*right].lot_id))
            }),
        }
        let mut projected = existing.clone();
        let mut remaining = quantity;
        let mut cost_basis = Decimal::ZERO;
        let mut allocations = Vec::new();
        for index in order {
            if remaining == Decimal::ZERO {
                break;
            }
            let selected = projected[index].remaining_quantity.min(remaining);
            if selected == Decimal::ZERO {
                continue;
            }
            cost_basis =
                cost_basis.checked_add(selected.checked_mul(projected[index].unit_cost)?)?;
            projected[index].remaining_quantity =
                projected[index].remaining_quantity.checked_sub(selected)?;
            remaining = remaining.checked_sub(selected)?;
            allocations.push((projected[index].lot_id.clone(), selected));
        }
        if remaining != Decimal::ZERO {
            return Err(AccountingError(
                "tax lot selection failed to conserve quantity".to_owned(),
            ));
        }
        let gross_proceeds = quantity.checked_mul(unit_proceeds)?;
        let realized_pnl = gross_proceeds.checked_sub(cost_basis)?.checked_sub(fee)?;
        projected.retain(|lot| lot.remaining_quantity > Decimal::ZERO);
        self.lots.insert(instrument_id.to_owned(), projected);
        self.applied_disposal_ids.insert(disposal_id.to_owned());
        let prior = self
            .realized_by_currency
            .get(currency)
            .copied()
            .unwrap_or(Decimal::ZERO);
        self.realized_by_currency
            .insert(currency.clone(), prior.checked_add(realized_pnl)?);
        Ok(Some(TaxLotDisposal {
            disposal_id: disposal_id.to_owned(),
            instrument_id: instrument_id.to_owned(),
            currency: currency.clone(),
            quantity,
            gross_proceeds,
            cost_basis,
            fee,
            realized_pnl,
            allocations,
        }))
    }

    /// Stable remaining lots for one instrument.
    pub fn lots(&self, instrument_id: &str) -> &[TaxLot] {
        self.lots.get(instrument_id).map_or(&[], Vec::as_slice)
    }

    /// Cumulative realized P&L for a currency.
    pub fn realized(&self, currency: &Currency) -> Decimal {
        self.realized_by_currency
            .get(currency)
            .copied()
            .unwrap_or(Decimal::ZERO)
    }
}

/// Financing exposure family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinancingKind {
    /// Debit cash or margin loan.
    CashDebit,
    /// Marked short-borrow exposure.
    ShortBorrow,
}

/// One exact financing balance and annualized rate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinancingBalance {
    /// Canonical balance or instrument identity.
    pub reference_id: String,
    /// Financing family.
    pub kind: FinancingKind,
    /// Accrual currency.
    pub currency: Currency,
    /// Positive principal or marked borrow value.
    pub principal: Decimal,
    /// Non-negative annual rate in basis points.
    pub annual_rate_bps: u32,
}

/// Exact financing accrual grouped by source and currency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinancingAccrual {
    /// Accrual interval in calendar days.
    pub days: u32,
    /// Day-count denominator, normally 360 or 365.
    pub day_count_basis: u32,
    /// Charge by source reference.
    pub charges_by_reference: BTreeMap<String, Decimal>,
    /// Total charge by currency.
    pub charges_by_currency: BTreeMap<Currency, Decimal>,
}

/// Accrues explicit cash-debit and short-borrow financing without a wall clock.
pub fn accrue_financing(
    balances: &[FinancingBalance],
    days: u32,
    day_count_basis: u32,
) -> Result<FinancingAccrual, AccountingError> {
    if days == 0 || !matches!(day_count_basis, 360 | 365) || balances.len() > 1_000_000 {
        return Err(AccountingError("invalid financing interval".to_owned()));
    }
    let denominator = Decimal::from_integer(i64::from(day_count_basis))?
        .checked_mul(Decimal::from_integer(10_000)?)?;
    let day_count = Decimal::from_integer(i64::from(days))?;
    let mut references = BTreeSet::new();
    let mut charges_by_reference = BTreeMap::new();
    let mut charges_by_currency: BTreeMap<Currency, Decimal> = BTreeMap::new();
    for balance in balances {
        validate_canonical_id("financing reference_id", &balance.reference_id)
            .map_err(|error| AccountingError(error.0))?;
        if !references.insert(balance.reference_id.as_str())
            || balance.principal <= Decimal::ZERO
            || balance.annual_rate_bps > 1_000_000
        {
            return Err(AccountingError("invalid financing balance".to_owned()));
        }
        let charge = balance
            .principal
            .checked_mul(Decimal::from_integer(i64::from(balance.annual_rate_bps))?)?
            .checked_mul(day_count)?
            .checked_div(denominator)?;
        charges_by_reference.insert(balance.reference_id.clone(), charge);
        let prior = charges_by_currency
            .get(&balance.currency)
            .copied()
            .unwrap_or(Decimal::ZERO);
        charges_by_currency.insert(balance.currency.clone(), prior.checked_add(charge)?);
    }
    Ok(FinancingAccrual {
        days,
        day_count_basis,
        charges_by_reference,
        charges_by_currency,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn currency(value: &str) -> Currency {
        Currency::new(value).expect("currency")
    }

    #[test]
    fn journal_balances_each_currency_and_is_idempotent() {
        let usd = currency("USD");
        let eur = currency("EUR");
        let transaction = JournalTransaction {
            transaction_id: "journal.trade-1".to_owned(),
            tenant_id: "tenant.acme".to_owned(),
            reference_id: "fill.trade-1".to_owned(),
            lines: vec![
                JournalLine {
                    account_id: "asset.cash".to_owned(),
                    currency: usd.clone(),
                    debit: amount("100"),
                    credit: Decimal::ZERO,
                },
                JournalLine {
                    account_id: "liability.payable".to_owned(),
                    currency: usd.clone(),
                    debit: Decimal::ZERO,
                    credit: amount("100"),
                },
                JournalLine {
                    account_id: "asset.cash".to_owned(),
                    currency: eur.clone(),
                    debit: amount("90"),
                    credit: Decimal::ZERO,
                },
                JournalLine {
                    account_id: "equity.capital".to_owned(),
                    currency: eur,
                    debit: Decimal::ZERO,
                    credit: amount("90"),
                },
            ],
        };
        let mut ledger = MultiCurrencyLedger::default();
        assert!(ledger.post(&transaction).unwrap());
        assert!(!ledger.post(&transaction).unwrap());
        assert_eq!(ledger.balance("asset.cash", &usd), amount("100"));
    }

    #[test]
    fn rejects_transaction_unbalanced_in_one_currency() {
        let transaction = JournalTransaction {
            transaction_id: "journal.bad".to_owned(),
            tenant_id: "tenant.acme".to_owned(),
            reference_id: "transfer.bad".to_owned(),
            lines: vec![
                JournalLine {
                    account_id: "asset.cash".to_owned(),
                    currency: currency("USD"),
                    debit: amount("100"),
                    credit: Decimal::ZERO,
                },
                JournalLine {
                    account_id: "equity.capital".to_owned(),
                    currency: currency("USD"),
                    debit: Decimal::ZERO,
                    credit: amount("99"),
                },
            ],
        };
        assert!(MultiCurrencyLedger::default().post(&transaction).is_err());
    }

    #[test]
    fn values_multi_currency_long_and_short_margin_exactly() {
        let usd = currency("USD");
        let eur = currency("EUR");
        let mut fx = FxBook::default();
        fx.upsert(FxQuote {
            base: eur.clone(),
            quote: usd.clone(),
            quote_rate: amount("1.10"),
            observed_at_epoch_seconds: 1_000,
        })
        .unwrap();
        let cash = BTreeMap::from([
            (usd.clone(), amount("10000")),
            (eur.clone(), amount("1000")),
        ]);
        let positions = vec![
            MarginPosition {
                instrument_id: "equity.us".to_owned(),
                asset_class: "equity".to_owned(),
                currency: usd.clone(),
                quantity: amount("10"),
                mark_price: amount("100"),
                multiplier: amount("1"),
            },
            MarginPosition {
                instrument_id: "equity.eu".to_owned(),
                asset_class: "equity".to_owned(),
                currency: eur,
                quantity: amount("-5"),
                mark_price: amount("100"),
                multiplier: amount("1"),
            },
        ];
        let policy = MarginPolicy {
            base_currency: usd,
            maximum_fx_age_seconds: 30,
            rates: BTreeMap::from([(
                "equity".to_owned(),
                MarginRate {
                    initial_bps: 5_000,
                    maintenance_bps: 2_500,
                },
            )]),
        };
        let snapshot = value_margin_account(&cash, &positions, &fx, &policy, 1_010).unwrap();
        assert_eq!(snapshot.cash_value, amount("11100"));
        assert_eq!(snapshot.position_market_value, amount("450"));
        assert_eq!(snapshot.net_liquidation_value, amount("11550"));
        assert_eq!(snapshot.initial_margin, amount("775"));
        assert_eq!(snapshot.maintenance_margin, amount("387.5"));
        assert!(!snapshot.margin_call);
    }

    #[test]
    fn refuses_missing_or_stale_fx() {
        let usd = currency("USD");
        let eur = currency("EUR");
        let cash = BTreeMap::from([(eur.clone(), amount("1"))]);
        let policy = MarginPolicy {
            base_currency: usd,
            maximum_fx_age_seconds: 10,
            rates: BTreeMap::from([(
                "equity".to_owned(),
                MarginRate {
                    initial_bps: 5_000,
                    maintenance_bps: 2_500,
                },
            )]),
        };
        assert!(value_margin_account(&cash, &[], &FxBook::default(), &policy, 20).is_err());
        let mut fx = FxBook::default();
        fx.upsert(FxQuote {
            base: eur,
            quote: policy.base_currency.clone(),
            quote_rate: amount("1.1"),
            observed_at_epoch_seconds: 1,
        })
        .unwrap();
        assert!(value_margin_account(&cash, &[], &fx, &policy, 20).is_err());
    }

    #[test]
    fn aggregates_multi_account_cash_and_positions_deterministically() {
        let usd = currency("USD");
        let eur = currency("EUR");
        let first = AccountPortfolioSnapshot {
            account_id: "acct.paper.001".to_owned(),
            as_of: "2026-01-02T21:00:00Z".to_owned(),
            reconciliation_id: "reconciliation.paper.001".to_owned(),
            configuration_fingerprint: "a".repeat(64),
            cash_by_currency: BTreeMap::from([
                (usd.clone(), amount("1000")),
                (eur.clone(), amount("200")),
            ]),
            positions: vec![MarginPosition {
                instrument_id: "inst.us_equity.spy".to_owned(),
                asset_class: "equity".to_owned(),
                currency: usd.clone(),
                quantity: amount("2"),
                mark_price: amount("10"),
                multiplier: amount("1"),
            }],
        };
        let second = AccountPortfolioSnapshot {
            account_id: "acct.paper.002".to_owned(),
            as_of: "2026-01-02T21:00:00Z".to_owned(),
            reconciliation_id: "reconciliation.paper.002".to_owned(),
            configuration_fingerprint: "a".repeat(64),
            cash_by_currency: BTreeMap::from([
                (usd.clone(), amount("-50")),
                (eur.clone(), amount("100")),
            ]),
            positions: vec![
                MarginPosition {
                    instrument_id: "inst.option.spy.call".to_owned(),
                    asset_class: "option".to_owned(),
                    currency: usd.clone(),
                    quantity: amount("3"),
                    mark_price: amount("5"),
                    multiplier: amount("100"),
                },
                MarginPosition {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    asset_class: "equity".to_owned(),
                    currency: usd.clone(),
                    quantity: amount("-1"),
                    mark_price: amount("12"),
                    multiplier: amount("1"),
                },
            ],
        };

        let forward = aggregate_account_portfolios(&[first.clone(), second.clone()]).unwrap();
        let replay = aggregate_account_portfolios(&[second, first]).unwrap();
        assert_eq!(forward, replay);
        assert_eq!(forward.account_count, 2);
        assert_eq!(forward.as_of, "2026-01-02T21:00:00Z");
        assert_eq!(forward.reconciliation_ids_by_account.len(), 2);
        assert_eq!(forward.cash_by_currency[&usd], amount("950"));
        assert_eq!(forward.cash_by_currency[&eur], amount("300"));
        assert_eq!(forward.positions.len(), 2);
        let equity = forward
            .positions
            .iter()
            .find(|position| position.instrument_id == "inst.us_equity.spy")
            .unwrap();
        assert_eq!(equity.quantity, amount("1"));
        assert_eq!(equity.market_value, amount("8"));
        assert_eq!(
            equity.contributing_account_ids,
            vec!["acct.paper.001".to_owned(), "acct.paper.002".to_owned()]
        );
        let option = forward
            .positions
            .iter()
            .find(|position| position.instrument_id == "inst.option.spy.call")
            .unwrap();
        assert_eq!(option.market_value, amount("1500"));
    }

    #[test]
    fn refuses_duplicate_account_or_position_identity_in_aggregation() {
        let account = AccountPortfolioSnapshot {
            account_id: "acct.paper.001".to_owned(),
            as_of: "2026-01-02T21:00:00Z".to_owned(),
            reconciliation_id: "reconciliation.paper.001".to_owned(),
            configuration_fingerprint: "a".repeat(64),
            cash_by_currency: BTreeMap::new(),
            positions: vec![MarginPosition {
                instrument_id: "inst.us_equity.spy".to_owned(),
                asset_class: "equity".to_owned(),
                currency: currency("USD"),
                quantity: amount("1"),
                mark_price: amount("100"),
                multiplier: amount("1"),
            }],
        };
        assert!(aggregate_account_portfolios(&[account.clone(), account.clone()]).is_err());
        let mut multiplier_mismatch = account.clone();
        multiplier_mismatch.account_id = "acct.paper.002".to_owned();
        multiplier_mismatch.positions[0].multiplier = amount("100");
        assert!(aggregate_account_portfolios(&[account.clone(), multiplier_mismatch]).is_err());
        let mut stale_snapshot = account.clone();
        stale_snapshot.account_id = "acct.paper.003".to_owned();
        stale_snapshot.reconciliation_id = "reconciliation.paper.003".to_owned();
        stale_snapshot.as_of = "2026-01-02T21:00:01Z".to_owned();
        assert!(aggregate_account_portfolios(&[account.clone(), stale_snapshot]).is_err());
        let mut duplicate_position = account;
        duplicate_position
            .positions
            .push(duplicate_position.positions[0].clone());
        assert!(aggregate_account_portfolios(&[duplicate_position]).is_err());
    }

    #[test]
    fn tax_lots_apply_fifo_lifo_and_idempotent_disposals_exactly() {
        let usd = currency("USD");
        let mut book = TaxLotBook::default();
        for (id, opened, quantity, cost) in [
            ("lot.one", "2026-01-01T00:00:00Z", "2", "100"),
            ("lot.two", "2026-01-02T00:00:00Z", "3", "110"),
        ] {
            assert!(book
                .acquire(TaxLot {
                    lot_id: id.to_owned(),
                    instrument_id: "instrument.spy".to_owned(),
                    currency: usd.clone(),
                    opened_at: opened.to_owned(),
                    remaining_quantity: amount(quantity),
                    unit_cost: amount(cost),
                })
                .unwrap());
        }
        let disposal = book
            .dispose(
                "disposal.one",
                "instrument.spy",
                &usd,
                amount("4"),
                amount("120"),
                amount("2"),
                "2026-02-01T00:00:00Z",
                TaxLotSelection::Fifo,
            )
            .unwrap()
            .expect("new disposal");
        assert_eq!(disposal.cost_basis, amount("420"));
        assert_eq!(disposal.realized_pnl, amount("58"));
        assert_eq!(
            book.lots("instrument.spy")[0].remaining_quantity,
            amount("1")
        );
        assert!(book
            .dispose(
                "disposal.one",
                "instrument.spy",
                &usd,
                amount("4"),
                amount("120"),
                amount("2"),
                "2026-02-01T00:00:00Z",
                TaxLotSelection::Lifo,
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn financing_accrual_is_exact_by_source_and_currency() {
        let usd = currency("USD");
        let accrual = accrue_financing(
            &[
                FinancingBalance {
                    reference_id: "cash.margin".to_owned(),
                    kind: FinancingKind::CashDebit,
                    currency: usd.clone(),
                    principal: amount("10000"),
                    annual_rate_bps: 900,
                },
                FinancingBalance {
                    reference_id: "borrow.spy".to_owned(),
                    kind: FinancingKind::ShortBorrow,
                    currency: usd.clone(),
                    principal: amount("5000"),
                    annual_rate_bps: 360,
                },
            ],
            10,
            360,
        )
        .expect("accrual");
        assert_eq!(accrual.charges_by_reference["cash.margin"], amount("25"));
        assert_eq!(accrual.charges_by_reference["borrow.spy"], amount("5"));
        assert_eq!(accrual.charges_by_currency[&usd], amount("30"));
    }
}

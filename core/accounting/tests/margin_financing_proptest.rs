//! Property-based coverage for `value_margin_account` and `accrue_financing`,
//! the last candidate named in E3.2 of docs/06-delivery/16-delivery-state.md.
//!
//! Both functions are held to an independent integer oracle in units of
//! 1e-8. Each generated book is built so the only rounding is at the points
//! the fixed-point contract documents: an FX conversion, a basis-point
//! requirement, and a day-count accrual, each truncated toward zero.
//! Quantities and multipliers are whole numbers and marks carry full 1e-8
//! precision, so every marked value is exact.
//!
//! Margin properties:
//! - every snapshot field matches the oracle, `maintenance <= initial`, and
//!   `margin_call` holds exactly when equity is below maintenance;
//! - a direct EUR->USD quote and the equivalent inverse USD->EUR quote value a
//!   book identically;
//! - negating every position negates its market value and leaves both margin
//!   requirements unchanged;
//! - position order does not matter, and requirements are additive over any
//!   split of the positions;
//! - raising a class's initial rate never lowers the initial requirement;
//! - valuation succeeds exactly when every needed FX conversion is fresh;
//! - a missing class, an invalid rate in use, or an invalid position fails.
//!
//! Financing properties:
//! - every charge matches the oracle and the currency totals sum the charges;
//! - balance order does not matter;
//! - splitting an interval loses at most one 1e-8 unit per balance and never
//!   charges more than accruing it whole;
//! - more days, a higher rate, a larger principal, or a 360-day basis never
//!   charges less;
//! - every documented invalid input is refused.

use std::collections::BTreeMap;

use follon_accounting::{
    accrue_financing, value_margin_account, Currency, FinancingBalance, FinancingKind, FxBook,
    FxQuote, MarginPolicy, MarginPosition, MarginRate,
};
use follon_domain::{Decimal, DECIMAL_SCALE};
use proptest::prelude::*;

const CLASSES: [&str; 3] = ["equity", "future", "option"];
/// EUR->USD rates as `numerator / denominator`, each chosen so the rate and
/// its inverse are both exact at 1e-8.
const RATES: [(i128, i128); 5] = [(5, 4), (2, 1), (1, 2), (4, 1), (4, 5)];
const QUOTE_TIME: i64 = 1_000;
/// Whole units of the largest cash balance.
const CASH_UNITS: i128 = 1_000_000;

fn usd() -> Currency {
    Currency::new("USD").unwrap()
}

fn eur() -> Currency {
    Currency::new("EUR").unwrap()
}

fn whole(units: i128) -> Decimal {
    Decimal::from_scaled(units * DECIMAL_SCALE)
}

#[derive(Clone, Debug)]
struct Position {
    class: usize,
    euro: bool,
    /// Signed whole units, never zero.
    quantity: i128,
    /// Mark in 1e-8 units, positive.
    mark: i128,
    /// Whole-number multiplier, positive.
    multiplier: i128,
}

#[derive(Clone, Debug)]
struct Book {
    usd_cash: Option<i128>,
    eur_cash: Option<i128>,
    positions: Vec<Position>,
    /// `(initial_bps, maintenance_bps)` for each of `CLASSES`.
    rates: [(u32, u32); 3],
    rate: usize,
    inverse_quote: bool,
    max_age: i64,
    /// Seconds between the quote and the valuation time.
    age: i64,
}

impl Book {
    fn needs_fx(&self) -> bool {
        self.eur_cash.is_some() || self.positions.iter().any(|position| position.euro)
    }

    fn cash(&self) -> BTreeMap<Currency, Decimal> {
        let mut cash = BTreeMap::new();
        if let Some(amount) = self.usd_cash {
            cash.insert(usd(), Decimal::from_scaled(amount));
        }
        if let Some(amount) = self.eur_cash {
            cash.insert(eur(), Decimal::from_scaled(amount));
        }
        cash
    }

    fn margin_positions(&self) -> Vec<MarginPosition> {
        self.positions
            .iter()
            .enumerate()
            .map(|(index, position)| MarginPosition {
                instrument_id: format!("inst.p{index}"),
                asset_class: CLASSES[position.class].to_owned(),
                currency: if position.euro { eur() } else { usd() },
                quantity: whole(position.quantity),
                mark_price: Decimal::from_scaled(position.mark),
                multiplier: whole(position.multiplier),
            })
            .collect()
    }

    fn fx(&self) -> FxBook {
        let (numerator, denominator) = RATES[self.rate];
        let (base, quote, rate) = if self.inverse_quote {
            (usd(), eur(), denominator * DECIMAL_SCALE / numerator)
        } else {
            (eur(), usd(), numerator * DECIMAL_SCALE / denominator)
        };
        let mut book = FxBook::default();
        book.upsert(FxQuote {
            base,
            quote,
            quote_rate: Decimal::from_scaled(rate),
            observed_at_epoch_seconds: QUOTE_TIME,
        })
        .unwrap();
        book
    }

    fn policy(&self) -> MarginPolicy {
        MarginPolicy {
            base_currency: usd(),
            maximum_fx_age_seconds: self.max_age,
            rates: CLASSES
                .iter()
                .zip(self.rates)
                .map(|(class, (initial_bps, maintenance_bps))| {
                    (
                        (*class).to_owned(),
                        MarginRate {
                            initial_bps,
                            maintenance_bps,
                        },
                    )
                })
                .collect(),
        }
    }

    fn as_of(&self) -> i64 {
        QUOTE_TIME + self.age
    }

    fn value(
        &self,
    ) -> Result<follon_accounting::MarginSnapshot, follon_accounting::AccountingError> {
        value_margin_account(
            &self.cash(),
            &self.margin_positions(),
            &self.fx(),
            &self.policy(),
            self.as_of(),
        )
    }
}

/// The oracle's snapshot, every amount in 1e-8 units.
#[derive(Debug, PartialEq)]
struct Expected {
    cash_value: i128,
    position_market_value: i128,
    initial_margin: i128,
    maintenance_margin: i128,
    exposure_usd: Option<i128>,
    exposure_eur: Option<i128>,
}

/// Converts EUR to USD, truncated toward zero, which is what Rust's integer
/// division does.
fn to_usd(amount: i128, rate: usize) -> i128 {
    let (numerator, denominator) = RATES[rate];
    amount * numerator / denominator
}

fn oracle(book: &Book) -> Expected {
    let mut expected = Expected {
        cash_value: 0,
        position_market_value: 0,
        initial_margin: 0,
        maintenance_margin: 0,
        exposure_usd: book.usd_cash,
        exposure_eur: book.eur_cash,
    };
    expected.cash_value += book.usd_cash.unwrap_or(0);
    expected.cash_value += to_usd(book.eur_cash.unwrap_or(0), book.rate);
    for position in &book.positions {
        let local = position.quantity * position.mark * position.multiplier;
        let exposure = if position.euro {
            &mut expected.exposure_eur
        } else {
            &mut expected.exposure_usd
        };
        *exposure = Some(exposure.unwrap_or(0) + local);
        let base = if position.euro {
            to_usd(local, book.rate)
        } else {
            local
        };
        expected.position_market_value += base;
        let (initial_bps, maintenance_bps) = book.rates[position.class];
        expected.initial_margin += base.abs() * i128::from(initial_bps) / 10_000;
        expected.maintenance_margin += base.abs() * i128::from(maintenance_bps) / 10_000;
    }
    expected
}

fn observed(snapshot: &follon_accounting::MarginSnapshot) -> Expected {
    Expected {
        cash_value: snapshot.cash_value.scaled(),
        position_market_value: snapshot.position_market_value.scaled(),
        initial_margin: snapshot.initial_margin.scaled(),
        maintenance_margin: snapshot.maintenance_margin.scaled(),
        exposure_usd: snapshot
            .exposure_by_currency
            .get(&usd())
            .map(|value| value.scaled()),
        exposure_eur: snapshot
            .exposure_by_currency
            .get(&eur())
            .map(|value| value.scaled()),
    }
}

fn position() -> impl Strategy<Value = Position> {
    (
        0usize..3,
        any::<bool>(),
        (-50i128..=50).prop_filter("nonzero quantity", |quantity| *quantity != 0),
        1i128..=10_000 * DECIMAL_SCALE,
        prop::sample::select(vec![1i128, 10, 50, 100]),
    )
        .prop_map(|(class, euro, quantity, mark, multiplier)| Position {
            class,
            euro,
            quantity,
            mark,
            multiplier,
        })
}

fn rate() -> impl Strategy<Value = (u32, u32)> {
    (1u32..=10_000).prop_flat_map(|initial| (Just(initial), 1u32..=initial))
}

fn cash() -> impl Strategy<Value = Option<i128>> {
    prop::option::of(-CASH_UNITS * DECIMAL_SCALE..=CASH_UNITS * DECIMAL_SCALE)
}

/// A valid book whose FX quote is always fresh.
fn book() -> impl Strategy<Value = Book> {
    (
        cash(),
        cash(),
        prop::collection::vec(position(), 0..7),
        [rate(), rate(), rate()],
        0usize..RATES.len(),
        any::<bool>(),
        0i64..=60,
    )
        .prop_flat_map(
            |(usd_cash, eur_cash, positions, rates, rate, inverse_quote, max_age)| {
                (0i64..=max_age).prop_map(move |age| Book {
                    usd_cash,
                    eur_cash,
                    positions: positions.clone(),
                    rates,
                    rate,
                    inverse_quote,
                    max_age,
                    age,
                })
            },
        )
}

#[derive(Clone, Debug)]
struct Balance {
    euro: bool,
    short_borrow: bool,
    /// Principal in 1e-8 units, positive.
    principal: i128,
    annual_rate_bps: u32,
}

fn balance() -> impl Strategy<Value = Balance> {
    (
        any::<bool>(),
        any::<bool>(),
        1i128..=CASH_UNITS * DECIMAL_SCALE,
        prop_oneof![Just(0u32), 0u32..=5_000, 0u32..=1_000_000],
    )
        .prop_map(|(euro, short_borrow, principal, annual_rate_bps)| Balance {
            euro,
            short_borrow,
            principal,
            annual_rate_bps,
        })
}

fn financing_balances(balances: &[Balance]) -> Vec<FinancingBalance> {
    balances
        .iter()
        .enumerate()
        .map(|(index, balance)| FinancingBalance {
            reference_id: format!("balance.b{index}"),
            kind: if balance.short_borrow {
                FinancingKind::ShortBorrow
            } else {
                FinancingKind::CashDebit
            },
            currency: if balance.euro { eur() } else { usd() },
            principal: Decimal::from_scaled(balance.principal),
            annual_rate_bps: balance.annual_rate_bps,
        })
        .collect()
}

/// The charge in 1e-8 units: principal x rate x days / (basis x 10,000),
/// truncated once, toward zero.
fn expected_charge(balance: &Balance, days: u32, basis: u32) -> i128 {
    balance.principal * i128::from(balance.annual_rate_bps) * i128::from(days)
        / (i128::from(basis) * 10_000)
}

fn charges(balances: &[Balance], days: u32, basis: u32) -> BTreeMap<String, i128> {
    accrue_financing(&financing_balances(balances), days, basis)
        .unwrap()
        .charges_by_reference
        .into_iter()
        .map(|(reference, charge)| (reference, charge.scaled()))
        .collect()
}

/// Half the time zero, so an invalid input sits exactly one step past its
/// limit; otherwise anywhere past it.
fn near_boundary(value: u32) -> u32 {
    if value.is_multiple_of(2) {
        0
    } else {
        value
    }
}

fn basis() -> impl Strategy<Value = u32> {
    prop::sample::select(vec![360u32, 365])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn margin_snapshot_matches_the_fixed_point_oracle(book in book()) {
        let snapshot = book.value().unwrap();
        let expected = oracle(&book);
        prop_assert_eq!(observed(&snapshot), expected);
        prop_assert_eq!(snapshot.base_currency, usd());
        prop_assert_eq!(
            snapshot.net_liquidation_value,
            snapshot.cash_value.checked_add(snapshot.position_market_value).unwrap()
        );
        prop_assert_eq!(
            snapshot.excess_liquidity,
            snapshot.net_liquidation_value.checked_sub(snapshot.initial_margin).unwrap()
        );
        prop_assert!(snapshot.maintenance_margin <= snapshot.initial_margin);
        prop_assert!(snapshot.maintenance_margin >= Decimal::ZERO);
        prop_assert_eq!(
            snapshot.margin_call,
            snapshot.net_liquidation_value < snapshot.maintenance_margin
        );
    }

    #[test]
    fn a_direct_and_an_inverse_quote_value_a_book_identically(book in book()) {
        let mut inverse = book.clone();
        inverse.inverse_quote = !book.inverse_quote;
        prop_assert_eq!(book.value().unwrap(), inverse.value().unwrap());
    }

    #[test]
    fn negating_every_position_negates_value_and_keeps_margin(book in book()) {
        let mut negated = book.clone();
        for position in &mut negated.positions {
            position.quantity = -position.quantity;
        }
        let original = book.value().unwrap();
        let flipped = negated.value().unwrap();
        prop_assert_eq!(
            flipped.position_market_value,
            Decimal::ZERO.checked_sub(original.position_market_value).unwrap()
        );
        prop_assert_eq!(flipped.initial_margin, original.initial_margin);
        prop_assert_eq!(flipped.maintenance_margin, original.maintenance_margin);
        prop_assert_eq!(flipped.cash_value, original.cash_value);
    }

    #[test]
    fn position_order_does_not_matter(book in book(), seed in any::<u64>()) {
        let mut shuffled = book.clone();
        let count = shuffled.positions.len();
        // Deterministic Fisher-Yates permutation, so a failure replays exactly.
        let mut state = seed;
        for index in (1..count).rev() {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            shuffled.positions.swap(index, (state >> 33) as usize % (index + 1));
        }
        let expected = book.value().unwrap();
        let actual = value_margin_account(
            &shuffled.cash(),
            &shuffled.margin_positions(),
            &shuffled.fx(),
            &shuffled.policy(),
            shuffled.as_of(),
        )
        .unwrap();
        // Instrument identities follow position index, so compare the values,
        // which are the only thing the permutation may not change.
        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn requirements_are_additive_over_any_split_of_the_positions(
        book in book(),
        split in prop::collection::vec(any::<bool>(), 7),
    ) {
        let mut left = book.clone();
        let mut right = book.clone();
        left.positions.clear();
        right.positions.clear();
        for (position, goes_left) in book.positions.iter().zip(&split) {
            if *goes_left {
                left.positions.push(position.clone());
            } else {
                right.positions.push(position.clone());
            }
        }
        let whole_book = book.value().unwrap();
        let left = left.value().unwrap();
        let right = right.value().unwrap();
        prop_assert_eq!(
            whole_book.initial_margin,
            left.initial_margin.checked_add(right.initial_margin).unwrap()
        );
        prop_assert_eq!(
            whole_book.maintenance_margin,
            left.maintenance_margin.checked_add(right.maintenance_margin).unwrap()
        );
        prop_assert_eq!(
            whole_book.position_market_value,
            left.position_market_value.checked_add(right.position_market_value).unwrap()
        );
    }

    #[test]
    fn raising_an_initial_rate_never_lowers_the_initial_requirement(
        book in book(),
        class in 0usize..3,
        raise in 0u32..=10_000,
    ) {
        let mut raised = book.clone();
        let (initial, maintenance) = raised.rates[class];
        raised.rates[class] = ((initial + raise).min(10_000), maintenance);
        let before = book.value().unwrap();
        let after = raised.value().unwrap();
        prop_assert!(after.initial_margin >= before.initial_margin);
        prop_assert_eq!(after.maintenance_margin, before.maintenance_margin);
        prop_assert!(after.excess_liquidity <= before.excess_liquidity);
    }

    #[test]
    fn valuation_succeeds_exactly_when_every_needed_conversion_is_fresh(
        book in book(),
        age in -30i64..=120,
    ) {
        let mut aged = book.clone();
        aged.age = age;
        let fresh = (0..=aged.max_age).contains(&age);
        let result = aged.value();
        if aged.needs_fx() && !fresh {
            prop_assert!(result.is_err(), "a stale quote must not value a book that needs it");
        } else {
            prop_assert_eq!(observed(&result.unwrap()), oracle(&aged));
        }
    }

    #[test]
    fn invalid_margin_inputs_are_refused(book in book(), case in 0usize..7, value in any::<u32>()) {
        prop_assume!(!book.positions.is_empty());
        let value = near_boundary(value);
        let mut positions = book.margin_positions();
        let mut policy = book.policy();
        let used = positions[0].asset_class.clone();
        match case {
            0 => {
                policy.rates.remove(&used);
            }
            1 => {
                let rate = policy.rates.get_mut(&used).unwrap();
                rate.initial_bps = 10_001 + value % 1_000_000;
                rate.maintenance_bps = rate.maintenance_bps.min(rate.initial_bps);
            }
            2 => {
                let rate = policy.rates.get_mut(&used).unwrap();
                rate.maintenance_bps = rate.initial_bps + 1 + value % 1_000;
            }
            3 => policy.rates.get_mut(&used).unwrap().maintenance_bps = 0,
            4 => positions[0].quantity = Decimal::ZERO,
            5 => positions[0].mark_price = Decimal::from_scaled(-i128::from(value % 1_000)),
            _ => positions[0].multiplier = Decimal::from_scaled(-i128::from(value % 1_000)),
        }
        prop_assert!(
            value_margin_account(&book.cash(), &positions, &book.fx(), &policy, book.as_of()).is_err(),
            "case {} was accepted", case
        );
    }

    #[test]
    fn financing_matches_the_day_count_oracle(
        balances in prop::collection::vec(balance(), 0..8),
        days in 1u32..=3_650,
        basis in basis(),
    ) {
        let accrual = accrue_financing(&financing_balances(&balances), days, basis).unwrap();
        prop_assert_eq!(accrual.days, days);
        prop_assert_eq!(accrual.day_count_basis, basis);
        prop_assert_eq!(accrual.charges_by_reference.len(), balances.len());
        let mut by_currency: BTreeMap<Currency, i128> = BTreeMap::new();
        for (index, balance) in balances.iter().enumerate() {
            let expected = expected_charge(balance, days, basis);
            prop_assert_eq!(
                accrual.charges_by_reference[&format!("balance.b{index}")].scaled(),
                expected
            );
            *by_currency.entry(if balance.euro { eur() } else { usd() }).or_default() += expected;
        }
        let actual: BTreeMap<Currency, i128> = accrual
            .charges_by_currency
            .iter()
            .map(|(currency, charge)| (currency.clone(), charge.scaled()))
            .collect();
        prop_assert_eq!(actual, by_currency);
    }

    #[test]
    fn financing_does_not_depend_on_balance_order(
        balances in prop::collection::vec(balance(), 0..8),
        days in 1u32..=3_650,
        basis in basis(),
    ) {
        let forward = financing_balances(&balances);
        let mut reversed = forward.clone();
        reversed.reverse();
        prop_assert_eq!(
            accrue_financing(&forward, days, basis).unwrap(),
            accrue_financing(&reversed, days, basis).unwrap()
        );
    }

    #[test]
    fn splitting_an_interval_loses_at_most_one_unit_per_balance(
        balances in prop::collection::vec(balance(), 1..8),
        first in 1u32..=1_825,
        second in 1u32..=1_825,
        basis in basis(),
    ) {
        let whole_interval = charges(&balances, first + second, basis);
        let first_part = charges(&balances, first, basis);
        let second_part = charges(&balances, second, basis);
        for (reference, whole_charge) in &whole_interval {
            let split = first_part[reference] + second_part[reference];
            prop_assert!(split <= *whole_charge, "splitting {} charged more", reference);
            prop_assert!(*whole_charge - split <= 1, "splitting {} lost more than one unit", reference);
        }
    }

    #[test]
    fn financing_is_monotonic_in_every_input(
        balance in balance(),
        days in 1u32..=3_650,
        more_days in 0u32..=365,
        more_bps in 0u32..=1_000,
        more_principal in 0i128..=DECIMAL_SCALE * 1_000,
        basis in basis(),
    ) {
        let charge = |balance: &Balance, days: u32, basis: u32| {
            charges(std::slice::from_ref(balance), days, basis)["balance.b0"]
        };
        let base = charge(&balance, days, basis);
        prop_assert!(base >= 0);
        prop_assert!(charge(&balance, days + more_days, basis) >= base);
        let mut higher = balance.clone();
        higher.annual_rate_bps = (balance.annual_rate_bps + more_bps).min(1_000_000);
        prop_assert!(charge(&higher, days, basis) >= base);
        let mut larger = balance.clone();
        larger.principal += more_principal;
        prop_assert!(charge(&larger, days, basis) >= base);
        prop_assert!(charge(&balance, days, 360) >= charge(&balance, days, 365));
        if balance.annual_rate_bps == 0 {
            prop_assert_eq!(base, 0);
        }
    }

    #[test]
    fn invalid_financing_inputs_are_refused(
        balances in prop::collection::vec(balance(), 1..8),
        case in 0usize..7,
        value in any::<u32>(),
    ) {
        let value = near_boundary(value);
        let mut financing = financing_balances(&balances);
        let mut days = 30u32;
        let mut basis = 365u32;
        match case {
            0 => days = 0,
            1 => basis = [364, 366, 0, 1, 720][value as usize % 5],
            2 => financing[0].principal = Decimal::ZERO,
            3 => financing[0].principal = Decimal::from_scaled(-1 - i128::from(value % 1_000)),
            4 => financing[0].annual_rate_bps = 1_000_001 + value % 1_000_000,
            5 => financing[0].reference_id = "Balance.Upper".to_owned(),
            _ => {
                let duplicate = financing[0].clone();
                financing.push(duplicate);
            }
        }
        prop_assert!(
            accrue_financing(&financing, days, basis).is_err(),
            "case {} was accepted", case
        );
    }
}

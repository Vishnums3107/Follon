//! Property-based coverage for `AdvancedBacktestAccount::accrue_financing`,
//! the account-level half of E3.2's margin/financing candidate (the pure
//! functions are covered in core/accounting/tests/margin_financing_proptest.rs).
//!
//! The wrapper decides which balances accrue and then debits the result. It is
//! held to an integer oracle in units of 1e-8:
//! - only a negative cash balance accrues cash-debit financing, on its absolute
//!   value, and only a short position accrues borrow, on
//!   `|quantity| x mark x multiplier` at the instrument's borrow rate;
//! - each charge is `principal x rate x days / (basis x 10,000)`, truncated
//!   toward zero, and the returned totals are grouped by currency;
//! - exactly the returned charge is debited from each currency, nothing else
//!   in the account changes, and the report's cumulative financing is the sum
//!   of every accrual;
//! - an accrual identity applies once: a repeat is refused and changes nothing;
//! - a missing cash-debit rate, a missing mark, or a non-positive mark for a
//!   short is refused and leaves the account unchanged.

use std::collections::BTreeMap;

use follon_accounting::{Currency, FxBook, FxQuote, MarginPolicy, MarginRate};
use follon_backtest::{AdvancedBacktestAccount, AdvancedInstrumentTerms, BacktestExecutionCharges};
use follon_domain::{Decimal, Fill, Side, DECIMAL_SCALE};
use proptest::prelude::*;

const UNITS: i128 = 1_000_000;

fn usd() -> Currency {
    Currency::new("USD").unwrap()
}

fn eur() -> Currency {
    Currency::new("EUR").unwrap()
}

fn currency(euro: bool) -> Currency {
    if euro {
        eur()
    } else {
        usd()
    }
}

fn whole(units: i128) -> Decimal {
    Decimal::from_scaled(units * DECIMAL_SCALE)
}

#[derive(Clone, Debug)]
struct Instrument {
    euro: bool,
    multiplier: i128,
    borrow_rate_bps: u32,
    /// Signed whole units; negative opens a short.
    quantity: i128,
    /// Fill price in 1e-8 units.
    price: i128,
    /// Accrual mark in 1e-8 units.
    mark: i128,
}

#[derive(Clone, Debug)]
struct Case {
    usd_cash: Option<i128>,
    eur_cash: Option<i128>,
    instruments: Vec<Instrument>,
    usd_debit_bps: u32,
    eur_debit_bps: u32,
    days: u32,
    basis: u32,
}

impl Case {
    fn instrument_id(index: usize) -> String {
        format!("inst.i{index}")
    }

    fn terms(instrument: &Instrument) -> AdvancedInstrumentTerms {
        AdvancedInstrumentTerms {
            currency: currency(instrument.euro),
            asset_class: "equity".to_owned(),
            multiplier: whole(instrument.multiplier),
            shortable: true,
            borrow_available: whole(100),
            borrow_rate_bps: instrument.borrow_rate_bps,
        }
    }

    /// The account after every opening fill, just before the accrual.
    fn account(&self) -> AdvancedBacktestAccount {
        let mut cash = BTreeMap::new();
        if let Some(amount) = self.usd_cash {
            cash.insert(usd(), Decimal::from_scaled(amount));
        }
        if let Some(amount) = self.eur_cash {
            cash.insert(eur(), Decimal::from_scaled(amount));
        }
        let mut account = AdvancedBacktestAccount::new(cash).unwrap();
        for (index, instrument) in self.instruments.iter().enumerate() {
            account
                .apply_fill(
                    &Fill {
                        execution_id: format!("execution.i{index}"),
                        order_id: format!("order.i{index}"),
                        instrument_id: Self::instrument_id(index),
                        side: if instrument.quantity < 0 {
                            Side::Sell
                        } else {
                            Side::Buy
                        },
                        quantity: whole(instrument.quantity.abs()),
                        price: Decimal::from_scaled(instrument.price),
                        fee: Decimal::ZERO,
                        executed_at: "2026-01-02T14:30:00Z".to_owned(),
                    },
                    &Self::terms(instrument),
                    BacktestExecutionCharges {
                        commission: Decimal::ZERO,
                        exchange: Decimal::ZERO,
                        regulatory: Decimal::ZERO,
                    },
                )
                .unwrap();
        }
        account
    }

    fn marks(&self) -> BTreeMap<String, Decimal> {
        self.instruments
            .iter()
            .enumerate()
            .map(|(index, instrument)| {
                (
                    Self::instrument_id(index),
                    Decimal::from_scaled(instrument.mark),
                )
            })
            .collect()
    }

    fn debit_rates(&self) -> BTreeMap<Currency, u32> {
        BTreeMap::from([(usd(), self.usd_debit_bps), (eur(), self.eur_debit_bps)])
    }

    /// The charges the accrual must return, from the pre-accrual cash.
    fn expected(&self, cash: &BTreeMap<Currency, Decimal>) -> BTreeMap<Currency, i128> {
        let charge = |principal: i128, bps: u32| {
            principal * i128::from(bps) * i128::from(self.days) / (i128::from(self.basis) * 10_000)
        };
        let mut expected: BTreeMap<Currency, i128> = BTreeMap::new();
        for (currency, amount) in cash {
            if amount.scaled() < 0 {
                let bps = self.debit_rates()[currency];
                *expected.entry(currency.clone()).or_default() += charge(-amount.scaled(), bps);
            }
        }
        for instrument in &self.instruments {
            if instrument.quantity < 0 {
                let principal = -instrument.quantity * instrument.mark * instrument.multiplier;
                *expected.entry(currency(instrument.euro)).or_default() +=
                    charge(principal, instrument.borrow_rate_bps);
            }
        }
        expected
    }
}

/// The account's report under EUR->USD = 2 and a fixed equity margin rate.
fn report(
    account: &AdvancedBacktestAccount,
    marks: &BTreeMap<String, Decimal>,
) -> follon_backtest::AdvancedBacktestReport {
    let mut fx = FxBook::default();
    fx.upsert(FxQuote {
        base: eur(),
        quote: usd(),
        quote_rate: whole(2),
        observed_at_epoch_seconds: 1_000,
    })
    .unwrap();
    let policy = MarginPolicy {
        base_currency: usd(),
        maximum_fx_age_seconds: 10,
        rates: BTreeMap::from([(
            "equity".to_owned(),
            MarginRate {
                initial_bps: 5_000,
                maintenance_bps: 2_500,
            },
        )]),
    };
    account.report(marks, &fx, &policy, 1_000).unwrap()
}

/// A complete fingerprint of the account: cash, positions, and every charge
/// the report carries, including cumulative financing.
fn fingerprint(account: &AdvancedBacktestAccount, marks: &BTreeMap<String, Decimal>) -> String {
    format!(
        "{:?} {}",
        account.cash_by_currency(),
        report(account, marks).canonical_json()
    )
}

/// Charges in USD at the report's EUR->USD rate of 2.
fn in_usd(charges: &BTreeMap<Currency, i128>) -> i128 {
    charges.get(&usd()).copied().unwrap_or(0) + 2 * charges.get(&eur()).copied().unwrap_or(0)
}

fn instrument() -> impl Strategy<Value = Instrument> {
    (
        any::<bool>(),
        prop::sample::select(vec![1i128, 10, 100]),
        prop_oneof![Just(0u32), 0u32..=5_000, 0u32..=1_000_000],
        (-50i128..=50).prop_filter("nonzero quantity", |quantity| *quantity != 0),
        1i128..=1_000 * DECIMAL_SCALE,
        1i128..=1_000 * DECIMAL_SCALE,
    )
        .prop_map(
            |(euro, multiplier, borrow_rate_bps, quantity, price, mark)| Instrument {
                euro,
                multiplier,
                borrow_rate_bps,
                quantity,
                price,
                mark,
            },
        )
}

fn cash() -> impl Strategy<Value = Option<i128>> {
    prop::option::of(-UNITS * DECIMAL_SCALE..=UNITS * DECIMAL_SCALE)
}

fn case() -> impl Strategy<Value = Case> {
    (
        cash(),
        cash(),
        prop::collection::vec(instrument(), 0..6),
        0u32..=5_000,
        0u32..=5_000,
        1u32..=365,
        prop::sample::select(vec![360u32, 365]),
    )
        .prop_filter("the account needs a cash balance", |case| {
            case.0.is_some() || case.1.is_some()
        })
        .prop_map(
            |(usd_cash, eur_cash, instruments, usd_debit_bps, eur_debit_bps, days, basis)| Case {
                usd_cash,
                eur_cash,
                instruments,
                usd_debit_bps,
                eur_debit_bps,
                days,
                basis,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(384))]

    #[test]
    fn accrual_debits_exactly_the_oracle_charges(case in case()) {
        let mut account = case.account();
        let before = account.cash_by_currency().clone();
        let positions = account.positions().clone();
        let expected = case.expected(&before);
        let charged = account
            .accrue_financing("financing.day-1", case.days, case.basis, &case.marks(), &case.debit_rates())
            .unwrap();
        let charged: BTreeMap<Currency, i128> = charged
            .into_iter()
            .map(|(currency, charge)| (currency, charge.scaled()))
            .collect();
        prop_assert_eq!(&charged, &expected);
        let mut currencies: Vec<&Currency> = before.keys().chain(expected.keys()).collect();
        currencies.sort();
        currencies.dedup();
        for currency in currencies {
            let prior = before.get(currency).map_or(0, |amount| amount.scaled());
            let debit = expected.get(currency).copied().unwrap_or(0);
            prop_assert_eq!(
                account.cash_by_currency().get(currency).map(|amount| amount.scaled()),
                Some(prior - debit),
                "cash in {} must fall by exactly its charge", currency.as_str()
            );
        }
        prop_assert_eq!(account.positions(), &positions);

        // A second accrual charges on the new balances and adds to, never
        // replaces, the cumulative financing the report carries.
        let second_expected = case.expected(account.cash_by_currency());
        let second = account
            .accrue_financing("financing.day-2", case.days, case.basis, &case.marks(), &case.debit_rates())
            .unwrap();
        let second: BTreeMap<Currency, i128> = second
            .into_iter()
            .map(|(currency, charge)| (currency, charge.scaled()))
            .collect();
        prop_assert_eq!(&second, &second_expected);
        prop_assert_eq!(
            report(&account, &case.marks()).financing_charges.scaled(),
            in_usd(&expected) + in_usd(&second_expected)
        );
    }

    #[test]
    fn an_accrual_identity_applies_once(case in case()) {
        let mut account = case.account();
        account
            .accrue_financing("financing.day-1", case.days, case.basis, &case.marks(), &case.debit_rates())
            .unwrap();
        let after_first = fingerprint(&account, &case.marks());
        prop_assert!(account
            .accrue_financing("financing.day-1", case.days, case.basis, &case.marks(), &case.debit_rates())
            .is_err());
        prop_assert_eq!(fingerprint(&account, &case.marks()), after_first);
    }

    #[test]
    fn a_refused_accrual_leaves_the_account_unchanged(case in case(), failure in 0usize..3, pick in any::<usize>()) {
        let mut account = case.account();
        let mut marks = case.marks();
        let mut rates = case.debit_rates();
        let debits: Vec<Currency> = account
            .cash_by_currency()
            .iter()
            .filter(|(_, amount)| amount.scaled() < 0)
            .map(|(currency, _)| currency.clone())
            .collect();
        let shorts: Vec<String> = case
            .instruments
            .iter()
            .enumerate()
            .filter(|(_, instrument)| instrument.quantity < 0)
            .map(|(index, _)| Case::instrument_id(index))
            .collect();
        match failure {
            0 => {
                prop_assume!(!debits.is_empty());
                rates.remove(&debits[pick % debits.len()]);
            }
            1 => {
                prop_assume!(!shorts.is_empty());
                marks.remove(&shorts[pick % shorts.len()]);
            }
            _ => {
                prop_assume!(!shorts.is_empty());
                let mark = if pick.is_multiple_of(2) { Decimal::ZERO } else { whole(-1) };
                marks.insert(shorts[pick % shorts.len()].clone(), mark);
            }
        }
        let before = fingerprint(&account, &case.marks());
        prop_assert!(account
            .accrue_financing("financing.day-1", case.days, case.basis, &marks, &rates)
            .is_err());
        prop_assert_eq!(fingerprint(&account, &case.marks()), before);
        // The refusal must not have consumed the identity either.
        prop_assert!(account
            .accrue_financing("financing.day-1", case.days, case.basis, &case.marks(), &case.debit_rates())
            .is_ok());
    }
}

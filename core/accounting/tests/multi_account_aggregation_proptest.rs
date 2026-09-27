//! Property-based coverage for `aggregate_account_portfolios`: a third,
//! independently designed property-test slice closing more of the
//! "multi-currency accounting" example named in
//! docs/06-delivery/14-master-plan-conformance-audit.md's Property/model/
//! fault testing row, after the OMS order-lifecycle (`core/control-plane`)
//! and options settlement (`core/options`) slices.
//!
//! `aggregate_account_portfolios`'s own doc comment states two invariants
//! this test holds it to: the result is deterministic irrespective of input
//! snapshot order, and it uses fixed-point values only, retaining native
//! currencies and source-account attribution without inventing an FX rate or
//! blended mark. This test checks conservation (nothing summed is dropped or
//! invented), order-independence, and the documented failure modes.

use std::collections::BTreeMap;

use follon_accounting::{AccountPortfolioSnapshot, Currency, MarginPosition};
use follon_domain::Decimal;
use proptest::prelude::*;

const AS_OF: &str = "2026-01-01T00:00:00Z";
const CONFIGURATION_FINGERPRINT: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CASH_CURRENCIES: [&str; 2] = ["USD", "EUR"];

struct InstrumentSpec {
    instrument_id: &'static str,
    asset_class: &'static str,
    currency: &'static str,
    multiplier: i64,
}

const INSTRUMENTS: [InstrumentSpec; 2] = [
    InstrumentSpec {
        instrument_id: "instrument.alpha",
        asset_class: "equity",
        currency: "USD",
        multiplier: 1,
    },
    InstrumentSpec {
        instrument_id: "instrument.beta",
        asset_class: "future",
        currency: "USD",
        multiplier: 50,
    },
];

/// One account's randomly generated cash-by-currency and per-instrument
/// holdings before it is turned into a real `AccountPortfolioSnapshot`.
#[derive(Clone, Debug)]
struct AccountSpec {
    cash: [Option<i64>; 2],
    positions: [Option<(i64, i64)>; 2],
}

fn account_spec_strategy() -> impl Strategy<Value = AccountSpec> {
    (
        prop::option::of(-1000_i64..=1000),
        prop::option::of(-1000_i64..=1000),
        prop::option::of((-20_i64..=20).prop_filter("nonzero quantity", |q| *q != 0)),
        prop::option::of(1_i64..=500),
        prop::option::of((-20_i64..=20).prop_filter("nonzero quantity", |q| *q != 0)),
        prop::option::of(1_i64..=500),
    )
        .prop_map(
            |(usd_cash, eur_cash, alpha_qty, alpha_mark, beta_qty, beta_mark)| AccountSpec {
                cash: [usd_cash, eur_cash],
                positions: [alpha_qty.zip(alpha_mark), beta_qty.zip(beta_mark)],
            },
        )
}

fn build_account(index: usize, spec: &AccountSpec) -> AccountPortfolioSnapshot {
    let mut cash_by_currency = BTreeMap::new();
    for (currency, cash) in CASH_CURRENCIES.iter().zip(spec.cash.iter()) {
        if let Some(amount) = cash {
            cash_by_currency.insert(
                Currency::new(*currency).unwrap(),
                Decimal::from_integer(*amount).unwrap(),
            );
        }
    }
    let mut positions = Vec::new();
    for (instrument, position) in INSTRUMENTS.iter().zip(spec.positions.iter()) {
        if let Some((quantity, mark)) = position {
            positions.push(MarginPosition {
                instrument_id: instrument.instrument_id.to_owned(),
                asset_class: instrument.asset_class.to_owned(),
                currency: Currency::new(instrument.currency).unwrap(),
                quantity: Decimal::from_integer(*quantity).unwrap(),
                mark_price: Decimal::from_integer(*mark).unwrap(),
                multiplier: Decimal::from_integer(instrument.multiplier).unwrap(),
            });
        }
    }
    AccountPortfolioSnapshot {
        account_id: format!("acct.{index}"),
        as_of: AS_OF.to_owned(),
        reconciliation_id: format!("recon.{index}"),
        configuration_fingerprint: CONFIGURATION_FINGERPRINT.to_owned(),
        cash_by_currency,
        positions,
    }
}

proptest! {
    /// Aggregated cash and every aggregated position's quantity/market value
    /// must equal the exact sum of the same field across every contributing
    /// account -- conservation, not an approximation or a dropped source.
    #[test]
    fn aggregation_conserves_cash_and_position_totals(
        specs in prop::collection::vec(account_spec_strategy(), 1..6),
    ) {
        let accounts: Vec<_> = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| build_account(index, spec))
            .collect();
        let result = follon_accounting::aggregate_account_portfolios(&accounts).unwrap();

        prop_assert_eq!(result.account_count as usize, accounts.len());

        for (slot, currency) in CASH_CURRENCIES.iter().enumerate() {
            let expected: i64 = specs.iter().filter_map(|spec| spec.cash[slot]).sum();
            let actual = result
                .cash_by_currency
                .get(&Currency::new(*currency).unwrap())
                .copied()
                .unwrap_or(Decimal::ZERO);
            prop_assert_eq!(actual, Decimal::from_integer(expected).unwrap());
        }

        for (slot, instrument) in INSTRUMENTS.iter().enumerate() {
            let contributing: Vec<(usize, i64, i64)> = specs
                .iter()
                .enumerate()
                .filter_map(|(index, spec)| spec.positions[slot].map(|(qty, mark)| (index, qty, mark)))
                .collect();
            let found = result
                .positions
                .iter()
                .find(|position| position.instrument_id == instrument.instrument_id);
            if contributing.is_empty() {
                prop_assert!(found.is_none());
                continue;
            }
            let position = found.unwrap();
            let expected_quantity: i64 = contributing.iter().map(|(_, qty, _)| qty).sum();
            let expected_market_value: i64 = contributing
                .iter()
                .map(|(_, qty, mark)| qty * mark * instrument.multiplier)
                .sum();
            prop_assert_eq!(position.quantity, Decimal::from_integer(expected_quantity).unwrap());
            prop_assert_eq!(
                position.market_value,
                Decimal::from_integer(expected_market_value).unwrap()
            );
            let mut expected_accounts: Vec<String> = contributing
                .iter()
                .map(|(index, _, _)| format!("acct.{index}"))
                .collect();
            expected_accounts.sort();
            prop_assert_eq!(&position.contributing_account_ids, &expected_accounts);
        }
    }

    /// The result must not depend on the order accounts were supplied in.
    #[test]
    fn aggregation_is_order_independent(
        specs in prop::collection::vec(account_spec_strategy(), 1..6),
        shuffle_seed in 0_u64..1000,
    ) {
        let accounts: Vec<_> = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| build_account(index, spec))
            .collect();
        let mut reordered = accounts.clone();
        // A cheap deterministic shuffle: rotate by the seed, which visits
        // every distinct rotation of a short vector without pulling in an
        // extra RNG dependency for this one test.
        if !reordered.is_empty() {
            let rotate_by = (shuffle_seed as usize) % reordered.len();
            reordered.rotate_left(rotate_by);
        }

        let first = follon_accounting::aggregate_account_portfolios(&accounts).unwrap();
        let second = follon_accounting::aggregate_account_portfolios(&reordered).unwrap();
        prop_assert_eq!(first, second);
    }
}

#[test]
fn duplicate_account_id_is_rejected() {
    let spec = AccountSpec {
        cash: [Some(100), None],
        positions: [None, None],
    };
    let mut first = build_account(0, &spec);
    let second = build_account(0, &spec);
    first.reconciliation_id = "recon.distinct".to_owned();
    let result = follon_accounting::aggregate_account_portfolios(&[first, second]);
    assert!(result.is_err());
}

#[test]
fn mismatched_as_of_is_rejected() {
    let spec = AccountSpec {
        cash: [Some(100), None],
        positions: [None, None],
    };
    let mut second = build_account(1, &spec);
    second.as_of = "2026-01-02T00:00:00Z".to_owned();
    let first = build_account(0, &spec);
    let result = follon_accounting::aggregate_account_portfolios(&[first, second]);
    assert!(result.is_err());
}

#[test]
fn empty_account_list_is_rejected() {
    let result = follon_accounting::aggregate_account_portfolios(&[]);
    assert!(result.is_err());
}

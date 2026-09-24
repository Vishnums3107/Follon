//! A short opened where the commission meets or exceeds the premium.
//!
//! Short-lot `unit_proceeds` is net of the opening fee, so it is zero or
//! negative when the fee is at least the premium -- a 1-lot, one-cent option
//! under a one-dollar minimum commission. That trade really executes at the
//! broker; the ledger must record it, restore it, and realize the right P&L on
//! cover, rather than refuse it (found by the PAPER combination property test,
//! conformance-audit item 55).

use std::str::FromStr;

use follon_accounting::{Currency, ShortTaxLot, TaxLotBook, TaxLotSelection};
use follon_domain::Decimal;

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

#[test]
fn a_short_with_zero_or_negative_net_proceeds_is_recorded_restored_and_covered() {
    for (net_proceeds, expected_realized) in [("0", "-1.10"), ("-0.35", "-1.45")] {
        let usd = Currency::new("USD").unwrap();
        let mut book = TaxLotBook::default();
        let opened = book
            .open_short(ShortTaxLot {
                lot_id: "taxshort-fee-heavy".to_owned(),
                instrument_id: "inst.us_option.cheap".to_owned(),
                currency: usd.clone(),
                opened_at: "2026-01-02T14:31:00Z".to_owned(),
                remaining_quantity: dec("1"),
                unit_proceeds: dec(net_proceeds),
            })
            .expect("a real broker-executed short must be recordable");
        assert!(opened);

        let restored = TaxLotBook::recover(book.snapshot()).expect("and restorable");
        assert_eq!(restored.snapshot(), book.snapshot());

        // Buy back at 1.00 with a 0.10 fee: realized = proceeds - cost - fee.
        let cover = book
            .cover(
                "taxcover-fee-heavy",
                "inst.us_option.cheap",
                &usd,
                dec("1"),
                dec("1.00"),
                dec("0.10"),
                "2026-01-02T14:32:00Z",
                TaxLotSelection::Fifo,
            )
            .unwrap()
            .unwrap();
        assert_eq!(cover.realized_pnl, dec(expected_realized));
        assert_eq!(book.realized(&usd), dec(expected_realized));
        assert!(book.short_lots("inst.us_option.cheap").is_empty());
    }
}

#[test]
fn a_short_lot_still_needs_a_positive_quantity() {
    let mut book = TaxLotBook::default();
    assert!(book
        .open_short(ShortTaxLot {
            lot_id: "taxshort-empty".to_owned(),
            instrument_id: "inst.us_option.cheap".to_owned(),
            currency: Currency::new("USD").unwrap(),
            opened_at: "2026-01-02T14:31:00Z".to_owned(),
            remaining_quantity: Decimal::ZERO,
            unit_proceeds: dec("1"),
        })
        .is_err());
}

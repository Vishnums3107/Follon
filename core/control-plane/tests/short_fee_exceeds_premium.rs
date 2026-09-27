//! A signed portfolio opened short where the commission exceeds the premium.
//!
//! A short's average cost is its net opening proceeds per unit, so it is
//! negative when the fee exceeded the premium. The broker really executed that
//! trade; the portfolio must record it, restore it, and realize the right P&L
//! on close. A long position's cost can still never be negative (found by the
//! PAPER combination property test, conformance-audit item 55).

use std::str::FromStr;

use follon_control_plane::Portfolio;
use follon_domain::{Decimal, Fill, Side};

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn fill(side: Side, price: &str, fee: &str, id: &str) -> Fill {
    Fill {
        execution_id: id.to_owned(),
        order_id: "order.cheap".to_owned(),
        instrument_id: "inst.us_option.cheap".to_owned(),
        side,
        quantity: dec("1"),
        price: dec(price),
        fee: dec(fee),
        executed_at: "2026-01-02T14:31:00Z".to_owned(),
    }
}

#[test]
fn a_short_whose_fee_exceeds_its_premium_is_recorded_restored_and_closed() {
    let mut portfolio = Portfolio::new("acct.cheap", "inst.us_option.cheap");
    portfolio
        .apply_signed_fill(&fill(Side::Sell, "1.00", "1.35", "exec-open"))
        .expect("a real broker-executed short must be recordable");
    let opened = portfolio.position_snapshot();
    assert_eq!(opened.quantity, dec("-1"));
    assert_eq!(opened.average_cost, dec("-0.35"));

    let restored = Portfolio::recover_signed(
        "acct.cheap",
        "inst.us_option.cheap",
        opened.quantity,
        opened.average_cost,
        opened.realized_pnl,
    )
    .expect("and restorable");
    assert_eq!(restored.position_snapshot(), opened);

    // Buy back at 1.00 with a 0.10 fee: realized = -0.35 - 1.00 - 0.10.
    portfolio
        .apply_signed_fill(&fill(Side::Buy, "1.00", "0.10", "exec-close"))
        .unwrap();
    let closed = portfolio.position_snapshot();
    assert_eq!(closed.quantity, Decimal::ZERO);
    assert_eq!(closed.realized_pnl, dec("-1.45"));
}

#[test]
fn a_long_position_still_cannot_restore_with_a_negative_cost() {
    assert!(Portfolio::recover_signed(
        "acct.cheap",
        "inst.us_option.cheap",
        dec("1"),
        dec("-0.35"),
        Decimal::ZERO,
    )
    .is_err());
}

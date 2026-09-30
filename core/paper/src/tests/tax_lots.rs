//! FIFO tax lots and their durability.

use super::*;

#[test]
fn paper_fifo_tax_lots_track_disposal_cost_basis_independent_of_average_cost() {
    let mut service = service();

    // First lot: 2 units @ $100.
    let first = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "2").unwrap(),
                ..intent("intent-tax-lot-001", "2026-01-02T14:31:00Z")
            },
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    let first_order_id = first.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &first_order_id,
            decimal("quantity", "2").unwrap(),
            decimal("price", "100").unwrap(),
            decimal("fee", "0.20").unwrap(),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // Second lot: 2 units @ $120, a distinctly higher price than the
    // first, so a FIFO disposal's cost basis provably differs from what
    // a blended average cost would report.
    let second = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "2").unwrap(),
                ..intent("intent-tax-lot-002", "2026-01-02T14:32:00Z")
            },
            market("2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    let second_order_id = second.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &second_order_id,
            decimal("quantity", "2").unwrap(),
            decimal("price", "120").unwrap(),
            decimal("fee", "0.20").unwrap(),
            "2026-01-02T14:32:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);
    assert_eq!(service.tax_lots("inst.us_equity.spy").len(), 2);

    // Dispose 2 units at $130. FIFO must consume the $100 lot first.
    let sell = service
        .submit_intent(
            OrderIntent {
                side: Side::Sell,
                quantity: decimal("quantity", "2").unwrap(),
                ..intent("intent-tax-lot-003", "2026-01-02T14:33:00Z")
            },
            market("2026-01-02T14:33:00Z"),
            "2026-01-02T14:33:00Z",
        )
        .unwrap();
    let sell_order_id = sell.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &sell_order_id,
            decimal("quantity", "2").unwrap(),
            decimal("price", "130").unwrap(),
            decimal("fee", "0.20").unwrap(),
            "2026-01-02T14:33:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // The FIFO book fully consumed the $100 lot (all-in unit cost
    // 100.10) and left the $120 lot (all-in unit cost 120.10) untouched.
    let remaining = service.tax_lots("inst.us_equity.spy");
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].unit_cost,
        decimal("unit cost", "120.10").unwrap()
    );
    // realized = proceeds(260) - fifo cost basis(200.20) - fee(0.20) = 59.60
    assert_eq!(
        service.realized_tax_pnl().unwrap(),
        decimal("realized", "59.60").unwrap()
    );

    // Portfolio's own blended average-cost realized P&L is a distinct
    // figure: average cost across both lots is 110.10/unit, so
    // realized = (130 - 110.10) * 2 - 0.20 = 39.60 -- proving the
    // tax-lot book is an independent ledger, not a relabeling of the
    // existing average-cost figure.
    let dashboard = service.dashboard();
    assert_eq!(dashboard.positions[0].realized_pnl, "39.60000000");
}

#[test]
fn paper_tax_lots_survive_a_durable_journal_reopen() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-tax-lots.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let account = account();
    let mut durable = PaperTradingService::open_durable(
        account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .unwrap();
    let submitted = durable
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "2").unwrap(),
                ..intent("intent-tax-lot-durable-001", "2026-01-02T14:31:00Z")
            },
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    let order_id = submitted.order_id.unwrap();
    durable
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "2").unwrap(),
            decimal("price", "100").unwrap(),
            decimal("fee", "0.20").unwrap(),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(durable.synchronize().unwrap(), 2);
    assert_eq!(durable.tax_lots("inst.us_equity.spy").len(), 1);
    drop(durable);

    let recovered = PaperTradingService::open_durable(
        account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .unwrap();
    let remaining = recovered.tax_lots("inst.us_equity.spy");
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].unit_cost,
        decimal("unit cost", "100.10").unwrap()
    );
    assert_eq!(
        remaining[0].remaining_quantity,
        decimal("qty", "2").unwrap()
    );
    assert_eq!(recovered.realized_tax_pnl().unwrap(), Decimal::ZERO);
    drop(recovered);
    let _ = fs::remove_file(&journal_path);
}

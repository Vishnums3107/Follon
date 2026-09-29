//! Deterministic execution-management algorithms.
//!
//! This crate converts an already risk-approved parent order into auditable
//! child instructions. It never contacts a broker and cannot bypass OMS/risk.

mod algorithms;
mod basket;
mod combo;
mod error;
mod evidence;
mod passive;
mod plan;
mod protective;
mod routing;
mod tca;

pub use algorithms::*;
pub use basket::*;
pub use combo::*;
pub use error::*;
pub use evidence::*;
pub use passive::*;
pub use plan::*;
pub use protective::*;
pub use routing::*;
pub use tca::*;

#[cfg(test)]
mod tests {

    use std::collections::BTreeSet;

    use std::str::FromStr;

    use follon_domain::{Decimal, Side};

    use super::*;

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn parent(quantity: &str) -> ParentOrder {
        ParentOrder {
            parent_order_id: "parent.exec.001".to_owned(),
            account_id: "account.main".to_owned(),
            instrument_id: "instrument.spy".to_owned(),
            side: Side::Buy,
            quantity: amount(quantity),
            limit_price: Some(amount("100")),
        }
    }

    #[test]
    fn twap_conserves_every_fixed_point_unit() {
        let parent = parent("10.00000001");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Twap {
                slice_count: 3,
                interval_seconds: 60,
            },
        )
        .expect("plan");
        assert_eq!(plan.children.len(), 3);
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);
        assert_eq!(plan.children[2].scheduled_after_seconds, 120);
        plan.validate_against(&parent).expect("conservation");
    }

    #[test]
    fn vwap_follows_forecast_curve_and_conserves_rounding_remainder() {
        let parent = parent("10.00000001");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Vwap {
                forecast_market_volumes: vec![amount("1"), amount("2"), amount("1")],
                interval_seconds: 60,
            },
        )
        .expect("plan");
        assert_eq!(plan.children[0].quantity, amount("2.5"));
        assert_eq!(plan.children[1].quantity, amount("5"));
        assert_eq!(plan.children[2].quantity, amount("2.50000001"));
        plan.validate_against(&parent).expect("conservation");
    }

    #[test]
    fn participation_never_exceeds_volume_or_parent() {
        let parent = parent("10");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Participation {
                participation_bps: 1_000,
                observed_market_volumes: vec![amount("20"), amount("30")],
                interval_seconds: 30,
            },
        )
        .expect("plan");
        assert_eq!(plan.children[0].quantity, amount("2"));
        assert_eq!(plan.children[1].quantity, amount("3"));
        assert_eq!(plan.unallocated_quantity, amount("5"));
    }

    #[test]
    fn arrival_price_front_loads_and_conserves_every_unit() {
        let parent = parent("10.00000001");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::ArrivalPrice {
                slice_count: 4,
                interval_seconds: 15,
                urgency_bps: 8_000,
            },
        )
        .expect("arrival plan");
        assert_eq!(plan.algorithm, "arrival-price-v1");
        assert!(plan.children[0].quantity > plan.children[3].quantity);
        assert_eq!(plan.children[3].scheduled_after_seconds, 45);
        plan.validate_against(&parent).expect("conservation");
    }

    #[test]
    fn option_combo_enforces_ratios_and_net_debit_without_leg_risk() {
        let plan = plan_option_combo(
            "combo.vertical.001",
            amount("3"),
            ComboPriceLimit::MaximumDebit(amount("2.50")),
            &[
                OptionComboLeg {
                    instrument_id: "option.spy.500.call".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: amount("5.00"),
                },
                OptionComboLeg {
                    instrument_id: "option.spy.505.call".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: amount("2.75"),
                },
            ],
        )
        .expect("protected combo");
        assert_eq!(plan.protected_net_price, amount("2.25"));
        assert_eq!(plan.legs[0].quantity, amount("3"));
        assert!(plan_option_combo(
            "combo.vertical.002",
            amount("1"),
            ComboPriceLimit::MaximumDebit(amount("2")),
            &[
                OptionComboLeg {
                    instrument_id: "option.spy.500.call".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: amount("5"),
                },
                OptionComboLeg {
                    instrument_id: "option.spy.505.call".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: amount("2.75"),
                },
            ],
        )
        .is_err());
    }

    #[test]
    fn passive_repricing_is_monotonic_post_only_and_strictly_collared() {
        let mut parent = parent("5");
        parent.limit_price = Some(amount("101"));
        let plan = plan_passive_repricing(
            &parent,
            &PassiveRepricePolicy {
                initial_limit_price: amount("99"),
                tick_size: amount("0.01"),
                maximum_chase_bps: 110,
                maximum_replacements: 4,
                minimum_replace_interval_seconds: 5,
            },
            &[
                PassiveMarketObservation {
                    observed_after_seconds: 5,
                    best_bid: amount("99"),
                    best_ask: amount("100"),
                },
                PassiveMarketObservation {
                    observed_after_seconds: 10,
                    best_bid: amount("100"),
                    best_ask: amount("100.50"),
                },
                PassiveMarketObservation {
                    observed_after_seconds: 15,
                    best_bid: amount("100.50"),
                    best_ask: amount("100.75"),
                },
            ],
        )
        .expect("bounded passive plan");
        assert_eq!(plan.replacements.len(), 1);
        assert_eq!(
            plan.replacements[0].replacement.limit_price,
            Some(amount("100"))
        );
        assert_eq!(
            plan.replacements[0].cancel_child_order_id,
            plan.initial.child_order_id
        );
        assert_eq!(plan.replacements[0].replacement.quantity, parent.quantity);
    }

    #[test]
    fn passive_replacement_clamped_to_an_off_grid_hard_limit_stays_on_the_tick_grid() {
        // Shrunk by `passive_repricing_proptest`: a sell collared at 3.86999999
        // on a 0.01 grid used to be replaced at exactly 3.86999999, which a
        // venue rejects after the working child's cancel is confirmed.
        let observations = [PassiveMarketObservation {
            observed_after_seconds: 5,
            best_bid: amount("3.73"),
            best_ask: amount("3.78"),
        }];
        let policy = PassiveRepricePolicy {
            initial_limit_price: amount("3.87"),
            tick_size: amount("0.01"),
            maximum_chase_bps: 100,
            maximum_replacements: 1,
            minimum_replace_interval_seconds: 1,
        };
        let mut sell = parent("1");
        sell.side = Side::Sell;
        sell.limit_price = Some(amount("3.86999999"));
        let plan = plan_passive_repricing(&sell, &policy, &observations).expect("sell plan");
        // The nearest on-grid price at or above the collar is 3.87, the initial
        // price itself, so there is nothing to improve on and no replacement.
        assert!(plan.replacements.is_empty());

        sell.limit_price = Some(amount("3.85999999"));
        let plan = plan_passive_repricing(&sell, &policy, &observations).expect("sell plan");
        assert_eq!(
            plan.replacements[0].replacement.limit_price,
            Some(amount("3.86"))
        );

        let mut buy = parent("1");
        buy.limit_price = Some(amount("3.75000001"));
        let buy_policy = PassiveRepricePolicy {
            initial_limit_price: amount("3.70"),
            maximum_chase_bps: 500,
            ..policy
        };
        let buy_observations = [PassiveMarketObservation {
            observed_after_seconds: 5,
            best_bid: amount("3.77"),
            best_ask: amount("3.80"),
        }];
        let plan = plan_passive_repricing(&buy, &buy_policy, &buy_observations).expect("buy plan");
        assert_eq!(
            plan.replacements[0].replacement.limit_price,
            Some(amount("3.75"))
        );
    }

    #[test]
    fn tied_depth_from_one_venue_routes_the_same_in_any_order() {
        // Two levels from one venue tie on all-in price, rank and venue; the
        // larger routes first, whatever order the book lists them in.
        let level = |size: &str| VenueQuote {
            venue: "venue.a".to_owned(),
            available_quantity: amount(size),
            price: amount("99"),
            fee_per_unit: Decimal::ZERO,
            latency_rank: 0,
        };
        let parent = parent("5");
        let forward = smart_route(&parent, &[level("3"), level("4")]).expect("route");
        let reverse = smart_route(&parent, &[level("4"), level("3")]).expect("route");
        assert_eq!(forward, reverse);
        assert_eq!(forward.children[0].quantity, amount("4"));
        assert_eq!(forward.children[1].quantity, amount("1"));
    }

    #[test]
    fn a_market_parent_routes_as_marketable_limits_through_both_routers() {
        let mut market_parent = parent("5");
        market_parent.limit_price = None;
        let quotes = [VenueQuote {
            venue: "venue.a".to_owned(),
            available_quantity: amount("9"),
            price: amount("99.5"),
            fee_per_unit: Decimal::ZERO,
            latency_rank: 0,
        }];
        let capability = |kinds: &[ChildOrderKind]| VenueCapability {
            venue: "venue.a".to_owned(),
            capability_version: "cap.a.v1".to_owned(),
            supported_order_kinds: kinds.iter().copied().collect(),
            supports_iceberg: false,
            min_quantity: None,
            max_quantity: None,
        };
        let plain = smart_route(&market_parent, &quotes).expect("plain route");
        let (gated, _) = smart_route_with_capabilities(
            &market_parent,
            &quotes,
            &[capability(&[ChildOrderKind::Limit])],
        )
        .expect("gated route");
        assert_eq!(gated, plain);
        assert_eq!(gated.children[0].kind, ChildOrderKind::Limit);
        assert_eq!(gated.children[0].limit_price, Some(amount("99.5")));
        // A venue that cannot take a limit order cannot receive a routed child.
        assert!(smart_route_with_capabilities(
            &market_parent,
            &quotes,
            &[capability(&[ChildOrderKind::Market])],
        )
        .is_err());
    }

    #[test]
    fn smart_router_uses_best_all_in_prices_and_respects_limit() {
        let parent = parent("5");
        let plan = smart_route(
            &parent,
            &[
                VenueQuote {
                    venue: "venue.slow".to_owned(),
                    available_quantity: amount("3"),
                    price: amount("99.90"),
                    fee_per_unit: amount("0.05"),
                    latency_rank: 2,
                },
                VenueQuote {
                    venue: "venue.fast".to_owned(),
                    available_quantity: amount("4"),
                    price: amount("99.92"),
                    fee_per_unit: amount("0.01"),
                    latency_rank: 1,
                },
                VenueQuote {
                    venue: "venue.over-limit".to_owned(),
                    available_quantity: amount("5"),
                    price: amount("100.01"),
                    fee_per_unit: Decimal::ZERO,
                    latency_rank: 0,
                },
            ],
        )
        .expect("route");
        assert_eq!(plan.children[0].venue.as_deref(), Some("venue.fast"));
        assert_eq!(plan.children[1].quantity, amount("1"));
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);
    }

    #[test]
    fn bracket_trailing_and_basket_contracts_are_fail_closed() {
        let parent = parent("2");
        let bracket = bracket_children(&parent, amount("110"), amount("95"), Some(amount("94.50")))
            .expect("bracket");
        assert_eq!(bracket[1].kind, ChildOrderKind::StopLimit);

        let mut trailing = TrailingStop::new(Side::Sell, 500, amount("100")).expect("trail");
        assert_eq!(
            trailing.update(amount("110")).expect("advance"),
            amount("104.5")
        );
        assert_eq!(
            trailing.update(amount("105")).expect("no retreat"),
            amount("104.5")
        );

        let basket = plan_basket(
            "basket.alpha",
            "account.main",
            amount("10000"),
            &[
                BasketLeg {
                    instrument_id: "instrument.spy".to_owned(),
                    side: Side::Buy,
                    weight_bps: 6_000,
                    reference_price: amount("100"),
                },
                BasketLeg {
                    instrument_id: "instrument.tlt".to_owned(),
                    side: Side::Sell,
                    weight_bps: 4_000,
                    reference_price: amount("80"),
                },
            ],
        )
        .expect("basket");
        assert_eq!(basket[0].quantity, amount("60"));
        assert_eq!(basket[1].quantity, amount("50"));
        assert!(plan_basket("basket.bad", "account.main", amount("1"), &[]).is_err());
    }

    #[test]
    fn transaction_cost_analysis_measures_arrival_target_fees_and_partial_fills() {
        let report = analyze_transaction_cost(&TransactionCostInput {
            analysis_id: "tca.alpha.001".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            parent_order_id: "parent.alpha.001".to_owned(),
            execution_algorithm: "twap-v1".to_owned(),
            order_type: "limit".to_owned(),
            side: Side::Buy,
            arrival_price: amount("100"),
            target_price: amount("101"),
            requested_quantity: amount("10"),
            fills: vec![
                TcaFill {
                    execution_id: "execution.alpha.001".to_owned(),
                    quantity: amount("4"),
                    price: amount("101"),
                    fee: amount("0.25"),
                },
                TcaFill {
                    execution_id: "execution.alpha.002".to_owned(),
                    quantity: amount("4"),
                    price: amount("102"),
                    fee: amount("0.25"),
                },
            ],
        })
        .expect("TCA report");
        assert_eq!(report.filled_quantity, amount("8"));
        assert_eq!(report.unfilled_quantity, amount("2"));
        assert_eq!(report.execution_vwap, Some(amount("101.5")));
        assert_eq!(report.arrival_price_cost, amount("12"));
        assert_eq!(report.target_price_cost, amount("4"));
        assert_eq!(report.fees, amount("0.5"));
        assert_eq!(report.arrival_total_cost, amount("12.5"));
        assert_eq!(report.target_total_cost, amount("4.5"));
        assert_eq!(report.arrival_price_cost_bps, amount("150"));
        assert_eq!(report.target_price_cost_bps, amount("49.50495049"));
    }

    #[test]
    fn transaction_cost_batch_groups_sides_and_rejects_duplicate_evidence() {
        let buy = TransactionCostInput {
            analysis_id: "tca.alpha.buy".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            parent_order_id: "parent.alpha.buy".to_owned(),
            execution_algorithm: "vwap-v1".to_owned(),
            order_type: "market".to_owned(),
            side: Side::Buy,
            arrival_price: amount("100"),
            target_price: amount("100"),
            requested_quantity: amount("1"),
            fills: vec![TcaFill {
                execution_id: "execution.alpha.buy".to_owned(),
                quantity: amount("1"),
                price: amount("101"),
                fee: Decimal::ZERO,
            }],
        };
        let mut sell = buy.clone();
        sell.analysis_id = "tca.alpha.sell".to_owned();
        sell.parent_order_id = "parent.alpha.sell".to_owned();
        sell.side = Side::Sell;
        sell.fills[0].execution_id = "execution.alpha.sell".to_owned();
        sell.fills[0].price = amount("99");
        let batch = analyze_transaction_costs(&[buy.clone(), sell]).expect("grouped TCA");
        assert_eq!(batch.buckets.len(), 1);
        assert_eq!(batch.buckets[0].arrival_price_cost, amount("2"));
        assert!(batch
            .canonical_json()
            .contains("transaction_cost_schema_version"));
        assert!(batch
            .markdown_report()
            .contains("Aggregate execution quality"));

        let mut duplicate = buy;
        duplicate.parent_order_id = "parent.alpha.duplicate".to_owned();
        assert!(analyze_transaction_costs(&[duplicate.clone(), duplicate]).is_err());
    }

    #[test]
    fn test_execution_planners_twap_vwap_arrival() {
        let parent = ParentOrder {
            parent_order_id: "parent.test.100".to_owned(),
            account_id: "acct.paper.001".to_owned(),
            instrument_id: "aapl.us".to_owned(),
            side: Side::Buy,
            quantity: amount("100"),
            limit_price: Some(amount("150")),
        };

        // 1. TWAP Planner (4 slices over 300s -> 100s intervals, 25 qty each)
        let twap_plan = plan_twap_execution(&parent, 300, 4, ChildOrderKind::Limit).expect("twap");
        assert_eq!(twap_plan.children.len(), 4);
        assert_eq!(twap_plan.unallocated_quantity, Decimal::ZERO);
        assert_eq!(twap_plan.children[0].quantity, amount("25"));
        assert_eq!(twap_plan.children[0].scheduled_after_seconds, 0);
        assert_eq!(twap_plan.children[3].scheduled_after_seconds, 300);

        // 2. VWAP Planner (4 slices with weights 10%, 20%, 30%, 40%)
        let profile = vec![
            amount("0.10"),
            amount("0.20"),
            amount("0.30"),
            amount("0.40"),
        ];
        let vwap_plan =
            plan_vwap_execution(&parent, 60, &profile, ChildOrderKind::Limit).expect("vwap");
        assert_eq!(vwap_plan.children.len(), 4);
        assert_eq!(vwap_plan.unallocated_quantity, Decimal::ZERO);
        assert_eq!(vwap_plan.children[0].quantity, amount("10"));
        assert_eq!(vwap_plan.children[3].quantity, amount("40"));

        // 3. Arrival Price Planner (4 front-loaded decay slices: 40%, 30%, 20%, 10%)
        let arrival_plan =
            plan_arrival_price_execution(&parent, 120, amount("5000"), ChildOrderKind::Limit)
                .expect("arrival");
        assert_eq!(arrival_plan.children.len(), 4);
        assert_eq!(arrival_plan.unallocated_quantity, Decimal::ZERO);
        assert_eq!(arrival_plan.children[0].quantity, amount("40"));
        assert_eq!(arrival_plan.children[3].quantity, amount("10"));
    }

    #[test]
    fn iceberg_conserves_parent_quantity_and_enforces_interval() {
        let parent = parent("10.5");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Iceberg {
                display_quantity: amount("3"),
                interval_seconds: 45,
            },
        )
        .expect("iceberg plan");
        assert_eq!(plan.algorithm, "iceberg-v1");
        assert_eq!(plan.children.len(), 4);
        assert_eq!(plan.children[0].quantity, amount("3"));
        assert_eq!(plan.children[1].quantity, amount("3"));
        assert_eq!(plan.children[2].quantity, amount("3"));
        assert_eq!(plan.children[3].quantity, amount("1.5"));
        assert_eq!(plan.children[0].scheduled_after_seconds, 0);
        assert_eq!(plan.children[1].scheduled_after_seconds, 45);
        assert_eq!(plan.children[2].scheduled_after_seconds, 90);
        assert_eq!(plan.children[3].scheduled_after_seconds, 135);
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);
        assert_eq!(plan.children[0].kind, ChildOrderKind::Limit);
        plan.validate_against(&parent).expect("conserved");

        // Market iceberg when parent has no limit
        let mut market_parent = parent.clone();
        market_parent.quantity = amount("5");
        market_parent.limit_price = None;
        let mkt_plan =
            plan_iceberg_execution(&market_parent, amount("2"), 30, ChildOrderKind::Market)
                .expect("market iceberg");
        assert_eq!(mkt_plan.children.len(), 3);
        assert_eq!(mkt_plan.children[0].kind, ChildOrderKind::Market);
        assert_eq!(mkt_plan.children[2].quantity, amount("1"));

        // Refuse invalid configs
        assert!(plan_iceberg_execution(&parent, amount("0"), 30, ChildOrderKind::Limit).is_err());
        assert!(plan_iceberg_execution(&parent, amount("1"), 0, ChildOrderKind::Limit).is_err());
    }

    #[test]
    fn algo_wheel_allocates_exact_weights_and_breaks_schedule_ties_deterministically() {
        let parent = parent("100");
        let wheel = ExecutionAlgorithm::AlgoWheel {
            allocations: vec![
                AlgoWheelAllocation {
                    algorithm: ExecutionAlgorithm::Twap {
                        slice_count: 2,
                        interval_seconds: 60,
                    },
                    weight_bps: 6_000,
                },
                AlgoWheelAllocation {
                    algorithm: ExecutionAlgorithm::Twap {
                        slice_count: 2,
                        interval_seconds: 60,
                    },
                    weight_bps: 4_000,
                },
            ],
        };
        let plan = plan_execution(&parent, &wheel).expect("wheel plan");
        assert_eq!(plan.algorithm, "algo-wheel-v1");
        assert_eq!(plan.children.len(), 4);
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);

        // Schedule offsets:
        // Branch 0 (60%): 60 qty -> 2 slices of 30 at offsets 0 and 60
        // Branch 1 (40%): 40 qty -> 2 slices of 20 at offsets 0 and 60
        // Sorted ties: offset 0 has branch 0 child 1 (30), then branch 1 child 1 (20)
        // Offset 60 has branch 0 child 2 (30), then branch 1 child 2 (20)
        assert_eq!(plan.children[0].scheduled_after_seconds, 0);
        assert_eq!(plan.children[0].quantity, amount("30"));
        assert_eq!(plan.children[1].scheduled_after_seconds, 0);
        assert_eq!(plan.children[1].quantity, amount("20"));
        assert_eq!(plan.children[2].scheduled_after_seconds, 60);
        assert_eq!(plan.children[2].quantity, amount("30"));
        assert_eq!(plan.children[3].scheduled_after_seconds, 60);
        assert_eq!(plan.children[3].quantity, amount("20"));

        plan.validate_against(&parent).expect("conservation");

        // Replay equality: running twice with identical inputs yields identical child orders
        let plan_repeat = plan_execution(&parent, &wheel).expect("repeat");
        assert_eq!(plan, plan_repeat);

        // Reject nested algo-wheel
        let nested = ExecutionAlgorithm::AlgoWheel {
            allocations: vec![AlgoWheelAllocation {
                algorithm: wheel.clone(),
                weight_bps: 10_000,
            }],
        };
        assert!(plan_execution(&parent, &nested).is_err());

        // Reject weights not summing to 10000
        let bad_weights = ExecutionAlgorithm::AlgoWheel {
            allocations: vec![AlgoWheelAllocation {
                algorithm: ExecutionAlgorithm::Immediate,
                weight_bps: 5_000,
            }],
        };
        assert!(plan_execution(&parent, &bad_weights).is_err());
    }

    #[test]
    fn capability_gated_routing_refuses_unknown_duplicate_and_unsupported_venues() {
        let parent = parent("10");
        let quotes = vec![
            VenueQuote {
                venue: "venue.nyse".to_owned(),
                available_quantity: amount("6"),
                price: amount("100"),
                fee_per_unit: amount("0.01"),
                latency_rank: 1,
            },
            VenueQuote {
                venue: "venue.nasdaq".to_owned(),
                available_quantity: amount("6"),
                price: amount("99.98"),
                fee_per_unit: amount("0.01"),
                latency_rank: 2,
            },
        ];

        let mut supported_kinds = BTreeSet::new();
        supported_kinds.insert(ChildOrderKind::Limit);

        let valid_caps = vec![
            VenueCapability {
                venue: "venue.nyse".to_owned(),
                capability_version: "cap.nyse.v1".to_owned(),
                supported_order_kinds: supported_kinds.clone(),
                supports_iceberg: true,
                min_quantity: Some(amount("1")),
                max_quantity: Some(amount("1000")),
            },
            VenueCapability {
                venue: "venue.nasdaq".to_owned(),
                capability_version: "cap.nasdaq.v1".to_owned(),
                supported_order_kinds: supported_kinds.clone(),
                supports_iceberg: false,
                min_quantity: None,
                max_quantity: None,
            },
        ];

        // 1. Success case
        let (plan, decisions) =
            smart_route_with_capabilities(&parent, &quotes, &valid_caps).expect("gated route");
        assert_eq!(plan.children.len(), 2);
        assert_eq!(decisions.len(), 2);
        // venue.nasdaq has all-in 99.99 vs nyse 100.01 -> nasdaq first
        assert_eq!(decisions[0].venue, "venue.nasdaq");
        assert_eq!(decisions[0].allocated_quantity, amount("6"));
        assert_eq!(decisions[1].venue, "venue.nyse");
        assert_eq!(decisions[1].allocated_quantity, amount("4"));
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);

        // 2. Refuses unknown venue capability
        let partial_caps = vec![valid_caps[0].clone()];
        assert!(smart_route_with_capabilities(&parent, &quotes, &partial_caps).is_err());

        // 3. Refuses duplicate capability record
        let duplicate_caps = vec![valid_caps[0].clone(), valid_caps[0].clone()];
        assert!(smart_route_with_capabilities(&parent, &quotes, &duplicate_caps).is_err());

        // 4. Refuses unsupported order kind
        let mut market_only_kinds = BTreeSet::new();
        market_only_kinds.insert(ChildOrderKind::Market);
        let market_only_caps = vec![
            valid_caps[0].clone(),
            VenueCapability {
                venue: "venue.nasdaq".to_owned(),
                capability_version: "cap.nasdaq.v1".to_owned(),
                supported_order_kinds: market_only_kinds,
                supports_iceberg: false,
                min_quantity: None,
                max_quantity: None,
            },
        ];
        // Parent is a Limit order, so nasdaq declaring only Market causes refusal
        assert!(smart_route_with_capabilities(&parent, &quotes, &market_only_caps).is_err());
    }

    #[test]
    fn execution_plan_evidence_binds_evidence_fingerprint_and_detects_tampering() {
        let parent = parent("10");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Twap {
                slice_count: 2,
                interval_seconds: 30,
            },
        )
        .expect("twap");

        let benchmark = ExecutionBenchmarkEvidence {
            benchmark_id: "bmk.test.001".to_owned(),
            parent_order_id: parent.parent_order_id.clone(),
            arrival_price: amount("100"),
            target_price: amount("100.05"),
            source: "quote.mid.v1".to_owned(),
        };

        let evidence = ExecutionPlanEvidence::new(
            "evidence.plan.001".to_owned(),
            &parent,
            &plan,
            Some("cap.nyse.v1".to_owned()),
            Vec::new(),
            Some(benchmark),
            "2026-09-04T10:00:00Z".to_owned(),
        )
        .expect("plan evidence");

        assert!(evidence.verify_fingerprint());
        assert!(!evidence.plan_sha256.is_empty());

        let json = evidence.canonical_json();
        assert!(json.contains("evidence.plan.001"));
        assert!(json.contains("twap-v1"));
        assert!(json.contains(&evidence.plan_sha256));

        // Tamper detection: modifying quantity breaks fingerprint verification
        let mut tampered = evidence.clone();
        tampered.parent_quantity = amount("11");
        assert!(!tampered.verify_fingerprint());
    }
}

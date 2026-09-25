//! Property tests for smart routing.
//!
//! `smart_route` and `smart_route_with_capabilities` end with
//! `ExecutionPlan::validate_against`, which proves only conservation. These
//! properties check the routers' own job, over random venue books, against
//! arithmetic written here in 1e-8 units:
//!
//! - every child names a quoted venue, is priced at that quote, never exceeds
//!   the quote's size (or the venue's `max_quantity`), and respects the
//!   parent's limit on the quote price;
//! - children never get worse in all-in price (price plus fee for a buy, minus
//!   fee for a sell), ties broken by latency rank, then venue, then larger
//!   size first;
//! - every child is a marketable limit at its quote price, whether or not the
//!   parent has a limit (the operator-decided contract of audit item 63);
//! - nothing is left unallocated while a quote inside the limit still has
//!   size;
//! - each plan equals an exact greedy oracle, including the capability
//!   router's `min_quantity` and `max_quantity` rules;
//! - quote order does not matter, including for several tied quotes from one
//!   venue;
//! - with permissive capabilities the gated router equals the plain one, for
//!   market and limit parents alike;
//! - every route decision mirrors its child, and malformed inputs are refused.

use std::cmp::Reverse;
use std::collections::BTreeSet;

use follon_domain::{Decimal, Side};
use follon_execution::{
    smart_route, smart_route_with_capabilities, ChildOrderKind, ExecutionPlan, ParentOrder,
    VenueCapability, VenueQuote,
};
use proptest::prelude::*;

const UNIT: i128 = 100_000_000;
const CENT: i128 = UNIT / 100;

#[derive(Clone, Debug)]
struct Quote {
    venue: String,
    available: i128,
    price: i128,
    fee: i128,
    latency: u32,
}

impl Quote {
    fn to_quote(&self) -> VenueQuote {
        VenueQuote {
            venue: self.venue.clone(),
            available_quantity: Decimal::from_scaled(self.available),
            price: Decimal::from_scaled(self.price),
            fee_per_unit: Decimal::from_scaled(self.fee),
            latency_rank: self.latency,
        }
    }

    fn all_in(&self, side: Side) -> i128 {
        match side {
            Side::Buy => self.price + self.fee,
            Side::Sell => self.price - self.fee,
        }
    }

    fn within_limit(&self, side: Side, limit: Option<i128>) -> bool {
        match (side, limit) {
            (Side::Buy, Some(limit)) => self.price <= limit,
            (Side::Sell, Some(limit)) => self.price >= limit,
            (_, None) => true,
        }
    }
}

#[derive(Clone, Debug)]
struct Book {
    side: Side,
    quantity: i128,
    limit: Option<i128>,
    quotes: Vec<Quote>,
    /// Per venue, `(min_quantity, max_quantity)` for the gated router.
    bounds: Vec<(Option<i128>, Option<i128>)>,
}

impl Book {
    fn parent(&self) -> ParentOrder {
        ParentOrder {
            parent_order_id: "parent.route".to_owned(),
            account_id: "acct.proptest".to_owned(),
            instrument_id: "inst.us_equity.proptest".to_owned(),
            side: self.side,
            quantity: Decimal::from_scaled(self.quantity),
            limit_price: self.limit.map(Decimal::from_scaled),
        }
    }

    fn quotes(&self) -> Vec<VenueQuote> {
        self.quotes.iter().map(Quote::to_quote).collect()
    }

    fn capabilities(&self, with_bounds: bool) -> Vec<VenueCapability> {
        self.quotes
            .iter()
            .zip(&self.bounds)
            .map(|(quote, (min, max))| VenueCapability {
                venue: quote.venue.clone(),
                capability_version: "venue.cap.v1".to_owned(),
                supported_order_kinds: BTreeSet::from([
                    ChildOrderKind::Limit,
                    ChildOrderKind::Market,
                ]),
                supports_iceberg: false,
                min_quantity: min.filter(|_| with_bounds).map(Decimal::from_scaled),
                max_quantity: max.filter(|_| with_bounds).map(Decimal::from_scaled),
            })
            .collect()
    }

    /// Quotes in the order a correct router must consider them.
    fn ranked(&self) -> Vec<(usize, &Quote)> {
        let mut ranked: Vec<(usize, &Quote)> = self.quotes.iter().enumerate().collect();
        ranked.sort_by_key(|(_, quote)| {
            let all_in = quote.all_in(self.side);
            (
                if self.side == Side::Buy {
                    all_in
                } else {
                    -all_in
                },
                quote.latency,
                quote.venue.clone(),
                Reverse(quote.available),
            )
        });
        ranked
    }

    /// `(venue, quantity, price)` a correct router allocates; the gated
    /// router also applies each venue's bounds.
    fn expected(&self, with_bounds: bool) -> (Vec<(String, i128, i128)>, i128) {
        let mut remaining = self.quantity;
        let mut children = Vec::new();
        for (index, quote) in self.ranked() {
            if remaining == 0 {
                break;
            }
            let (min, max) = if with_bounds {
                self.bounds[index]
            } else {
                (None, None)
            };
            let capacity = max.map_or(quote.available, |max| quote.available.min(max));
            if min.is_some_and(|min| capacity < min) {
                continue;
            }
            let quantity = capacity.min(remaining);
            if min.is_some_and(|min| quantity < min) || !quote.within_limit(self.side, self.limit) {
                continue;
            }
            children.push((quote.venue.clone(), quantity, quote.price));
            remaining -= quantity;
        }
        (children, remaining)
    }
}

fn book() -> impl Strategy<Value = Book> {
    (
        prop_oneof![Just(Side::Buy), Just(Side::Sell)],
        1i128..=3_000,
        prop::option::of(90i128..=110),
        prop::collection::vec(
            (
                1i128..=500,
                // Few distinct prices, fees and ranks, so ties are common.
                90i128..=110,
                // Fees in cents, wide enough to reorder quotes against their price.
                prop_oneof![Just(0i128), Just(1i128), 0i128..=300],
                0u32..=3,
                prop::option::of(1i128..=100),
                prop::option::of(1i128..=600),
            ),
            1..=12,
        ),
    )
        .prop_map(|(side, units, limit_cents, raw)| {
            let mut quotes = Vec::new();
            let mut bounds = Vec::new();
            for (index, (available, price, fee, latency, min, max)) in raw.into_iter().enumerate() {
                quotes.push(Quote {
                    venue: format!("venue.v{index:02}"),
                    available: available * UNIT,
                    price: price * CENT,
                    fee: fee * CENT,
                    latency,
                });
                let max = max.map(|max| max.max(min.unwrap_or(1)));
                bounds.push((min.map(|min| min * UNIT), max.map(|max| max * UNIT)));
            }
            Book {
                side,
                quantity: units * UNIT,
                limit: limit_cents.map(|cents| cents * CENT),
                quotes,
                bounds,
            }
        })
}

/// Books in which quotes share a few venues, as multi-level depth does, and
/// often tie on price, fee and rank.
fn depth_book() -> impl Strategy<Value = Book> {
    (book(), prop::collection::vec(0usize..3, 12)).prop_map(|(mut book, venues)| {
        for (quote, venue) in book.quotes.iter_mut().zip(venues) {
            quote.venue = format!("venue.d{venue}");
            quote.price = 100 * CENT;
            quote.fee = 0;
            quote.latency = 0;
        }
        book
    })
}

/// One permissive capability per distinct venue.
fn unique_capabilities(book: &Book) -> Vec<VenueCapability> {
    let venues: BTreeSet<&str> = book
        .quotes
        .iter()
        .map(|quote| quote.venue.as_str())
        .collect();
    venues
        .into_iter()
        .map(|venue| VenueCapability {
            venue: venue.to_owned(),
            capability_version: "venue.cap.v1".to_owned(),
            supported_order_kinds: BTreeSet::from([ChildOrderKind::Limit]),
            supports_iceberg: false,
            min_quantity: None,
            max_quantity: None,
        })
        .collect()
}

fn children(plan: &ExecutionPlan) -> Vec<(String, i128, i128)> {
    plan.children
        .iter()
        .map(|child| {
            (
                child.venue.clone().expect("a routed child names its venue"),
                child.quantity.scaled(),
                child
                    .limit_price
                    .expect("a routed child carries its quote price")
                    .scaled(),
            )
        })
        .collect()
}

fn assert_routing_invariants(book: &Book, plan: &ExecutionPlan) -> Result<(), TestCaseError> {
    let mut previous: Option<(i128, u32, String)> = None;
    for (index, child) in plan.children.iter().enumerate() {
        prop_assert_eq!(
            &child.child_order_id,
            &format!("parent.route.route.{:04}", index + 1)
        );
        prop_assert_eq!(child.scheduled_after_seconds, 0);
        prop_assert_eq!(
            child.kind,
            ChildOrderKind::Limit,
            "a routed child must be a marketable limit"
        );
        let venue = child.venue.as_deref().expect("venue");
        let quote = book
            .quotes
            .iter()
            .find(|quote| quote.venue == venue)
            .expect("a child must route to a quoted venue");
        prop_assert_eq!(
            child.limit_price.map(|price| price.scaled()),
            Some(quote.price)
        );
        prop_assert!(
            child.quantity.scaled() <= quote.available,
            "child exceeds displayed size"
        );
        prop_assert!(
            quote.within_limit(book.side, book.limit),
            "child breaches the parent limit"
        );
        let key = (quote.all_in(book.side), quote.latency, quote.venue.clone());
        if let Some(previous) = &previous {
            let worse = match book.side {
                Side::Buy => key.0 < previous.0,
                Side::Sell => key.0 > previous.0,
            };
            prop_assert!(!worse, "a better all-in quote was routed after a worse one");
            if key.0 == previous.0 {
                prop_assert!(
                    (key.1, &key.2) > (previous.1, &previous.2),
                    "tie-break out of order"
                );
            }
        }
        previous = Some(key);
    }
    let total: i128 = plan
        .children
        .iter()
        .map(|child| child.quantity.scaled())
        .sum();
    prop_assert_eq!(total + plan.unallocated_quantity.scaled(), book.quantity);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn plain_routing_is_the_exact_greedy_best_execution(book in book()) {
        let plan = smart_route(&book.parent(), &book.quotes()).expect("a valid book must route");
        assert_routing_invariants(&book, &plan)?;
        let (expected, unallocated) = book.expected(false);
        prop_assert_eq!(children(&plan), expected);
        prop_assert_eq!(plan.unallocated_quantity.scaled(), unallocated);
        if unallocated > 0 {
            for quote in book.quotes.iter().filter(|quote| quote.within_limit(book.side, book.limit)) {
                let routed: i128 = plan
                    .children
                    .iter()
                    .filter(|child| child.venue.as_deref() == Some(quote.venue.as_str()))
                    .map(|child| child.quantity.scaled())
                    .sum();
                prop_assert_eq!(routed, quote.available, "eligible size left unrouted at {}", quote.venue);
            }
        }
    }

    #[test]
    fn gated_routing_applies_venue_bounds_exactly(book in book()) {
        let (plan, decisions) = smart_route_with_capabilities(
            &book.parent(),
            &book.quotes(),
            &book.capabilities(true),
        )
        .expect("a valid book must route");
        assert_routing_invariants(&book, &plan)?;
        let (expected, unallocated) = book.expected(true);
        prop_assert_eq!(children(&plan), expected);
        prop_assert_eq!(plan.unallocated_quantity.scaled(), unallocated);
        for (child, (min, max)) in plan.children.iter().map(|child| {
            let index = book.quotes.iter().position(|quote| Some(quote.venue.as_str()) == child.venue.as_deref()).unwrap();
            (child, book.bounds[index])
        }) {
            prop_assert!(min.is_none_or(|min| child.quantity.scaled() >= min), "child under venue minimum");
            prop_assert!(max.is_none_or(|max| child.quantity.scaled() <= max), "child over venue maximum");
        }
        prop_assert_eq!(decisions.len(), plan.children.len());
        for (index, (decision, child)) in decisions.iter().zip(&plan.children).enumerate() {
            let quote = book.quotes.iter().find(|quote| Some(quote.venue.as_str()) == child.venue.as_deref()).unwrap();
            prop_assert_eq!(&decision.decision_id, &format!("parent.route.decision.{:04}", index + 1));
            prop_assert_eq!(Some(decision.venue.as_str()), child.venue.as_deref());
            prop_assert_eq!(decision.allocated_quantity, child.quantity);
            prop_assert_eq!(decision.all_in_price.scaled(), quote.all_in(book.side));
            prop_assert_eq!(decision.fee_per_unit.scaled(), quote.fee);
            prop_assert_eq!(decision.latency_rank, quote.latency);
        }
    }

    #[test]
    fn quote_order_does_not_change_either_plan(book in book(), seed in any::<u64>()) {
        let mut shuffled = book.clone();
        // A deterministic permutation derived from the seed.
        let count = shuffled.quotes.len();
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|index| Reverse((*index as u64).wrapping_mul(seed | 1).rotate_left(17)));
        shuffled.quotes = order.iter().map(|index| book.quotes[*index].clone()).collect();
        shuffled.bounds = order.iter().map(|index| book.bounds[*index]).collect();
        prop_assert_eq!(
            smart_route(&book.parent(), &book.quotes()).unwrap(),
            smart_route(&shuffled.parent(), &shuffled.quotes()).unwrap()
        );
        prop_assert_eq!(
            smart_route_with_capabilities(&book.parent(), &book.quotes(), &book.capabilities(true)).unwrap(),
            smart_route_with_capabilities(&shuffled.parent(), &shuffled.quotes(), &shuffled.capabilities(true)).unwrap()
        );
    }

    #[test]
    fn tied_depth_from_one_venue_routes_the_same_in_any_order(book in depth_book()) {
        let mut reversed = book.clone();
        reversed.quotes.reverse();
        let plan = smart_route(&book.parent(), &book.quotes()).unwrap();
        prop_assert_eq!(&plan, &smart_route(&reversed.parent(), &reversed.quotes()).unwrap());
        let (gated, _) = smart_route_with_capabilities(&book.parent(), &book.quotes(), &unique_capabilities(&book)).unwrap();
        let (gated_reversed, _) = smart_route_with_capabilities(&reversed.parent(), &reversed.quotes(), &unique_capabilities(&reversed)).unwrap();
        prop_assert_eq!(gated, gated_reversed);
        let (expected, unallocated) = book.expected(false);
        prop_assert_eq!(children(&plan), expected);
        prop_assert_eq!(plan.unallocated_quantity.scaled(), unallocated);
    }

    #[test]
    fn permissive_gated_routing_equals_plain_routing(book in book()) {
        let plain = smart_route(&book.parent(), &book.quotes()).unwrap();
        let (gated, _) = smart_route_with_capabilities(&book.parent(), &book.quotes(), &book.capabilities(false)).unwrap();
        prop_assert_eq!(gated, plain);
    }

    #[test]
    fn malformed_routing_inputs_are_refused(book in book(), which in 0usize..5) {
        let parent = book.parent();
        let quotes = book.quotes();
        let mut capabilities = book.capabilities(true);
        let refused = match which {
            // A quoted venue with no capability record.
            0 => {
                capabilities.pop();
                smart_route_with_capabilities(&parent, &quotes, &capabilities).is_err()
            }
            // Two capability records for one venue.
            1 => {
                capabilities.push(capabilities[0].clone());
                smart_route_with_capabilities(&parent, &quotes, &capabilities).is_err()
            }
            // A venue that cannot take the parent's order kind.
            // A venue that cannot take the limit order every routed child is,
            // whatever the parent's kind.
            2 => {
                capabilities[0].supported_order_kinds.remove(&ChildOrderKind::Limit);
                smart_route_with_capabilities(&parent, &quotes, &capabilities).is_err()
            }
            // A quote with no size, to either router.
            3 => {
                let mut broken = quotes.clone();
                broken[0].available_quantity = Decimal::ZERO;
                smart_route(&parent, &broken).is_err()
                    && smart_route_with_capabilities(&parent, &broken, &capabilities).is_err()
            }
            // No quotes at all.
            _ => smart_route(&parent, &[]).is_err()
                && smart_route_with_capabilities(&parent, &[], &capabilities).is_err(),
        };
        prop_assert!(refused, "malformed routing input {} was routed", which);
    }
}

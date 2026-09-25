//! Property-based coverage for `detect_quote_gaps` and `repair_quote_gaps`
//! (E3.5).
//!
//! Each case generates a true, gap-free provider stream per instrument, then
//! records only some of it. The recovery batch is a subset of the dropped
//! quotes plus some recorded ones. Because the true stream is known, the
//! oracle is exact:
//! - gaps are the dropped sequences strictly between an instrument's first and
//!   last recorded sequence;
//! - the repaired stream is exactly the recorded quotes plus the recovered
//!   ones, each a record of the true stream, so nothing is invented;
//! - residual gaps are the dropped interior sequences the batch did not supply;
//! - input order and identical re-deliveries do not change the result;
//! - repairing the repaired stream again recovers nothing new;
//! - a conflicting, out-of-range, identity-reusing, or time-contradicting
//!   recovery record refuses the whole repair, as does a recording holding two
//!   different quotes for one sequence or one identity for two;
//! - detection agrees with what `FeedQualityMonitor` reports as sequence gaps.

use std::collections::BTreeSet;

use follon_domain::{Decimal, DECIMAL_SCALE};
use follon_market_data::{
    detect_quote_gaps, repair_quote_gaps, FeedQualityMonitor, FeedQualityPolicy, FeedStatus, Quote,
    SequenceRange,
};
use proptest::prelude::*;

const INSTRUMENTS: [&str; 3] = ["inst.a", "inst.b", "inst.c"];

fn timestamp(offset_seconds: u32) -> String {
    let minutes = offset_seconds / 60;
    format!(
        "2026-01-02T{:02}:{:02}:{:02}Z",
        14 + minutes / 60,
        minutes % 60,
        offset_seconds % 60
    )
}

#[derive(Clone, Debug)]
struct Point {
    time: u32,
    bid_cents: i128,
    spread_cents: i128,
    recorded: bool,
    recovered: bool,
    /// For a recorded quote, whether the batch also carries it.
    corroborated: bool,
}

#[derive(Clone, Debug)]
struct Stream {
    first_sequence: u64,
    points: Vec<Point>,
}

impl Stream {
    fn quote(&self, instrument: usize, index: usize) -> Quote {
        let point = &self.points[index];
        let sequence = self.first_sequence + index as u64;
        Quote {
            event_time: timestamp(point.time),
            received_at: timestamp(point.time + 1),
            instrument_id: INSTRUMENTS[instrument].to_owned(),
            quote_id: format!("quote.i{instrument}.s{sequence}"),
            source_sequence: sequence,
            bid_price: Decimal::from_scaled(point.bid_cents * DECIMAL_SCALE / 100),
            bid_quantity: Decimal::from_integer(10).unwrap(),
            ask_price: Decimal::from_scaled(
                (point.bid_cents + point.spread_cents) * DECIMAL_SCALE / 100,
            ),
            ask_quantity: Decimal::from_integer(5).unwrap(),
        }
    }

    /// Indices strictly between the first and last recorded point.
    fn interior(&self) -> std::ops::Range<usize> {
        let recorded: Vec<usize> = (0..self.points.len())
            .filter(|index| self.points[*index].recorded)
            .collect();
        match (recorded.first(), recorded.last()) {
            (Some(first), Some(last)) => first + 1..*last,
            _ => 0..0,
        }
    }
}

#[derive(Clone, Debug)]
struct Case {
    streams: Vec<Stream>,
}

impl Case {
    fn recorded(&self) -> Vec<Quote> {
        self.select(|stream, index| stream.points[index].recorded)
    }

    /// Recovered dropped quotes inside a gap, plus corroborated recorded ones.
    fn recovery(&self) -> Vec<Quote> {
        self.select(|stream, index| {
            let point = &stream.points[index];
            (point.recorded && point.corroborated)
                || (!point.recorded && point.recovered && stream.interior().contains(&index))
        })
    }

    fn select(&self, keep: impl Fn(&Stream, usize) -> bool) -> Vec<Quote> {
        let mut quotes = Vec::new();
        for (instrument, stream) in self.streams.iter().enumerate() {
            for index in 0..stream.points.len() {
                if keep(stream, index) {
                    quotes.push(stream.quote(instrument, index));
                }
            }
        }
        quotes
    }

    /// Runs of interior dropped sequences matching `filter`.
    fn ranges(&self, filter: impl Fn(&Point) -> bool) -> Vec<SequenceRange> {
        let mut ranges: Vec<SequenceRange> = Vec::new();
        for (instrument, stream) in self.streams.iter().enumerate() {
            let mut previous: Option<u64> = None;
            for index in stream.interior() {
                let point = &stream.points[index];
                let sequence = stream.first_sequence + index as u64;
                if point.recorded || !filter(point) {
                    previous = None;
                    continue;
                }
                match (previous, ranges.last_mut()) {
                    (Some(prior), Some(last)) if prior + 1 == sequence => last.to = sequence,
                    _ => ranges.push(SequenceRange {
                        instrument_id: INSTRUMENTS[instrument].to_owned(),
                        from: sequence,
                        to: sequence,
                    }),
                }
                previous = Some(sequence);
            }
        }
        ranges
    }

    fn gaps(&self) -> Vec<SequenceRange> {
        self.ranges(|_| true)
    }

    fn recovered(&self) -> Vec<SequenceRange> {
        self.ranges(|point| point.recovered)
    }

    fn residual(&self) -> Vec<SequenceRange> {
        self.ranges(|point| !point.recovered)
    }

    /// The true records the repaired stream must hold, in canonical order.
    fn repaired(&self) -> Vec<Quote> {
        self.select(|stream, index| {
            let point = &stream.points[index];
            point.recorded || (point.recovered && stream.interior().contains(&index))
        })
    }
}

fn stream() -> impl Strategy<Value = Stream> {
    (
        // Mostly small and overlapping across instruments, so instrument
        // boundaries are exercised; sometimes near the top of the range.
        prop_oneof![4 => 1u64..=40, 1 => u64::MAX / 2..=u64::MAX - 64],
        prop::collection::vec(
            (
                0u32..=2,
                1i128..=10_000,
                0i128..=10,
                prop::bool::weighted(0.6),
                any::<bool>(),
                prop::bool::weighted(0.2),
            ),
            1..30,
        ),
    )
        .prop_map(|(first_sequence, raw)| {
            let mut time = 0;
            let points = raw
                .into_iter()
                .map(
                    |(step, bid_cents, spread_cents, recorded, recovered, corroborated)| {
                        time += step;
                        Point {
                            time,
                            bid_cents,
                            spread_cents,
                            recorded,
                            recovered,
                            corroborated,
                        }
                    },
                )
                .collect();
            Stream {
                first_sequence,
                points,
            }
        })
}

fn case() -> impl Strategy<Value = Case> {
    prop::collection::vec(stream(), 1..=3)
        .prop_map(|streams| Case { streams })
        .prop_filter("something must be recorded", |case| {
            !case.recorded().is_empty()
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn repair_matches_the_true_stream_oracle(case in case()) {
        let repair = repair_quote_gaps(&case.recorded(), &case.recovery()).unwrap();
        prop_assert_eq!(&repair.gaps, &case.gaps());
        prop_assert_eq!(&repair.recovered, &case.recovered());
        prop_assert_eq!(&repair.residual, &case.residual());
        prop_assert_eq!(&repair.quotes, &case.repaired());
        prop_assert_eq!(repair.is_complete(), case.residual().is_empty());
        let corroborated = case.recovery().iter().filter(|quote| case.recorded().contains(quote)).count();
        prop_assert_eq!(repair.corroborated, corroborated);
        prop_assert_eq!(repair.recorded_redeliveries, 0);
        prop_assert_eq!(detect_quote_gaps(&case.recorded()).unwrap(), case.gaps());
    }

    #[test]
    fn input_order_and_redeliveries_do_not_change_the_repair(case in case(), redeliver in any::<prop::sample::Index>()) {
        let expected = repair_quote_gaps(&case.recorded(), &case.recovery()).unwrap();
        let mut recorded = case.recorded();
        recorded.reverse();
        let again = recorded[redeliver.index(recorded.len())].clone();
        recorded.push(again);
        let mut recovery = case.recovery();
        recovery.reverse();
        let actual = repair_quote_gaps(&recorded, &recovery).unwrap();
        prop_assert_eq!(actual.recorded_redeliveries, 1);
        prop_assert_eq!(&actual.quotes, &expected.quotes);
        prop_assert_eq!(&actual.gaps, &expected.gaps);
        prop_assert_eq!(&actual.recovered, &expected.recovered);
        prop_assert_eq!(&actual.residual, &expected.residual);
        prop_assert_eq!(actual.corroborated, expected.corroborated);
    }

    #[test]
    fn repairing_a_repaired_stream_again_recovers_nothing_new(case in case()) {
        let first = repair_quote_gaps(&case.recorded(), &case.recovery()).unwrap();
        let second = repair_quote_gaps(&first.quotes, &case.recovery()).unwrap();
        prop_assert_eq!(&second.quotes, &first.quotes);
        prop_assert_eq!(&second.gaps, &first.residual);
        prop_assert!(second.recovered.is_empty());
        prop_assert_eq!(&second.residual, &first.residual);
        prop_assert_eq!(second.corroborated, case.recovery().len());
    }

    #[test]
    fn a_full_recovery_restores_the_true_interior(case in case()) {
        let mut full = case.clone();
        for stream in &mut full.streams {
            for point in &mut stream.points {
                point.recovered = true;
            }
        }
        let repair = repair_quote_gaps(&full.recorded(), &full.recovery()).unwrap();
        prop_assert!(repair.is_complete());
        prop_assert_eq!(&repair.recovered, &full.gaps());
        // Every sequence from each instrument's first to last recorded one.
        let expected = full.select(|stream, index| {
            let interior = stream.interior();
            stream.points[index].recorded || interior.contains(&index)
        });
        prop_assert_eq!(repair.quotes, expected);
    }

    #[test]
    fn a_hostile_recovery_record_refuses_the_whole_repair(
        case in case(),
        kind in 0usize..6,
        pick in any::<prop::sample::Index>(),
    ) {
        let recorded = case.recorded();
        let mut recovery = case.recovery();
        let target = recorded[pick.index(recorded.len())].clone();
        let hostile = match kind {
            // A different record for a recorded sequence.
            0 => {
                recovery.retain(|quote| *quote != target);
                let mut altered = target;
                altered.bid_quantity = altered.bid_quantity.checked_add(Decimal::from_integer(1).unwrap()).unwrap();
                altered
            }
            // Just past an instrument's last recorded sequence, or (half the
            // time) just before its first.
            1 => {
                let sequences = recorded
                    .iter()
                    .filter(|quote| quote.instrument_id == target.instrument_id)
                    .map(|quote| quote.source_sequence);
                let (first, last) = (sequences.clone().min().unwrap(), sequences.max().unwrap());
                let sequence = if pick.index(2) == 0 {
                    last + 1
                } else {
                    // Sequence 0 is invalid anyway, so it would not test the bound.
                    prop_assume!(first > 1);
                    first - 1
                };
                let mut outside = target;
                outside.source_sequence = sequence;
                outside.quote_id = format!("quote.outside.{sequence}");
                outside
            }
            // An instrument the recording never saw.
            2 => {
                let mut unseen = target;
                unseen.instrument_id = "inst.unseen".to_owned();
                unseen.quote_id = "quote.unseen".to_owned();
                unseen
            }
            // 3: a gap filled under the identity of the recorded quote below
            // it. 4 and 5: a gap filled with a quote earlier than that
            // neighbour, or later than every recorded quote. The neighbour is
            // copied, so each case trips only its own check.
            _ => {
                let gaps = case.gaps();
                prop_assume!(!gaps.is_empty());
                let gap = gaps[pick.index(gaps.len())].clone();
                recovery.retain(|quote| {
                    (quote.instrument_id.as_str(), quote.source_sequence)
                        != (gap.instrument_id.as_str(), gap.from)
                });
                let mut filler = recorded
                    .iter()
                    .find(|quote| {
                        quote.instrument_id == gap.instrument_id && quote.source_sequence + 1 == gap.from
                    })
                    .unwrap()
                    .clone();
                filler.source_sequence = gap.from;
                let time = match kind {
                    3 => None,
                    4 => Some("2026-01-01T00:00:00Z"),
                    _ => Some("2026-01-02T23:59:59Z"),
                };
                if let Some(time) = time {
                    filler.quote_id = format!("quote.misordered.{}", gap.from);
                    filler.event_time = time.to_owned();
                    filler.received_at = time.to_owned();
                }
                filler
            }
        };
        recovery.push(hostile);
        prop_assert!(
            repair_quote_gaps(&recorded, &recovery).is_err(),
            "hostile record kind {} was accepted", kind
        );
    }

    #[test]
    fn a_recording_that_contradicts_itself_is_refused(
        case in case(),
        reuse_identity in any::<bool>(),
        pick in any::<prop::sample::Index>(),
    ) {
        let mut recorded = case.recorded();
        let target = recorded[pick.index(recorded.len())].clone();
        let mut hostile = target.clone();
        if reuse_identity {
            // The same quote identity at a sequence nothing else holds.
            let last = recorded
                .iter()
                .filter(|quote| quote.instrument_id == target.instrument_id)
                .map(|quote| quote.source_sequence)
                .max()
                .unwrap();
            hostile.source_sequence = last + 1;
        } else {
            // A different record for the same sequence.
            hostile.ask_quantity = hostile.ask_quantity.checked_add(Decimal::from_integer(1).unwrap()).unwrap();
        }
        recorded.push(hostile);
        prop_assert!(detect_quote_gaps(&recorded).is_err());
        prop_assert!(repair_quote_gaps(&recorded, &case.recovery()).is_err());
    }

    #[test]
    fn detection_agrees_with_the_feed_quality_monitor(case in case(), swaps in prop::collection::vec(any::<prop::sample::Index>(), 0..6)) {
        // Arrival order: canonical, with some later quotes arriving late, but
        // each instrument's first recorded quote arriving first.
        let mut arrival = case.recorded();
        let firsts: BTreeSet<usize> = (0..arrival.len())
            .filter(|index| *index == 0 || arrival[index - 1].instrument_id != arrival[*index].instrument_id)
            .collect();
        for swap in swaps {
            let index = swap.index(arrival.len());
            if index + 1 < arrival.len()
                && !firsts.contains(&index)
                && !firsts.contains(&(index + 1))
            {
                arrival.swap(index, index + 1);
            }
        }
        let policy = FeedQualityPolicy {
            maximum_transport_delay_milliseconds: 1_000_000,
            maximum_staleness_seconds: 1_000_000,
        };
        let mut monitor = FeedQualityMonitor::default();
        let mut reported: BTreeSet<(String, u64)> = BTreeSet::new();
        for quote in &arrival {
            let snapshot = monitor.observe(quote, "2026-01-03T00:00:00Z", &policy).unwrap();
            if snapshot.status == FeedStatus::SequenceGap {
                for sequence in snapshot.missing_sequence_from.unwrap()..=snapshot.missing_sequence_to.unwrap() {
                    reported.insert((quote.instrument_id.clone(), sequence));
                }
            }
        }
        for quote in &arrival {
            reported.remove(&(quote.instrument_id.clone(), quote.source_sequence));
        }
        let detected: BTreeSet<(String, u64)> = detect_quote_gaps(&arrival)
            .unwrap()
            .into_iter()
            .flat_map(|gap| (gap.from..=gap.to).map(move |sequence| (gap.instrument_id.clone(), sequence)))
            .collect();
        prop_assert_eq!(reported, detected);
    }
}

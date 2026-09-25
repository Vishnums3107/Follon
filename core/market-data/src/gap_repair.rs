//! Deterministic quote-stream gap repair from an explicit recovery batch.
//!
//! A recorded quote stream can miss provider sequences, which
//! [`FeedQualityMonitor`](crate::FeedQualityMonitor) reports as
//! [`FeedStatus::SequenceGap`](crate::FeedStatus::SequenceGap). Repair fills a
//! missing sequence only with a record from a recovery batch the caller
//! supplies, such as a vendor replay of the gap window. It never interpolates,
//! and it never changes or drops a recorded quote. A sequence the batch does
//! not supply stays declared as residual, so a consumer can refuse an
//! incomplete stream instead of trading on it.
//!
//! A gap is a missing sequence strictly between two recorded sequences of one
//! instrument. Nothing before an instrument's first or after its last recorded
//! sequence is knowable from the recording, so repair refuses to extend a
//! stream rather than guess where it should have started or ended.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use follon_domain::Decimal;

use crate::{parse_utc, MarketDataError, Quote};

/// The normalized v1 quote CSV contract used by recording and repair.
pub const QUOTE_CSV_HEADER: &str = "event_time,received_at,instrument_id,quote_id,source_sequence,bid_price,bid_quantity,ask_price,ask_quantity";

/// An inclusive run of provider sequences for one instrument.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SequenceRange {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// First sequence in the run.
    pub from: u64,
    /// Last sequence in the run.
    pub to: u64,
}

/// The auditable result of one repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuoteGapRepair {
    /// Every recorded quote plus every recovered one, by instrument then sequence.
    pub quotes: Vec<Quote>,
    /// Gaps in the recorded stream before repair.
    pub gaps: Vec<SequenceRange>,
    /// Runs of sequences filled from the recovery batch.
    pub recovered: Vec<SequenceRange>,
    /// Gaps that remain after repair.
    pub residual: Vec<SequenceRange>,
    /// Identical re-deliveries collapsed in the recorded stream.
    pub recorded_redeliveries: usize,
    /// Recovery records identical to a recorded quote, which confirm it.
    pub corroborated: usize,
}

impl QuoteGapRepair {
    /// True when no gap remains.
    pub fn is_complete(&self) -> bool {
        self.residual.is_empty()
    }
}

type QuoteKey = (String, u64);

/// Indexes quotes by `(instrument, sequence)`, collapsing identical
/// re-deliveries and refusing two different records for one sequence or one
/// quote identity reused for two sequences.
fn index_quotes(
    quotes: &[Quote],
    label: &str,
) -> Result<(BTreeMap<QuoteKey, Quote>, usize), MarketDataError> {
    let mut by_key: BTreeMap<QuoteKey, Quote> = BTreeMap::new();
    let mut by_id: BTreeMap<&str, QuoteKey> = BTreeMap::new();
    let mut redeliveries = 0;
    for quote in quotes {
        quote.validate()?;
        let key = (quote.instrument_id.clone(), quote.source_sequence);
        if let Some(existing) = by_key.get(&key) {
            if existing != quote {
                return Err(MarketDataError(format!(
                    "{label} holds two different quotes for {} sequence {}",
                    quote.instrument_id, quote.source_sequence
                )));
            }
            redeliveries += 1;
            continue;
        }
        if by_id.insert(&quote.quote_id, key.clone()).is_some() {
            return Err(MarketDataError(format!(
                "{label} reuses quote identity {} for two sequences",
                quote.quote_id
            )));
        }
        by_key.insert(key, quote.clone());
    }
    Ok((by_key, redeliveries))
}

/// Groups sorted sequences of each instrument into inclusive runs.
fn runs<'a>(keys: impl IntoIterator<Item = &'a QuoteKey>) -> Vec<SequenceRange> {
    let mut ranges: Vec<SequenceRange> = Vec::new();
    for (instrument_id, sequence) in keys {
        match ranges.last_mut() {
            Some(last)
                if last.instrument_id == *instrument_id
                    && last.to.checked_add(1) == Some(*sequence) =>
            {
                last.to = *sequence;
            }
            _ => ranges.push(SequenceRange {
                instrument_id: instrument_id.clone(),
                from: *sequence,
                to: *sequence,
            }),
        }
    }
    ranges
}

/// The missing sequences strictly between consecutive recorded ones.
fn gaps_of<'a>(keys: impl IntoIterator<Item = &'a QuoteKey>) -> Vec<SequenceRange> {
    let mut gaps = Vec::new();
    let mut previous: Option<&QuoteKey> = None;
    for key in keys {
        if let Some((instrument_id, sequence)) = previous {
            if *instrument_id == key.0 && key.1 > sequence + 1 {
                gaps.push(SequenceRange {
                    instrument_id: instrument_id.clone(),
                    from: sequence + 1,
                    to: key.1 - 1,
                });
            }
        }
        previous = Some(key);
    }
    gaps
}

/// Returns every gap in a recorded quote stream, whatever order it arrived in.
pub fn detect_quote_gaps(recorded: &[Quote]) -> Result<Vec<SequenceRange>, MarketDataError> {
    let (recorded, _) = index_quotes(recorded, "recorded stream")?;
    Ok(gaps_of(recorded.keys()))
}

/// Fills recorded gaps from a recovery batch and declares what remains.
///
/// The whole repair is refused, with nothing merged, when the batch holds a
/// record that differs from a recorded quote at the same sequence, reuses a
/// recorded quote identity for another sequence, lies outside every gap, or
/// has an event time that contradicts the order of its merged neighbours. A
/// recovery record identical to a recorded quote corroborates it.
pub fn repair_quote_gaps(
    recorded: &[Quote],
    recovery: &[Quote],
) -> Result<QuoteGapRepair, MarketDataError> {
    let (mut merged, recorded_redeliveries) = index_quotes(recorded, "recorded stream")?;
    if merged.is_empty() {
        return Err(MarketDataError(
            "gap repair requires a recorded stream".to_owned(),
        ));
    }
    let (recovery, _) = index_quotes(recovery, "recovery batch")?;
    let recorded_ids: BTreeMap<String, QuoteKey> = merged
        .iter()
        .map(|(key, quote)| (quote.quote_id.clone(), key.clone()))
        .collect();
    let mut bounds: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    for (instrument_id, sequence) in merged.keys() {
        bounds
            .entry(instrument_id.as_str())
            .and_modify(|(_, last)| *last = *sequence)
            .or_insert((*sequence, *sequence));
    }
    let gaps = gaps_of(merged.keys());

    let mut corroborated = 0;
    let mut accepted = BTreeMap::new();
    for (key, quote) in recovery {
        if let Some(existing) = merged.get(&key) {
            if *existing != quote {
                return Err(MarketDataError(format!(
                    "recovery quote {} conflicts with the recorded quote for {} sequence {}",
                    quote.quote_id, key.0, key.1
                )));
            }
            corroborated += 1;
            continue;
        }
        if recorded_ids.contains_key(&quote.quote_id) {
            return Err(MarketDataError(format!(
                "recovery quote {} reuses a recorded quote identity for another sequence",
                quote.quote_id
            )));
        }
        match bounds.get(key.0.as_str()) {
            Some((first, last)) if *first < key.1 && key.1 < *last => {}
            _ => {
                return Err(MarketDataError(format!(
                    "recovery quote {} lies outside every recorded gap",
                    quote.quote_id
                )))
            }
        }
        accepted.insert(key, quote);
    }
    let recovered = runs(accepted.keys());
    let recovered_keys: BTreeSet<QuoteKey> = accepted.keys().cloned().collect();
    merged.append(&mut accepted);

    // A recovered quote must sit between its merged neighbours in event time.
    let mut previous: Option<(&QuoteKey, &Quote)> = None;
    for (key, quote) in &merged {
        if let Some((previous_key, previous_quote)) = previous {
            if previous_key.0 == key.0
                && (recovered_keys.contains(key) || recovered_keys.contains(previous_key))
                && parse_utc(&quote.event_time)? < parse_utc(&previous_quote.event_time)?
            {
                let recovered = if recovered_keys.contains(key) {
                    quote
                } else {
                    previous_quote
                };
                return Err(MarketDataError(format!(
                    "recovery quote {} contradicts the recorded event-time order",
                    recovered.quote_id
                )));
            }
        }
        previous = Some((key, quote));
    }

    let residual = gaps_of(merged.keys());
    Ok(QuoteGapRepair {
        quotes: merged.into_values().collect(),
        gaps,
        recovered,
        residual,
        recorded_redeliveries,
        corroborated,
    })
}

/// Imports the normalized v1 quote CSV contract.
pub fn import_quotes(csv: &str) -> Result<Vec<Quote>, MarketDataError> {
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| MarketDataError("quote CSV is empty".to_owned()))?;
    if header.trim_start_matches('\u{feff}').trim_end_matches('\r') != QUOTE_CSV_HEADER {
        return Err(MarketDataError(
            "quote CSV header does not match v1 contract".to_owned(),
        ));
    }
    let mut quotes = Vec::new();
    for (index, line) in lines.enumerate() {
        let row = index + 2;
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() != 9 {
            return Err(MarketDataError(format!("invalid quote row {row}")));
        }
        let decimal = |field: &str, name: &str| {
            Decimal::from_str(field)
                .map_err(|error| MarketDataError(format!("invalid {name} on row {row}: {error}")))
        };
        let quote = Quote {
            event_time: fields[0].to_owned(),
            received_at: fields[1].to_owned(),
            instrument_id: fields[2].to_owned(),
            quote_id: fields[3].to_owned(),
            source_sequence: fields[4]
                .parse()
                .map_err(|_| MarketDataError(format!("invalid source sequence on row {row}")))?,
            bid_price: decimal(fields[5], "bid price")?,
            bid_quantity: decimal(fields[6], "bid quantity")?,
            ask_price: decimal(fields[7], "ask price")?,
            ask_quantity: decimal(fields[8], "ask quantity")?,
        };
        quote.validate()?;
        quotes.push(quote);
    }
    Ok(quotes)
}

/// Renders quotes in the v1 CSV contract, in the order given.
pub fn quotes_to_csv(quotes: &[Quote]) -> String {
    let mut output = format!("{QUOTE_CSV_HEADER}\n");
    for quote in quotes {
        output.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            quote.event_time,
            quote.received_at,
            quote.instrument_id,
            quote.quote_id,
            quote.source_sequence,
            quote.bid_price,
            quote.bid_quantity,
            quote.ask_price,
            quote.ask_quantity,
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(instrument: &str, sequence: u64, second: u32) -> Quote {
        Quote {
            event_time: format!("2026-01-02T14:30:{second:02}Z"),
            received_at: format!("2026-01-02T14:30:{second:02}Z"),
            instrument_id: instrument.to_owned(),
            quote_id: format!("quote.{}.{sequence}", instrument.replace('.', "-")),
            source_sequence: sequence,
            bid_price: Decimal::from_integer(100).unwrap(),
            bid_quantity: Decimal::from_integer(10).unwrap(),
            ask_price: Decimal::from_integer(101).unwrap(),
            ask_quantity: Decimal::from_integer(12).unwrap(),
        }
    }

    fn range(instrument: &str, from: u64, to: u64) -> SequenceRange {
        SequenceRange {
            instrument_id: instrument.to_owned(),
            from,
            to,
        }
    }

    const SPY: &str = "inst.spy";

    #[test]
    fn repair_fills_only_what_the_batch_supplies_and_declares_the_rest() {
        let recorded = [quote(SPY, 1, 1), quote(SPY, 5, 5), quote(SPY, 8, 8)];
        let recovery = [quote(SPY, 2, 2), quote(SPY, 3, 3), quote(SPY, 7, 7)];
        let repair = repair_quote_gaps(&recorded, &recovery).unwrap();
        assert_eq!(repair.gaps, vec![range(SPY, 2, 4), range(SPY, 6, 7)]);
        assert_eq!(repair.recovered, vec![range(SPY, 2, 3), range(SPY, 7, 7)]);
        assert_eq!(repair.residual, vec![range(SPY, 4, 4), range(SPY, 6, 6)]);
        assert!(!repair.is_complete());
        let sequences: Vec<u64> = repair.quotes.iter().map(|q| q.source_sequence).collect();
        assert_eq!(sequences, vec![1, 2, 3, 5, 7, 8]);
    }

    #[test]
    fn a_recovery_record_that_differs_from_the_recording_refuses_the_repair() {
        let recorded = [quote(SPY, 1, 1), quote(SPY, 3, 3)];
        let mut altered = quote(SPY, 3, 3);
        altered.bid_price = Decimal::from_integer(99).unwrap();
        let error = repair_quote_gaps(&recorded, &[quote(SPY, 2, 2), altered]).unwrap_err();
        assert!(
            error.0.contains("conflicts with the recorded quote"),
            "{error}"
        );
        // An identical record corroborates instead.
        let repair = repair_quote_gaps(&recorded, &[quote(SPY, 2, 2), quote(SPY, 3, 3)]).unwrap();
        assert_eq!(repair.corroborated, 1);
        assert!(repair.is_complete());
    }

    #[test]
    fn repair_never_extends_a_stream_past_its_recorded_ends() {
        let recorded = [quote(SPY, 5, 5), quote(SPY, 7, 7)];
        for outside in [quote(SPY, 4, 4), quote(SPY, 8, 8), quote("inst.qqq", 6, 6)] {
            let error = repair_quote_gaps(&recorded, &[outside]).unwrap_err();
            assert!(error.0.contains("outside every recorded gap"), "{error}");
        }
    }

    #[test]
    fn a_recovered_quote_out_of_event_time_order_is_refused() {
        let recorded = [quote(SPY, 1, 10), quote(SPY, 3, 20)];
        for second in [9, 21] {
            let error = repair_quote_gaps(&recorded, &[quote(SPY, 2, second)]).unwrap_err();
            assert!(error.0.contains("event-time order"), "{error}");
        }
        assert!(repair_quote_gaps(&recorded, &[quote(SPY, 2, 10)]).is_ok());
    }

    #[test]
    fn quote_identities_must_stay_unique_across_the_recording_and_the_batch() {
        let recorded = [quote(SPY, 1, 1), quote(SPY, 3, 3)];
        let mut reused = quote(SPY, 2, 2);
        reused.quote_id = recorded[0].quote_id.clone();
        let error = repair_quote_gaps(&recorded, &[reused.clone()]).unwrap_err();
        assert!(
            error.0.contains("reuses a recorded quote identity"),
            "{error}"
        );
        let error = detect_quote_gaps(&[recorded[0].clone(), reused]).unwrap_err();
        assert!(error.0.contains("reuses quote identity"), "{error}");
    }

    #[test]
    fn the_v1_quote_csv_round_trips_exactly() {
        let quotes = vec![quote(SPY, 1, 1), quote("inst.qqq", 4, 2)];
        let csv = quotes_to_csv(&quotes);
        assert_eq!(import_quotes(&csv).unwrap(), quotes);
        assert!(import_quotes("event_time,instrument_id\n").is_err());
        assert!(import_quotes(&csv.replace(",4,", ",x,")).is_err());
    }
}

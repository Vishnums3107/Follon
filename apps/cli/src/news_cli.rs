//! Operator CLI for local-fixture news scoring and deterministic replay output.

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use follon_news::nlp::NlpSentimentEngine;
use follon_news::{
    ingest_local_headlines_ndjson, NewsHeadline, NewsReplayItem, NewsSource, ReplayNewsFeed,
};

fn print_usage() {
    eprintln!(
        r#"follon-news — local-fixture news scoring & deterministic replay harness

USAGE:
    follon-news <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    score <HEADLINE> [--source <SOURCE>] [--ticker <TICKER>]
        Score raw headline text using Follon's deterministic keyword baseline.

    replay <INPUT_NDJSON> [--output <OUTPUT_NDJSON>]
        Replay a chronological stream of news headlines and derive sentiment vectors.

    help
        Print this help message.
"#
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return Ok(());
    }

    match args[0].as_str() {
        "score" => handle_score(&args[1..]),
        "replay" => handle_replay(&args[1..]),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        unknown => {
            eprintln!("Error: unknown subcommand '{}'", unknown);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn handle_score(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Error: 'score' requires a headline text argument.");
        eprintln!("Example: follon-news score \"Apple Reports Record Q3 Earnings Beat and Raises Forecast\"");
        std::process::exit(1);
    }

    let headline_text = &args[0];
    let mut source_str = "DOW_JONES";
    let mut ticker_str = "aapl.us";

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--source" && i + 1 < args.len() {
            source_str = &args[i + 1];
            i += 2;
        } else if args[i] == "--ticker" && i + 1 < args.len() {
            ticker_str = &args[i + 1];
            i += 2;
        } else {
            i += 1;
        }
    }

    let source = NewsSource::parse(source_str).map_err(|e| e.0)?;
    let headline = NewsHeadline {
        news_id: "cli.news.001".to_owned(),
        source,
        headline: headline_text.clone(),
        raw_body_hash: "0".repeat(64),
        sequence_number: 1,
        event_time_ns: 1_788_271_200_000_000_000,
        receive_time_ns: 1_788_271_200_050_000_000,
        entity_tickers: vec![ticker_str.to_owned()],
    };

    let engine = NlpSentimentEngine::new();
    let vectors = engine
        .extract_sentiment_vectors(&headline)
        .map_err(|e| e.0)?;

    println!("============================================================");
    println!("FOLLON LOCAL FIXTURE SENTIMENT REPORT");
    println!("============================================================");
    println!("Headline       : \"{}\"", headline_text);
    println!("Source         : {}", source.as_str());
    println!("Instrument     : {}", ticker_str);
    println!("------------------------------------------------------------");

    if let Some(sentiment) = vectors.first() {
        println!("Taxonomy       : {}", sentiment.taxonomy.as_str());
        println!(
            "Polarity BPS   : {:+} BPS",
            sentiment.sentiment_polarity_bps
        );
        println!("Confidence BPS : {} BPS", sentiment.confidence_bps);
        println!("Novelty BPS    : {} BPS", sentiment.novelty_score_bps);
        println!(
            "Surprise Mag   : {:+} BPS",
            sentiment.surprise_magnitude_bps
        );
        println!("Event Time     : {} ns", sentiment.event_time_ns);
    } else {
        println!("No sentiment vectors extracted.");
    }
    println!("============================================================");

    Ok(())
}

fn handle_replay(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Error: 'replay' requires an input NDJSON file path.");
        eprintln!("Example: follon-news replay var/news/2026-09-01-headlines.ndjson");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[0]);
    let output_path = if args.len() >= 3 && args[1] == "--output" {
        Some(PathBuf::from(&args[2]))
    } else {
        None
    };

    if !input_path.exists() {
        eprintln!("Error: input file '{:?}' does not exist.", input_path);
        std::process::exit(1);
    }

    let headlines = ingest_local_headlines_ndjson(&fs::read_to_string(&input_path)?)?;
    let headline_count = headlines.len();
    let engine = NlpSentimentEngine::new();
    let feed = ReplayNewsFeed::from_headlines(headlines, &engine)?;

    println!("Loaded {} headlines into ReplayNewsFeed.", headline_count);
    println!("Processing chronological news stream...\n");

    let mut out_file = if let Some(ref p) = output_path {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        Some(File::create(p)?)
    } else {
        None
    };

    let mut processed = 0;
    for item in feed.events() {
        if let NewsReplayItem::Sentiment(sentiment) = item {
            processed += 1;
            let json_line = serde_json::to_string(&serde_json::json!({
                "event_id": sentiment.event_id,
                "causation_news_id": sentiment.causation_news_id,
                "instrument_id": sentiment.instrument_id,
                "taxonomy": sentiment.taxonomy.as_str(),
                "sentiment_polarity_bps": sentiment.sentiment_polarity_bps,
                "confidence_bps": sentiment.confidence_bps,
                "novelty_score_bps": sentiment.novelty_score_bps,
                "surprise_magnitude_bps": sentiment.surprise_magnitude_bps,
                "event_time_ns": sentiment.event_time_ns,
            }))?;

            if let Some(ref mut f) = out_file {
                writeln!(f, "{}", json_line)?;
            }

            println!(
                "[{:03}] [{:18}] {:<8} {:+5} BPS (conf: {:4} BPS)",
                processed,
                sentiment.taxonomy.as_str(),
                sentiment.instrument_id,
                sentiment.sentiment_polarity_bps,
                sentiment.confidence_bps,
            );
        }
    }

    println!("\nReplay finished: {} events processed.", processed);
    if let Some(p) = output_path {
        println!("Sentiment vectors written to {:?}", p);
    }

    Ok(())
}

//! The shared controlled-LIVE configuration parser (E3.3c).
//!
//! `follon-live-status` and the trading API's kill-switch route both open a
//! journal through `LiveConfiguration`. A journal opens only under the exact
//! configuration fingerprint it was written with, so these tests hold the
//! parser to the checked-in journal as well as to the document's rules.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_domain::Decimal;
use follon_live::{
    LiveBrokerAccountSnapshot, LiveBrokerAdapter, LiveBrokerEvent, LiveBrokerOrderRequest,
    LiveBrokerSubmitResult, LiveConfiguration, LiveError, LiveKillSwitchScope, LiveTradingService,
};
use follon_secrets::SecretMaterial;
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

/// Refuses every broker operation: these tests never reach a broker.
struct OfflineAdapter;

impl LiveBrokerAdapter for OfflineAdapter {
    fn connect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
        Err(LiveError("offline".to_owned()))
    }
    fn submit(&mut self, _: &LiveBrokerOrderRequest) -> Result<LiveBrokerSubmitResult, LiveError> {
        Err(LiveError("offline".to_owned()))
    }
    fn cancel(&mut self, _: &str) -> Result<(), LiveError> {
        Err(LiveError("offline".to_owned()))
    }
    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
        Err(LiveError("offline".to_owned()))
    }
    fn snapshot(&mut self, _: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
        Err(LiveError("offline".to_owned()))
    }
    fn reconnect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
        Err(LiveError("offline".to_owned()))
    }
}

#[test]
fn the_checked_in_journal_opens_under_its_parsed_configuration() {
    let bytes = fs::read(fixture("config/live-v1.json")).unwrap();
    let configuration = LiveConfiguration::from_json(&bytes).unwrap();
    assert_eq!(
        configuration.content_hash,
        format!("{:x}", Sha256::digest(&bytes))
    );

    // Opened through a copy: a test must never be able to modify the fixture.
    let scratch = std::env::temp_dir().join(format!(
        "follon-live-configuration-check-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();
    let journal = scratch.join("journal-v1.ndjson");
    fs::copy(fixture("live/journal-v1.ndjson"), &journal).unwrap();
    let recorded: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&journal)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();

    let service = LiveTradingService::open_durable(
        configuration.account,
        configuration.risk,
        configuration.activation,
        configuration.kill_switches,
        OfflineAdapter,
        &journal,
        "2026-08-11T13:30:00Z",
    )
    .unwrap();
    assert_eq!(
        service.configuration_fingerprint(),
        recorded["state"]["configuration_fingerprint"]
    );
    drop(service);
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn a_portfolio_risk_configuration_composes_real_limits() {
    let configuration = LiveConfiguration::from_json(
        &fs::read(fixture("config/live-v1-portfolio-risk.json")).unwrap(),
    )
    .unwrap();
    let composition = configuration
        .risk
        .portfolio_risk
        .expect("fixture declares a portfolio_risk block");
    assert_eq!(composition.policy.max_gross_exposure, decimal("5000"));
    assert_eq!(composition.policy.max_concentration_bps, decimal("6000"));
    // Real and operator-configurable (Slice 2: durable peak-equity
    // tracking makes drawdown a genuine computed ratio).
    assert_eq!(composition.policy.max_drawdown_bps, decimal("3000"));
    // Real and operator-configurable (Slice 2b: durable session-start
    // equity baseline makes daily P&L a genuine computed figure).
    assert_eq!(composition.policy.max_daily_loss, decimal("500"));
    // Real and operator-configurable (Slice 2c: margin_rates below makes
    // margin utilization a genuine computed ratio).
    assert_eq!(
        composition.policy.max_margin_utilization_bps,
        decimal("4000")
    );
    let margin_rates = composition
        .margin_rates
        .as_ref()
        .expect("fixture declares margin_rates");
    let equity_rate = margin_rates.get("equity").expect("equity margin rate");
    assert_eq!(equity_rate.initial_bps, 5000);
    assert_eq!(equity_rate.maintenance_bps, 2500);
    assert_eq!(composition.policy.max_abs_delta, Decimal::ZERO);
    assert_eq!(composition.policy.max_abs_gamma, Decimal::ZERO);
    // Live's own dedicated checks already cover these; the composed
    // kernel's copies must stay permanently permissive.
    assert_eq!(composition.policy.max_open_orders, usize::MAX);
    assert_eq!(composition.policy.max_order_rate, u32::MAX);
    // Real and operator-configurable (Slice 2d: durable per-strategy
    // attribution makes strategy exposure a genuine computed figure).
    assert_eq!(
        composition.policy.strategy_limits.get("strategy.live.001"),
        Some(&decimal("3000"))
    );
    assert_eq!(
        composition.policy.sector_limits.get("index"),
        Some(&decimal("2500"))
    );
    let bucket = composition
        .instrument_buckets
        .get("inst.us_equity.spy")
        .expect("fixture declares a bucket for the SPY instrument");
    assert_eq!(bucket.asset_class, "equity");
    assert_eq!(bucket.currency, "USD");
    assert_eq!(bucket.sector, "index");
    composition.policy.validate().unwrap();
    // Permitting net shorts with real capital is not a configuration option.
    assert!(configuration.risk.short_exposure.is_none());
}

#[test]
fn the_document_is_strict() {
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("config/live-v1.json")).unwrap()).unwrap();
    let refuse = |adjust: fn(&mut serde_json::Value)| {
        let mut changed = document.clone();
        adjust(&mut changed);
        LiveConfiguration::from_json(&serde_json::to_vec(&changed).unwrap())
            .unwrap_err()
            .0
    };
    assert!(refuse(|document| document["unexpected"] = 1.into())
        .starts_with("invalid live configuration: unknown field `unexpected`"));
    assert_eq!(
        refuse(|document| document["schema_version"] = 2.into()),
        "unsupported live configuration schema version"
    );
    assert_eq!(
        refuse(|document| document["activation"]["mode"] = "PAPER".into()),
        "live activation mode must be SHADOW or CANARY"
    );
    assert!(LiveConfiguration::from_json(b"").is_err());
    assert!(LiveConfiguration::from_json(&vec![b' '; 1024 * 1024 + 1]).is_err());
}

#[test]
fn a_kill_switch_scope_round_trips_through_its_key() {
    for scope in [
        LiveKillSwitchScope::Global,
        LiveKillSwitchScope::Account("acct.live.001".to_owned()),
        LiveKillSwitchScope::Strategy("strategy.live.001".to_owned()),
        LiveKillSwitchScope::Instrument("inst.us_equity.spy".to_owned()),
    ] {
        assert_eq!(
            LiveKillSwitchScope::from_key(&scope.as_key()).unwrap(),
            scope
        );
    }
    for key in [
        "everything",
        "Global",
        "account:",
        "instrument:Not Canonical",
    ] {
        assert!(
            LiveKillSwitchScope::from_key(key).is_err(),
            "accepted {key}"
        );
    }
}

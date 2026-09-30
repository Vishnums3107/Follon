//! Journal creation, exclusivity, integrity and recovery.

use super::*;

fn modern_registry(account: &PaperAccount, venue_id: &str) -> PaperBrokerRegistry {
    let mut registry = PaperBrokerRegistry::new();
    registry
        .register(
            PaperBrokerRoute {
                account_id: account.account_id.clone(),
                adapter_id: format!("adapter.ibkr.paper.{}", account.account_id),
                venue_id: venue_id.to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(IbkrPaperAdapter::new(account).unwrap()),
        )
        .unwrap();
    registry
}

#[test]
fn a_refused_configuration_leaves_no_paper_journal() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{}.ndjson",
        std::process::id(),
        "refused-configuration"
    ));
    let _ = fs::remove_file(&journal_path);
    let open = |policy: PaperRiskPolicy| {
        PaperTradingService::open_durable(
            account(),
            policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
            &journal_path,
        )
    };
    let mut unpaired = policy();
    unpaired.instrument_lot_sizes.insert(
        "inst.us_equity.iwm".to_owned(),
        decimal("lot", "1").unwrap(),
    );
    assert!(open(unpaired).is_err());
    assert!(
        !journal_path.exists(),
        "a refused configuration created a journal"
    );

    // The same path opens, and journals, once the configuration is valid.
    drop(open(policy()).unwrap());
    assert!(journal_path.exists());
    fs::remove_file(&journal_path).unwrap();
}

#[test]
fn a_legacy_route_refusal_leaves_no_paper_journal() {
    let directory = std::env::temp_dir().join(format!(
        "follon-paper-legacy-refusal-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    let journal_path = directory.join("journal.ndjson");
    let open = || {
        let mut registry = PaperBrokerRegistry::new();
        registry
            .register_legacy_ibkr_paper_route(&account())
            .unwrap();
        PaperTradingService::open_durable(
            account(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            registry,
            &journal_path,
        )
    };
    let refusal = "legacy PAPER adapter routing may only reopen an existing journal";

    // Legacy routing may only reopen, so it creates neither the journal
    // nor its directory.
    assert_eq!(open().err().unwrap().0, refusal);
    assert!(
        !directory.exists(),
        "a legacy-route refusal created the journal's directory"
    );

    // Nor does it create the journal in a directory that exists.
    fs::create_dir_all(&directory).unwrap();
    assert_eq!(open().err().unwrap().0, refusal);
    assert!(
        !journal_path.exists(),
        "a legacy-route refusal created a journal"
    );

    // An empty journal that already existed is refused and left as it was.
    fs::write(&journal_path, b"").unwrap();
    assert_eq!(open().err().unwrap().0, refusal);
    assert_eq!(fs::read(&journal_path).unwrap(), b"");

    // A journal the single-adapter composition initialised still reopens.
    fs::remove_file(&journal_path).unwrap();
    drop(
        PaperTradingService::open_durable(
            account(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
            &journal_path,
        )
        .unwrap(),
    );
    drop(open().unwrap());
    fs::remove_dir_all(&directory).unwrap();
}

/// Links `link` to `target`, which need not exist. Returns false where
/// this account cannot create a symbolic link, such as Windows without
/// Developer Mode, after saying so.
fn symlink_to(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(target, link);
    if let Err(error) = &linked {
        eprintln!("cannot create a symbolic link ({error}); the refusal was not exercised");
    }
    linked.is_ok()
}

#[test]
fn a_paper_journal_refuses_a_symbolic_link_even_a_dangling_one() {
    let directory =
        std::env::temp_dir().join(format!("follon-paper-journal-link-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let target = directory.join("elsewhere.ndjson");
    let link = directory.join("journal.ndjson");
    if !symlink_to(&target, &link) {
        fs::remove_dir_all(&directory).unwrap();
        return;
    }
    let refusal = "paper journal path must not be a symbolic link";

    // Nothing exists at the target, so following the link finds nothing.
    // Opening must not create the journal there.
    assert_eq!(FilePaperJournal::open(&link).err().unwrap().0, refusal);
    assert!(!target.exists(), "the journal was created through the link");
    assert_eq!(
        PaperTradingService::open_durable(
            account(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
            &link,
        )
        .err()
        .unwrap()
        .0,
        refusal
    );
    assert!(!target.exists(), "the journal was created through the link");

    // A link to a real journal is refused too.
    drop(FilePaperJournal::open(&target).unwrap());
    assert_eq!(FilePaperJournal::open(&link).err().unwrap().0, refusal);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn legacy_single_account_journal_reopens_through_registry_without_rewriting_evidence() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-registry-migration-{}.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let paper_account = account();
    let mut legacy_service = PaperTradingService::open_durable(
        paper_account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();
    legacy_service
        .activate_kill_switch(KillSwitchScope::Global)
        .unwrap();
    drop(legacy_service);

    let mut registry = PaperBrokerRegistry::new();
    registry
        .register_legacy_ibkr_paper_route(&paper_account)
        .unwrap();
    let recovered = PaperTradingService::open_durable(
        paper_account,
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        registry,
        &journal_path,
    )
    .unwrap();
    assert_eq!(
        recovered.kill_switches().active_keys(),
        vec!["global".to_owned()]
    );
    drop(recovered);
    let _ = fs::remove_file(&journal_path);
}

#[test]
fn legacy_registry_refuses_empty_journals_and_modern_route_changes_fail_recovery() {
    let legacy_path = std::env::temp_dir().join(format!(
        "follon-paper-legacy-empty-{}.ndjson",
        std::process::id()
    ));
    let modern_path = std::env::temp_dir().join(format!(
        "follon-paper-route-mismatch-{}.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&legacy_path);
    let _ = fs::remove_file(&modern_path);
    let paper_account = account();

    let mut legacy_registry = PaperBrokerRegistry::new();
    legacy_registry
        .register_legacy_ibkr_paper_route(&paper_account)
        .unwrap();
    assert!(PaperTradingService::open_durable(
        paper_account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        legacy_registry,
        &legacy_path,
    )
    .is_err());

    let mut modern_service = PaperTradingService::open_durable(
        paper_account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        modern_registry(&paper_account, "venue.ibkr.paper"),
        &modern_path,
    )
    .unwrap();
    modern_service
        .activate_kill_switch(KillSwitchScope::Global)
        .unwrap();
    drop(modern_service);
    assert!(PaperTradingService::open_durable(
        paper_account,
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        modern_registry(&account(), "venue.ibkr.paper.changed"),
        &modern_path,
    )
    .is_err());
    let _ = fs::remove_file(&legacy_path);
    let _ = fs::remove_file(&modern_path);
}

#[test]
fn paper_journal_allows_exactly_one_open_operator_process() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{}.ndjson",
        std::process::id(),
        "exclusive-lock"
    ));
    let _ = fs::remove_file(&journal_path);
    let first = FilePaperJournal::open(&journal_path).unwrap();
    assert!(FilePaperJournal::open(&journal_path).is_err());
    drop(first);
    let reopened = FilePaperJournal::open(&journal_path).unwrap();
    drop(reopened);
    fs::remove_file(journal_path).unwrap();
}

#[test]
fn paper_journal_refuses_a_tampered_hash_chain() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-tampered.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let paper_account = account();
    let service = PaperTradingService::open_durable(
        paper_account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .expect("durable service");
    drop(service);
    let original = fs::read_to_string(&journal_path).expect("read journal");
    let tampered = original.replacen("100000.00000000", "100001.00000000", 1);
    assert_ne!(tampered, original);
    fs::write(&journal_path, tampered).expect("tamper journal");

    assert!(PaperTradingService::open_durable(
        paper_account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .is_err());
    fs::remove_file(journal_path).unwrap();
}

#[test]
fn durable_journal_recovers_unknown_orders_and_30_clean_day_gate_is_measured() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{}.ndjson",
        std::process::id(),
        "recovery"
    ));
    let _ = fs::remove_file(&journal_path);
    let account = account();
    let adapter = IbkrPaperAdapter::new(&account).unwrap();
    let mut faulted = FaultInjectingBroker::new(adapter);
    faulted.inject(BrokerOperation::Submit, BrokerFault::Disconnect);
    let mut durable_service = PaperTradingService::open_durable(
        account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        faulted,
        &journal_path,
    )
    .unwrap();
    assert!(durable_service
        .submit_intent(
            intent("intent-paper-004", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .is_err());
    drop(durable_service);

    let recovered = PaperTradingService::open_durable(
        account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .unwrap();
    assert_eq!(
        recovered.order("order-intent-paper-004").unwrap().oms.state,
        OrderState::Unknown
    );
    drop(recovered);
    let mut incompatible_policy = policy();
    incompatible_policy.max_order_notional = decimal("notional", "40000").unwrap();
    assert!(PaperTradingService::open_durable(
        account.clone(),
        incompatible_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .is_err());
    let gate_journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{}.ndjson",
        std::process::id(),
        "thirty-day-gate"
    ));
    let _ = fs::remove_file(&gate_journal_path);
    let gate_account = account.clone();
    let mut gate_service = PaperTradingService::open_durable(
        gate_account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&gate_account).unwrap(),
        &gate_journal_path,
    )
    .unwrap();
    let dates = [
        "2026-03-02",
        "2026-03-03",
        "2026-03-04",
        "2026-03-05",
        "2026-03-06",
        "2026-03-09",
        "2026-03-10",
        "2026-03-11",
        "2026-03-12",
        "2026-03-13",
        "2026-03-16",
        "2026-03-17",
        "2026-03-18",
        "2026-03-19",
        "2026-03-20",
        "2026-03-23",
        "2026-03-24",
        "2026-03-25",
        "2026-03-26",
        "2026-03-27",
        "2026-03-30",
        "2026-03-31",
        "2026-04-01",
        "2026-04-02",
        "2026-04-06",
        "2026-04-07",
        "2026-04-08",
        "2026-04-09",
        "2026-04-10",
        "2026-04-13",
    ];
    let gate_sessions: Vec<_> = dates.iter().map(|date| paper_session(date)).collect();
    let gate_calendar = paper_calendar(&gate_sessions);
    for (date, session) in dates.into_iter().zip(&gate_sessions) {
        let report = gate_service
            .reconcile(&format!("{date}T21:00:00Z"))
            .unwrap();
        assert!(report.is_clean());
        gate_service
            .record_paper_session(session, &report, &gate_calendar)
            .unwrap();
    }
    assert!(gate_service.promotion_status().eligible_for_next_gate);
    drop(gate_service);
    fs::remove_file(gate_journal_path).unwrap();
    fs::remove_file(journal_path).unwrap();
}

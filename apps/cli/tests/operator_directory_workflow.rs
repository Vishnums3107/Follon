//! End-to-end validation for `follon-admin operator-add`: the directory it
//! writes is exactly what the trading API loads, the printed enrolment URI is
//! the only copy of the second factor it shows, and a refused addition
//! leaves the directory untouched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use follon_identity::{decode_base32, totp_code, LoginOutcome, OperatorDirectory, Permission};

const PASSWORD: &str = "Correct-Horse-9-Battery";

fn workspace() -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("follon-operator-directory-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn operator_add(directory: &Path, password_file: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_follon-admin"))
        .arg("operator-add")
        .args(["--directory", directory.to_str().unwrap()])
        .args(["--password-file", password_file.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("follon-admin should start")
}

fn totp_secret(output: &Output) -> Vec<u8> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let uri = stdout
        .lines()
        .find_map(|line| line.strip_prefix("totp_uri="))
        .expect("the enrolment URI is printed");
    let secret = uri
        .split(['?', '&'])
        .find_map(|part| part.strip_prefix("secret="))
        .expect("the URI carries the secret");
    decode_base32(secret).unwrap()
}

#[test]
fn provisioned_operators_log_in_with_the_printed_second_factor() {
    let root = workspace();
    let directory = root.join("operators.json");
    let password_file = root.join("password.txt");
    fs::write(&password_file, format!("{PASSWORD}\n")).unwrap();
    let identity = |user: &str, email: &str, roles: &str| {
        vec![
            "--tenant-id".to_owned(),
            "tenant.alpha".to_owned(),
            "--user-id".to_owned(),
            user.to_owned(),
            "--email".to_owned(),
            email.to_owned(),
            "--roles".to_owned(),
            roles.to_owned(),
        ]
    };
    let run = |arguments: Vec<String>| {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        operator_add(&directory, &password_file, &arguments)
    };

    let trader = run(identity("user.trader", "trader@example.com", "trader"));
    assert!(
        trader.status.success(),
        "{}",
        String::from_utf8_lossy(&trader.stderr)
    );
    let risk = run(identity(
        "user.risk",
        "risk@example.com",
        "risk_manager,auditor",
    ));
    assert!(
        risk.status.success(),
        "{}",
        String::from_utf8_lossy(&risk.stderr)
    );

    let written = fs::read_to_string(&directory).unwrap();
    assert!(!written.contains(PASSWORD), "only the hash is stored");
    let parsed = OperatorDirectory::parse(&written).unwrap();
    assert_eq!(parsed.tenant_id, "tenant.alpha");
    assert_eq!(parsed.operators.len(), 2);
    assert_eq!(parsed.operators[1].roles, ["auditor", "risk_manager"]);

    // Every refusal leaves the directory byte for byte as it was.
    let weak = root.join("weak.txt");
    fs::write(&weak, "short\n").unwrap();
    let mut other_tenant = identity("user.other", "other@example.com", "trader");
    other_tenant[1] = "tenant.beta".to_owned();
    let refusals = [
        run(identity("user.trader", "again@example.com", "trader")),
        run(identity("user.new", "TRADER@example.com", "trader")),
        run(identity("user.new", "new@example.com", "superuser")),
        run(other_tenant),
        {
            let arguments = identity("user.weak", "weak@example.com", "trader");
            let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
            operator_add(&directory, &weak, &arguments)
        },
    ];
    for (index, refused) in refusals.iter().enumerate() {
        assert!(!refused.status.success(), "refusal {index} succeeded");
        assert!(
            !String::from_utf8_lossy(&refused.stdout).contains("totp_uri="),
            "refusal {index} printed a secret"
        );
        assert_eq!(
            fs::read_to_string(&directory).unwrap(),
            written,
            "refusal {index}"
        );
    }

    // The printed second factor logs the trader in and nothing else does.
    let mut service = parsed.identity_service().unwrap();
    let now = 1_800_000_000;
    let LoginOutcome::MfaRequired {
        challenge_token, ..
    } = service
        .begin_login("tenant.alpha", "trader@example.com", PASSWORD, now)
        .unwrap()
    else {
        panic!("a provisioned operator must need a second factor");
    };
    let wrong = totp_code(&totp_secret(&risk), now).unwrap();
    assert!(service
        .complete_totp(&challenge_token, &wrong, now)
        .is_err());
    let session = service
        .complete_totp(
            &challenge_token,
            &totp_code(&totp_secret(&trader), now).unwrap(),
            now,
        )
        .unwrap();
    assert!(service
        .authorize(&session.token, "tenant.alpha", Permission::PaperTrade, now)
        .is_ok());
    fs::remove_dir_all(&root).unwrap();
}

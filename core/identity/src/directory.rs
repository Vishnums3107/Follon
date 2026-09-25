//! The provisioned operator directory a server loads at startup.
//!
//! A directory binds one tenant to its operators. Each operator carries an
//! Argon2id password hash and a TOTP secret, so a server holds no plaintext
//! password and every operator must pass a second factor. The TOTP secret is
//! live credential material: the file must be readable only by the service
//! account, like any other secret file.
//!
//! One directory serves one tenant. A server that loads it can therefore
//! authorize an operator only for that tenant, which is what keeps a PAPER
//! route owned by one tenant out of reach of another tenant's operators.

use std::collections::BTreeSet;

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::{
    hash_new_password, normalize_email, validate_argon2id_hash, IdentityError, IdentityService,
    Role,
};
use follon_domain::validate_canonical_id;

/// Current operator directory contract version.
pub const OPERATOR_DIRECTORY_SCHEMA_VERSION: u32 = 1;

/// One tenant's provisioned operators.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorDirectory {
    /// Contract version; must equal [`OPERATOR_DIRECTORY_SCHEMA_VERSION`].
    pub directory_schema_version: u32,
    /// The single tenant every operator belongs to.
    pub tenant_id: String,
    /// Provisioned operators, in the order they were added.
    pub operators: Vec<OperatorRecord>,
}

/// One provisioned operator. `Debug` is not derived, so a log line can never
/// print the hash or the TOTP secret.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorRecord {
    /// Canonical user identity, recorded as the submitting operator.
    pub user_id: String,
    /// Login email, unique within the tenant.
    pub email: String,
    /// Argon2id PHC password hash.
    pub password_hash: String,
    /// RFC 4648 base32 TOTP secret of at least 160 bits.
    pub totp_secret_base32: String,
    /// Stable role names; see [`Role::name`].
    pub roles: Vec<String>,
}

impl std::fmt::Debug for OperatorRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperatorRecord")
            .field("user_id", &self.user_id)
            .field("roles", &self.roles)
            .finish_non_exhaustive()
    }
}

impl OperatorDirectory {
    /// An empty directory for one tenant.
    pub fn new(tenant_id: impl Into<String>) -> Result<Self, IdentityError> {
        let tenant_id = tenant_id.into();
        validate_canonical_id("tenant_id", &tenant_id).map_err(|error| IdentityError(error.0))?;
        Ok(Self {
            directory_schema_version: OPERATOR_DIRECTORY_SCHEMA_VERSION,
            tenant_id,
            operators: Vec::new(),
        })
    }

    /// Parses and fully validates a directory document.
    pub fn parse(document: &str) -> Result<Self, IdentityError> {
        let directory: Self = serde_json::from_str(document)
            .map_err(|error| IdentityError(format!("invalid operator directory: {error}")))?;
        directory.validate()?;
        Ok(directory)
    }

    /// Refuses an unknown version, a duplicate identity or email, a hash that
    /// is not Argon2id, a missing or short TOTP secret, or an unknown role.
    pub fn validate(&self) -> Result<(), IdentityError> {
        if self.directory_schema_version != OPERATOR_DIRECTORY_SCHEMA_VERSION {
            return Err(IdentityError(
                "unsupported operator directory schema version".to_owned(),
            ));
        }
        validate_canonical_id("tenant_id", &self.tenant_id)
            .map_err(|error| IdentityError(error.0))?;
        if self.operators.is_empty() {
            return Err(IdentityError(
                "operator directory must list at least one operator".to_owned(),
            ));
        }
        let mut users = BTreeSet::new();
        let mut emails = BTreeSet::new();
        for operator in &self.operators {
            validate_canonical_id("user_id", &operator.user_id)
                .map_err(|error| IdentityError(error.0))?;
            if !users.insert(operator.user_id.as_str())
                || !emails.insert(normalize_email(&operator.email)?)
            {
                return Err(IdentityError(format!(
                    "operator {} duplicates a user identity or email",
                    operator.user_id
                )));
            }
            validate_argon2id_hash(&operator.password_hash)?;
            if decode_base32(&operator.totp_secret_base32)?.len() < 20 {
                return Err(IdentityError(format!(
                    "operator {} needs a TOTP secret of at least 160 bits",
                    operator.user_id
                )));
            }
            roles(operator)?;
        }
        Ok(())
    }

    /// Adds one operator, hashing the password and generating a fresh TOTP
    /// secret, which is returned once for enrolment in an authenticator.
    pub fn add_operator(
        &mut self,
        user_id: &str,
        email: &str,
        password: &str,
        roles: &[Role],
    ) -> Result<Vec<u8>, IdentityError> {
        let mut secret = vec![0_u8; 20];
        OsRng.fill_bytes(&mut secret);
        let mut names: Vec<String> = roles.iter().map(|role| role.name().to_owned()).collect();
        names.sort();
        names.dedup();
        let mut candidate = self.clone();
        candidate.operators.push(OperatorRecord {
            user_id: user_id.to_owned(),
            email: email.trim().to_owned(),
            password_hash: hash_new_password(password)?,
            totp_secret_base32: encode_base32(&secret),
            roles: names,
        });
        candidate.validate()?;
        *self = candidate;
        Ok(secret)
    }

    /// Pretty canonical JSON with a trailing newline.
    pub fn to_json(&self) -> Result<String, IdentityError> {
        serde_json::to_string_pretty(self)
            .map(|json| json + "\n")
            .map_err(|error| IdentityError(format!("cannot serialize directory: {error}")))
    }

    /// Builds the in-memory identity service a server authorizes against.
    pub fn identity_service(&self) -> Result<IdentityService, IdentityError> {
        self.validate()?;
        let mut service = IdentityService::default();
        for operator in &self.operators {
            service.import_user(
                operator.user_id.clone(),
                self.tenant_id.clone(),
                operator.email.clone(),
                &operator.password_hash,
                Some(decode_base32(&operator.totp_secret_base32)?),
                roles(operator)?,
            )?;
        }
        Ok(service)
    }
}

fn roles(operator: &OperatorRecord) -> Result<BTreeSet<Role>, IdentityError> {
    let mut roles = BTreeSet::new();
    for name in &operator.roles {
        let role =
            Role::from_name(name).ok_or_else(|| IdentityError(format!("unknown role {name}")))?;
        if !roles.insert(role) {
            return Err(IdentityError(format!("duplicate role {name}")));
        }
    }
    if roles.is_empty() {
        return Err(IdentityError(format!(
            "operator {} must have at least one role",
            operator.user_id
        )));
    }
    Ok(roles)
}

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32 without padding, as authenticator apps expect.
pub fn encode_base32(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(char::from(
                BASE32_ALPHABET[((buffer >> bits) & 31) as usize],
            ));
        }
    }
    if bits > 0 {
        output.push(char::from(
            BASE32_ALPHABET[((buffer << (5 - bits)) & 31) as usize],
        ));
    }
    output
}

/// Decodes unpadded RFC 4648 base32; any other character, or non-zero
/// trailing bits, is refused.
pub fn decode_base32(text: &str) -> Result<Vec<u8>, IdentityError> {
    let invalid = || IdentityError("invalid base32 TOTP secret".to_owned());
    let mut output = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for character in text.bytes() {
        let value = BASE32_ALPHABET
            .iter()
            .position(|candidate| *candidate == character)
            .ok_or_else(invalid)?;
        buffer = (buffer << 5) | value as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    if bits >= 5 || buffer & ((1 << bits) - 1) != 0 {
        return Err(invalid());
    }
    Ok(output)
}

/// An `otpauth://` enrolment URI for an authenticator app.
pub fn totp_provisioning_uri(issuer: &str, account: &str, secret: &[u8]) -> String {
    let escape = |value: &str| -> String {
        value
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'@') {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    };
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30",
        escape(issuer),
        escape(account),
        encode_base32(secret),
        escape(issuer),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{totp_code, LoginOutcome, Permission};

    const PASSWORD: &str = "Correct-Horse-9-Battery";

    #[test]
    fn base32_round_trips_and_matches_the_rfc_4648_vectors() {
        assert_eq!(encode_base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(decode_base32("MZXW6YTBOI").unwrap(), b"foobar");
        for length in 0..40 {
            let bytes: Vec<u8> = (0..length).map(|value| (value * 37 + 11) as u8).collect();
            assert_eq!(decode_base32(&encode_base32(&bytes)).unwrap(), bytes);
        }
        assert!(
            decode_base32("MZXW6YTBO1").is_err(),
            "digit 1 is not base32"
        );
        assert!(decode_base32("mzxw6ytboi").is_err(), "lowercase is refused");
        assert!(
            decode_base32("MZXW6YTBOJ").is_err(),
            "non-zero trailing bits"
        );
    }

    #[test]
    fn totp_matches_the_rfc_6238_sha1_vector() {
        // RFC 6238 appendix B, SHA-1, T = 59: 94287082, whose low six digits are 287082.
        assert_eq!(totp_code(b"12345678901234567890", 59).unwrap(), "287082");
        assert_eq!(
            totp_code(b"12345678901234567890", 1_111_111_109).unwrap(),
            "081804"
        );
    }

    #[test]
    fn a_provisioned_operator_logs_in_with_password_and_totp_only() {
        let mut directory = OperatorDirectory::new("tenant.alpha").unwrap();
        let secret = directory
            .add_operator(
                "user.trader",
                "Trader@Example.com",
                PASSWORD,
                &[Role::Trader],
            )
            .unwrap();
        let reloaded = OperatorDirectory::parse(&directory.to_json().unwrap()).unwrap();
        assert_eq!(reloaded, directory);
        assert!(!reloaded.to_json().unwrap().contains(PASSWORD));

        let mut service = reloaded.identity_service().unwrap();
        let now = 1_800_000_000;
        let LoginOutcome::MfaRequired {
            challenge_token, ..
        } = service
            .begin_login("tenant.alpha", "trader@example.com", PASSWORD, now)
            .unwrap()
        else {
            panic!("a provisioned operator must always need a second factor");
        };
        let session = service
            .complete_totp(&challenge_token, &totp_code(&secret, now).unwrap(), now)
            .unwrap();
        let context = service
            .authorize(&session.token, "tenant.alpha", Permission::PaperTrade, now)
            .unwrap();
        assert_eq!(context.user_id, "user.trader");
        assert!(service
            .authorize(&session.token, "tenant.beta", Permission::PaperTrade, now)
            .is_err());
        assert!(service
            .authorize(
                &session.token,
                "tenant.alpha",
                Permission::KillSwitchOperate,
                now
            )
            .is_err());
    }

    #[test]
    fn a_directory_refuses_every_unsafe_record() {
        let mut directory = OperatorDirectory::new("tenant.alpha").unwrap();
        directory
            .add_operator("user.one", "one@example.com", PASSWORD, &[Role::Trader])
            .unwrap();
        let valid = directory.clone();
        let record = valid.operators[0].clone();
        let with = |change: &dyn Fn(&mut OperatorRecord)| {
            let mut candidate = valid.clone();
            change(&mut candidate.operators[0]);
            candidate
        };
        let cases = [
            with(&|record| record.password_hash = "plaintext-password".to_owned()),
            with(&|record| {
                record.password_hash =
                    "$argon2i$v=19$m=16,t=2,p=1$c29tZXNhbHQ$Hz/9sPFbDyO9C6D/2wGdYw".to_owned()
            }),
            with(&|record| record.totp_secret_base32 = String::new()),
            with(&|record| record.totp_secret_base32 = encode_base32(&[7; 19])),
            with(&|record| record.roles = vec!["superuser".to_owned()]),
            // An unknown role is refused even beside a valid one.
            with(&|record| record.roles = vec!["trader".to_owned(), "superuser".to_owned()]),
            with(&|record| record.roles = Vec::new()),
            with(&|record| record.roles = vec!["trader".to_owned(), "trader".to_owned()]),
            with(&|record| record.user_id = "User.One".to_owned()),
            {
                let mut candidate = valid.clone();
                candidate.operators.push(record.clone());
                candidate
            },
            {
                let mut candidate = valid.clone();
                let mut twin = record.clone();
                twin.user_id = "user.two".to_owned();
                twin.email = "ONE@example.com".to_owned();
                candidate.operators.push(twin);
                candidate
            },
            {
                let mut candidate = valid.clone();
                candidate.directory_schema_version = 2;
                candidate
            },
            {
                let mut candidate = valid.clone();
                candidate.operators.clear();
                candidate
            },
        ];
        for (index, candidate) in cases.iter().enumerate() {
            assert!(candidate.validate().is_err(), "case {index} was accepted");
            assert!(
                OperatorDirectory::parse(&serde_json::to_string(candidate).unwrap()).is_err(),
                "case {index} parsed"
            );
        }
        assert!(OperatorDirectory::parse(
            &valid
                .to_json()
                .unwrap()
                .replace("\"tenant_id\"", "\"extra\": 1, \"tenant_id\"")
        )
        .is_err());
        // A refused addition leaves the directory unchanged.
        let mut unchanged = valid.clone();
        assert!(unchanged
            .add_operator("user.one", "other@example.com", PASSWORD, &[Role::Trader])
            .is_err());
        assert!(unchanged
            .add_operator("user.weak", "weak@example.com", "short", &[Role::Trader])
            .is_err());
        assert_eq!(unchanged, valid);
    }

    #[test]
    fn the_published_schema_matches_the_serialized_directory() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/json-schema/v1/operator-directory.schema.json"
        ))
        .unwrap();
        let mut directory = OperatorDirectory::new("tenant.alpha").unwrap();
        directory
            .add_operator("user.one", "one@example.com", PASSWORD, &[Role::Trader])
            .unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&directory.to_json().unwrap()).unwrap();
        let keys = |value: &serde_json::Value| -> BTreeSet<String> {
            value.as_object().unwrap().keys().cloned().collect()
        };
        let required = |value: &serde_json::Value| -> BTreeSet<String> {
            value["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key.as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(keys(&written), keys(&schema["properties"]));
        assert_eq!(keys(&written), required(&schema));
        let operator_schema = &schema["properties"]["operators"]["items"];
        assert_eq!(
            keys(&written["operators"][0]),
            keys(&operator_schema["properties"])
        );
        assert_eq!(keys(&written["operators"][0]), required(operator_schema));
        let role_names: BTreeSet<&str> = operator_schema["properties"]["roles"]["items"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        for name in &role_names {
            assert!(Role::from_name(name).is_some(), "{name}");
        }
        assert_eq!(role_names.len(), 5);
    }

    #[test]
    fn provisioning_uri_carries_the_secret_and_escapes_labels() {
        let uri = totp_provisioning_uri("Follon PAPER", "trader@example.com", b"foobar");
        assert_eq!(
            uri,
            "otpauth://totp/Follon%20PAPER:trader@example.com?secret=MZXW6YTBOI&issuer=Follon%20PAPER&algorithm=SHA1&digits=6&period=30"
        );
    }
}

//! Customer identity, MFA, session, tenant-isolation, and RBAC contracts.
//!
//! Password hashes and MFA secrets stay behind this boundary. Browser and
//! desktop clients receive only short-lived opaque tokens whose hashes are
//! retained server-side for immediate revocation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use follon_domain::validate_canonical_id;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha1::Sha1;
use sha2::{Digest, Sha256};

const LOGIN_FAILURE_LIMIT: u8 = 5;
const LOCKOUT_SECONDS: i64 = 15 * 60;
const MFA_CHALLENGE_SECONDS: i64 = 5 * 60;
const MFA_FAILURE_LIMIT: u8 = 5;
const SESSION_SECONDS: i64 = 15 * 60;
const TOTP_STEP_SECONDS: i64 = 30;

/// Identity operation failure. Authentication failures intentionally use one
/// generic message to avoid account enumeration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityError(pub String);

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IdentityError {}

/// Customer roles with deliberately non-overlapping operational intent.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Role {
    /// Manage tenant members, roles, and authentication policy.
    OrganizationAdmin,
    /// Change risk policy and operate kill switches.
    RiskManager,
    /// Create and manage non-administrative orders.
    Trader,
    /// Read dashboards and reports.
    ReadOnly,
    /// Read immutable audit and compliance evidence.
    Auditor,
}

/// Authorization capabilities checked at server-side action boundaries.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Permission {
    /// Read tenant portfolio and execution state.
    PortfolioRead,
    /// Submit and cancel PAPER orders.
    PaperTrade,
    /// Submit and cancel controlled-LIVE orders.
    LiveTrade,
    /// Manage portfolio and order risk policy.
    RiskPolicyManage,
    /// Activate or reset a kill switch.
    KillSwitchOperate,
    /// Manage members, roles, and MFA requirements.
    IdentityManage,
    /// Read immutable audit evidence.
    AuditRead,
    /// Promote a signed release.
    ReleasePromote,
}

impl Role {
    /// Returns whether this role grants one permission.
    pub fn grants(self, permission: Permission) -> bool {
        match self {
            Self::OrganizationAdmin => matches!(
                permission,
                Permission::PortfolioRead
                    | Permission::IdentityManage
                    | Permission::AuditRead
                    | Permission::ReleasePromote
            ),
            Self::RiskManager => matches!(
                permission,
                Permission::PortfolioRead
                    | Permission::RiskPolicyManage
                    | Permission::KillSwitchOperate
                    | Permission::AuditRead
            ),
            Self::Trader => matches!(
                permission,
                Permission::PortfolioRead | Permission::PaperTrade | Permission::LiveTrade
            ),
            Self::ReadOnly => permission == Permission::PortfolioRead,
            Self::Auditor => matches!(
                permission,
                Permission::PortfolioRead | Permission::AuditRead
            ),
        }
    }
}

#[derive(Clone, Debug)]
struct UserRecord {
    user_id: String,
    tenant_id: String,
    password_hash: String,
    mfa_secret: Option<Vec<u8>>,
    recovery_code_hashes: BTreeSet<[u8; 32]>,
    roles: BTreeSet<Role>,
    enabled: bool,
    security_version: u64,
}

#[derive(Clone, Debug)]
struct LoginFailures {
    count: u8,
    locked_until_epoch_seconds: Option<i64>,
}

#[derive(Clone, Debug)]
struct MfaChallengeRecord {
    user_id: String,
    expires_at_epoch_seconds: i64,
    attempts: u8,
}

#[derive(Clone, Debug)]
struct SessionRecord {
    user_id: String,
    tenant_id: String,
    expires_at_epoch_seconds: i64,
    security_version: u64,
}

/// Successful session response. The opaque token is returned once and is not
/// stored in plaintext by the identity service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionGrant {
    /// Bearer token for the HTTP/gRPC authorization header.
    pub token: String,
    /// Absolute expiry time.
    pub expires_at_epoch_seconds: i64,
}

/// Password-login outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoginOutcome {
    /// A second factor is required to issue the session.
    MfaRequired {
        /// Single-purpose opaque challenge token.
        challenge_token: String,
        /// Absolute challenge expiry.
        expires_at_epoch_seconds: i64,
    },
    /// Session issued for a user without enrolled MFA.
    Authenticated(SessionGrant),
}

/// Verified authorization context for downstream tenant-scoped operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationContext {
    /// Authenticated user ID.
    pub user_id: String,
    /// Tenant supplied by the caller and verified against the session.
    pub tenant_id: String,
    /// Current user roles.
    pub roles: BTreeSet<Role>,
}

/// Stateful IAM service suitable for a transactional persistence adapter.
#[derive(Debug, Default)]
pub struct IdentityService {
    users: BTreeMap<String, UserRecord>,
    email_index: BTreeMap<(String, String), String>,
    failures: BTreeMap<String, LoginFailures>,
    challenges: BTreeMap<[u8; 32], MfaChallengeRecord>,
    sessions: BTreeMap<[u8; 32], SessionRecord>,
}

impl IdentityService {
    /// Creates one tenant-scoped customer account with an Argon2id password hash.
    pub fn create_user(
        &mut self,
        user_id: impl Into<String>,
        tenant_id: impl Into<String>,
        email: impl Into<String>,
        password: &str,
        roles: BTreeSet<Role>,
    ) -> Result<(), IdentityError> {
        let user_id = user_id.into();
        let tenant_id = tenant_id.into();
        validate_canonical_id("user_id", &user_id).map_err(|error| IdentityError(error.0))?;
        validate_canonical_id("tenant_id", &tenant_id).map_err(|error| IdentityError(error.0))?;
        let normalized_email = normalize_email(&email.into())?;
        validate_password(password)?;
        if roles.is_empty() {
            return Err(IdentityError("user must have at least one role".to_owned()));
        }
        if self.users.contains_key(&user_id)
            || self
                .email_index
                .contains_key(&(tenant_id.clone(), normalized_email.clone()))
        {
            return Err(IdentityError("user already exists".to_owned()));
        }
        let password_hash = hash_password(password)?;
        self.email_index.insert(
            (tenant_id.clone(), normalized_email.clone()),
            user_id.clone(),
        );
        self.users.insert(
            user_id.clone(),
            UserRecord {
                user_id,
                tenant_id,
                password_hash,
                mfa_secret: None,
                recovery_code_hashes: BTreeSet::new(),
                roles,
                enabled: true,
                security_version: 1,
            },
        );
        Ok(())
    }

    /// Generates and enrolls a new 160-bit TOTP secret. Existing sessions are
    /// invalidated so MFA applies immediately.
    pub fn enroll_totp(&mut self, user_id: &str) -> Result<Vec<u8>, IdentityError> {
        let mut secret = vec![0_u8; 20];
        OsRng.fill_bytes(&mut secret);
        self.set_totp_secret(user_id, secret.clone())?;
        Ok(secret)
    }

    /// Enrolls a caller-provided TOTP secret. This is useful for controlled
    /// migrations and deterministic interoperability tests.
    pub fn set_totp_secret(&mut self, user_id: &str, secret: Vec<u8>) -> Result<(), IdentityError> {
        if secret.len() < 20 {
            return Err(IdentityError(
                "TOTP secret must contain at least 160 bits".to_owned(),
            ));
        }
        let user = self
            .users
            .get_mut(user_id)
            .ok_or_else(|| IdentityError("user not found".to_owned()))?;
        user.mfa_secret = Some(secret);
        user.recovery_code_hashes.clear();
        user.security_version = user
            .security_version
            .checked_add(1)
            .ok_or_else(|| IdentityError("security version overflow".to_owned()))?;
        self.revoke_user_sessions(user_id);
        Ok(())
    }

    /// Verifies tenant, password, account status, and lockout policy before
    /// either issuing a session or a single-purpose MFA challenge.
    pub fn begin_login(
        &mut self,
        tenant_id: &str,
        email: &str,
        password: &str,
        now_epoch_seconds: i64,
    ) -> Result<LoginOutcome, IdentityError> {
        let normalized_email = normalize_email(email)?;
        let Some(user_id) = self
            .email_index
            .get(&(tenant_id.to_owned(), normalized_email))
            .cloned()
        else {
            return Err(authentication_failed());
        };
        if self.is_locked(&user_id, now_epoch_seconds) {
            return Err(authentication_failed());
        }
        let verified = self
            .users
            .get(&user_id)
            .filter(|user| user.enabled)
            .is_some_and(|user| verify_password(password, &user.password_hash));
        if !verified {
            self.record_failure(&user_id, now_epoch_seconds)?;
            return Err(authentication_failed());
        }
        self.failures.remove(&user_id);
        if self
            .users
            .get(&user_id)
            .and_then(|user| user.mfa_secret.as_ref())
            .is_some()
        {
            let challenge_token = random_token();
            let expires_at_epoch_seconds = now_epoch_seconds
                .checked_add(MFA_CHALLENGE_SECONDS)
                .ok_or_else(|| IdentityError("challenge expiry overflow".to_owned()))?;
            self.challenges.insert(
                token_hash(&challenge_token),
                MfaChallengeRecord {
                    user_id,
                    expires_at_epoch_seconds,
                    attempts: 0,
                },
            );
            Ok(LoginOutcome::MfaRequired {
                challenge_token,
                expires_at_epoch_seconds,
            })
        } else {
            Ok(LoginOutcome::Authenticated(
                self.issue_session(&user_id, now_epoch_seconds)?,
            ))
        }
    }

    /// Completes a TOTP challenge with ±1 time-step clock tolerance.
    pub fn complete_totp(
        &mut self,
        challenge_token: &str,
        code: &str,
        now_epoch_seconds: i64,
    ) -> Result<SessionGrant, IdentityError> {
        let challenge_hash = token_hash(challenge_token);
        let (user_id, expires_at, attempts) = self
            .challenges
            .get(&challenge_hash)
            .map(|challenge| {
                (
                    challenge.user_id.clone(),
                    challenge.expires_at_epoch_seconds,
                    challenge.attempts,
                )
            })
            .ok_or_else(authentication_failed)?;
        if now_epoch_seconds > expires_at || attempts >= MFA_FAILURE_LIMIT {
            self.challenges.remove(&challenge_hash);
            return Err(authentication_failed());
        }
        let secret = self
            .users
            .get(&user_id)
            .filter(|user| user.enabled)
            .and_then(|user| user.mfa_secret.as_ref())
            .ok_or_else(authentication_failed)?;
        if !verify_totp(secret, code, now_epoch_seconds) {
            if let Some(challenge) = self.challenges.get_mut(&challenge_hash) {
                challenge.attempts = challenge.attempts.saturating_add(1);
            }
            return Err(authentication_failed());
        }
        self.challenges.remove(&challenge_hash);
        self.issue_session(&user_id, now_epoch_seconds)
    }

    /// Rotates and returns ten one-time MFA recovery codes. Only hashes remain
    /// after this call; the plaintext values cannot be retrieved again.
    pub fn rotate_recovery_codes(&mut self, user_id: &str) -> Result<Vec<String>, IdentityError> {
        let user = self
            .users
            .get_mut(user_id)
            .filter(|user| user.enabled && user.mfa_secret.is_some())
            .ok_or_else(|| IdentityError("MFA enrollment is required".to_owned()))?;
        let mut codes = Vec::with_capacity(10);
        let mut hashes = BTreeSet::new();
        while codes.len() < 10 {
            let mut material = [0_u8; 16];
            OsRng.fill_bytes(&mut material);
            let code = hex(&material);
            if hashes.insert(token_hash(&code)) {
                codes.push(code);
            }
        }
        user.recovery_code_hashes = hashes;
        user.security_version = user
            .security_version
            .checked_add(1)
            .ok_or_else(|| IdentityError("security version overflow".to_owned()))?;
        self.revoke_user_sessions(user_id);
        Ok(codes)
    }

    /// Completes a password-authenticated MFA challenge with one recovery code.
    /// A successful code is atomically consumed before a session is issued.
    pub fn complete_recovery_code(
        &mut self,
        challenge_token: &str,
        recovery_code: &str,
        now_epoch_seconds: i64,
    ) -> Result<SessionGrant, IdentityError> {
        let challenge_hash = token_hash(challenge_token);
        let (user_id, expires_at, attempts) = self
            .challenges
            .get(&challenge_hash)
            .map(|challenge| {
                (
                    challenge.user_id.clone(),
                    challenge.expires_at_epoch_seconds,
                    challenge.attempts,
                )
            })
            .ok_or_else(authentication_failed)?;
        if now_epoch_seconds > expires_at || attempts >= MFA_FAILURE_LIMIT {
            self.challenges.remove(&challenge_hash);
            return Err(authentication_failed());
        }
        let recovery_hash = token_hash(recovery_code);
        let valid = self
            .users
            .get(&user_id)
            .filter(|user| user.enabled)
            .is_some_and(|user| user.recovery_code_hashes.contains(&recovery_hash));
        if !valid {
            if let Some(challenge) = self.challenges.get_mut(&challenge_hash) {
                challenge.attempts = challenge.attempts.saturating_add(1);
            }
            return Err(authentication_failed());
        }
        let user = self
            .users
            .get_mut(&user_id)
            .ok_or_else(authentication_failed)?;
        user.recovery_code_hashes.remove(&recovery_hash);
        self.challenges.remove(&challenge_hash);
        self.issue_session(&user_id, now_epoch_seconds)
    }

    /// Changes a password through a valid tenant session, rehashes it with a
    /// fresh Argon2id salt, and immediately revokes every existing session.
    pub fn change_password(
        &mut self,
        token: &str,
        tenant_id: &str,
        current_password: &str,
        new_password: &str,
        now_epoch_seconds: i64,
    ) -> Result<(), IdentityError> {
        validate_password(new_password)?;
        let session = self
            .sessions
            .get(&token_hash(token))
            .filter(|session| {
                now_epoch_seconds <= session.expires_at_epoch_seconds
                    && session.tenant_id == tenant_id
            })
            .cloned()
            .ok_or_else(|| IdentityError("access denied".to_owned()))?;
        let user = self
            .users
            .get_mut(&session.user_id)
            .filter(|user| {
                user.enabled
                    && user.tenant_id == tenant_id
                    && user.security_version == session.security_version
            })
            .ok_or_else(|| IdentityError("access denied".to_owned()))?;
        if !verify_password(current_password, &user.password_hash)
            || verify_password(new_password, &user.password_hash)
        {
            return Err(authentication_failed());
        }
        user.password_hash = hash_password(new_password)?;
        user.security_version = user
            .security_version
            .checked_add(1)
            .ok_or_else(|| IdentityError("security version overflow".to_owned()))?;
        let user_id = user.user_id.clone();
        self.revoke_user_sessions(&user_id);
        Ok(())
    }

    /// Authorizes a live session, enforces exact tenant isolation, and checks
    /// current roles rather than roles captured at login time.
    pub fn authorize(
        &self,
        token: &str,
        tenant_id: &str,
        permission: Permission,
        now_epoch_seconds: i64,
    ) -> Result<AuthorizationContext, IdentityError> {
        let session = self
            .sessions
            .get(&token_hash(token))
            .filter(|session| {
                now_epoch_seconds <= session.expires_at_epoch_seconds
                    && session.tenant_id == tenant_id
            })
            .ok_or_else(|| IdentityError("access denied".to_owned()))?;
        let user = self
            .users
            .get(&session.user_id)
            .filter(|user| {
                user.enabled
                    && user.tenant_id == tenant_id
                    && user.security_version == session.security_version
            })
            .ok_or_else(|| IdentityError("access denied".to_owned()))?;
        if !user.roles.iter().any(|role| role.grants(permission)) {
            return Err(IdentityError("access denied".to_owned()));
        }
        Ok(AuthorizationContext {
            user_id: user.user_id.clone(),
            tenant_id: user.tenant_id.clone(),
            roles: user.roles.clone(),
        })
    }

    /// Replaces roles and immediately invalidates prior sessions.
    pub fn set_roles(&mut self, user_id: &str, roles: BTreeSet<Role>) -> Result<(), IdentityError> {
        if roles.is_empty() {
            return Err(IdentityError("user must have at least one role".to_owned()));
        }
        let user = self
            .users
            .get_mut(user_id)
            .ok_or_else(|| IdentityError("user not found".to_owned()))?;
        user.roles = roles;
        user.security_version = user
            .security_version
            .checked_add(1)
            .ok_or_else(|| IdentityError("security version overflow".to_owned()))?;
        self.revoke_user_sessions(user_id);
        Ok(())
    }

    /// Disables an account and immediately invalidates sessions and challenges.
    pub fn disable_user(&mut self, user_id: &str) -> Result<(), IdentityError> {
        let user = self
            .users
            .get_mut(user_id)
            .ok_or_else(|| IdentityError("user not found".to_owned()))?;
        user.enabled = false;
        user.security_version = user
            .security_version
            .checked_add(1)
            .ok_or_else(|| IdentityError("security version overflow".to_owned()))?;
        self.revoke_user_sessions(user_id);
        self.challenges
            .retain(|_, challenge| challenge.user_id != user_id);
        Ok(())
    }

    /// Revokes one opaque session.
    pub fn revoke_session(&mut self, token: &str) -> bool {
        self.sessions.remove(&token_hash(token)).is_some()
    }

    fn issue_session(
        &mut self,
        user_id: &str,
        now_epoch_seconds: i64,
    ) -> Result<SessionGrant, IdentityError> {
        let user = self
            .users
            .get(user_id)
            .filter(|user| user.enabled)
            .ok_or_else(authentication_failed)?;
        let token = random_token();
        let expires_at_epoch_seconds = now_epoch_seconds
            .checked_add(SESSION_SECONDS)
            .ok_or_else(|| IdentityError("session expiry overflow".to_owned()))?;
        self.sessions.insert(
            token_hash(&token),
            SessionRecord {
                user_id: user.user_id.clone(),
                tenant_id: user.tenant_id.clone(),
                expires_at_epoch_seconds,
                security_version: user.security_version,
            },
        );
        Ok(SessionGrant {
            token,
            expires_at_epoch_seconds,
        })
    }

    fn is_locked(&mut self, user_id: &str, now_epoch_seconds: i64) -> bool {
        let Some(failures) = self.failures.get(user_id) else {
            return false;
        };
        if failures
            .locked_until_epoch_seconds
            .is_some_and(|until| now_epoch_seconds < until)
        {
            return true;
        }
        if failures.locked_until_epoch_seconds.is_some() {
            self.failures.remove(user_id);
        }
        false
    }

    fn record_failure(
        &mut self,
        user_id: &str,
        now_epoch_seconds: i64,
    ) -> Result<(), IdentityError> {
        let failures = self
            .failures
            .entry(user_id.to_owned())
            .or_insert(LoginFailures {
                count: 0,
                locked_until_epoch_seconds: None,
            });
        failures.count = failures.count.saturating_add(1);
        if failures.count >= LOGIN_FAILURE_LIMIT {
            failures.locked_until_epoch_seconds = Some(
                now_epoch_seconds
                    .checked_add(LOCKOUT_SECONDS)
                    .ok_or_else(|| IdentityError("lockout expiry overflow".to_owned()))?,
            );
        }
        Ok(())
    }

    fn revoke_user_sessions(&mut self, user_id: &str) {
        self.sessions
            .retain(|_, session| session.user_id != user_id);
    }
}

fn normalize_email(email: &str) -> Result<String, IdentityError> {
    let normalized = email.trim().to_ascii_lowercase();
    let mut parts = normalized.split('@');
    if normalized.len() > 254
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_some()
    {
        return Err(IdentityError("invalid email address".to_owned()));
    }
    Ok(normalized)
}

fn validate_password(password: &str) -> Result<(), IdentityError> {
    if password.len() < 12
        || password.len() > 128
        || !password.bytes().any(|byte| byte.is_ascii_lowercase())
        || !password.bytes().any(|byte| byte.is_ascii_uppercase())
        || !password.bytes().any(|byte| byte.is_ascii_digit())
        || !password.bytes().any(|byte| !byte.is_ascii_alphanumeric())
    {
        return Err(IdentityError(
            "password must be 12-128 characters with upper, lower, number, and symbol".to_owned(),
        ));
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, IdentityError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| IdentityError("password hashing failed".to_owned()))
}

fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

fn authentication_failed() -> IdentityError {
    IdentityError("authentication failed".to_owned())
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    result
}

fn totp_code(secret: &[u8], epoch_seconds: i64) -> Result<String, IdentityError> {
    if epoch_seconds < 0 || secret.len() < 20 {
        return Err(authentication_failed());
    }
    let counter =
        u64::try_from(epoch_seconds / TOTP_STEP_SECONDS).map_err(|_| authentication_failed())?;
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).map_err(|_| authentication_failed())?;
    mac.update(&counter.to_be_bytes());
    let output = mac.finalize().into_bytes();
    let offset = usize::from(output[19] & 0x0f);
    let binary = (u32::from(output[offset] & 0x7f) << 24)
        | (u32::from(output[offset + 1]) << 16)
        | (u32::from(output[offset + 2]) << 8)
        | u32::from(output[offset + 3]);
    Ok(format!("{:06}", binary % 1_000_000))
}

fn verify_totp(secret: &[u8], code: &str, epoch_seconds: i64) -> bool {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    [-TOTP_STEP_SECONDS, 0, TOTP_STEP_SECONDS]
        .into_iter()
        .filter_map(|offset| epoch_seconds.checked_add(offset))
        .filter_map(|timestamp| totp_code(secret, timestamp).ok())
        .any(|expected| expected.as_bytes() == code.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(role: Role) -> BTreeSet<Role> {
        BTreeSet::from([role])
    }

    #[test]
    fn password_totp_session_and_rbac_are_enforced() {
        let mut identity = IdentityService::default();
        identity
            .create_user(
                "user.alice",
                "tenant.acme",
                "Alice@Example.COM",
                "Correct-Horse-7!",
                roles(Role::Trader),
            )
            .unwrap();
        let secret = b"12345678901234567890".to_vec();
        identity
            .set_totp_secret("user.alice", secret.clone())
            .unwrap();
        let outcome = identity
            .begin_login(
                "tenant.acme",
                "alice@example.com",
                "Correct-Horse-7!",
                1_000,
            )
            .unwrap();
        let LoginOutcome::MfaRequired {
            challenge_token, ..
        } = outcome
        else {
            panic!("MFA should be required")
        };
        let code = totp_code(&secret, 1_000).unwrap();
        let session = identity
            .complete_totp(&challenge_token, &code, 1_000)
            .unwrap();
        assert!(identity
            .authorize(&session.token, "tenant.acme", Permission::LiveTrade, 1_001,)
            .is_ok());
        assert!(identity
            .authorize(
                &session.token,
                "tenant.acme",
                Permission::RiskPolicyManage,
                1_001,
            )
            .is_err());
        assert!(identity
            .authorize(
                &session.token,
                "tenant.other",
                Permission::PortfolioRead,
                1_001,
            )
            .is_err());
    }

    #[test]
    fn role_change_and_disable_revoke_sessions_immediately() {
        let mut identity = IdentityService::default();
        identity
            .create_user(
                "user.bob",
                "tenant.acme",
                "bob@example.com",
                "Correct-Horse-8!",
                roles(Role::ReadOnly),
            )
            .unwrap();
        let LoginOutcome::Authenticated(session) = identity
            .begin_login("tenant.acme", "bob@example.com", "Correct-Horse-8!", 2_000)
            .unwrap()
        else {
            panic!("session expected")
        };
        identity
            .set_roles("user.bob", roles(Role::OrganizationAdmin))
            .unwrap();
        assert!(identity
            .authorize(
                &session.token,
                "tenant.acme",
                Permission::PortfolioRead,
                2_001,
            )
            .is_err());
        identity.disable_user("user.bob").unwrap();
    }

    #[test]
    fn repeated_password_failures_lock_the_account_without_enumeration() {
        let mut identity = IdentityService::default();
        identity
            .create_user(
                "user.carol",
                "tenant.acme",
                "carol@example.com",
                "Correct-Horse-9!",
                roles(Role::Auditor),
            )
            .unwrap();
        for attempt in 0..LOGIN_FAILURE_LIMIT {
            let error = identity
                .begin_login(
                    "tenant.acme",
                    "carol@example.com",
                    "wrong-password",
                    3_000 + i64::from(attempt),
                )
                .unwrap_err();
            assert_eq!(error, authentication_failed());
        }
        assert_eq!(
            identity
                .begin_login(
                    "tenant.acme",
                    "carol@example.com",
                    "Correct-Horse-9!",
                    3_010,
                )
                .unwrap_err(),
            authentication_failed()
        );
        assert!(identity
            .begin_login(
                "tenant.acme",
                "carol@example.com",
                "Correct-Horse-9!",
                4_000,
            )
            .is_ok());
    }

    #[test]
    fn weak_passwords_and_duplicate_tenant_email_are_rejected() {
        let mut identity = IdentityService::default();
        assert!(identity
            .create_user(
                "user.weak",
                "tenant.acme",
                "weak@example.com",
                "password",
                roles(Role::ReadOnly),
            )
            .is_err());
        identity
            .create_user(
                "user.one",
                "tenant.acme",
                "same@example.com",
                "Correct-Horse-1!",
                roles(Role::ReadOnly),
            )
            .unwrap();
        assert!(identity
            .create_user(
                "user.two",
                "tenant.acme",
                "SAME@example.com",
                "Correct-Horse-2!",
                roles(Role::ReadOnly),
            )
            .is_err());
    }

    #[test]
    fn recovery_codes_are_one_time_and_password_rotation_revokes_sessions() {
        let mut identity = IdentityService::default();
        identity
            .create_user(
                "user.recovery",
                "tenant.acme",
                "recovery@example.com",
                "Correct-Horse-4!",
                roles(Role::Trader),
            )
            .unwrap();
        identity
            .set_totp_secret("user.recovery", b"12345678901234567890".to_vec())
            .unwrap();
        let codes = identity
            .rotate_recovery_codes("user.recovery")
            .expect("recovery codes");
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|code| code.len() == 32));

        let LoginOutcome::MfaRequired {
            challenge_token, ..
        } = identity
            .begin_login(
                "tenant.acme",
                "recovery@example.com",
                "Correct-Horse-4!",
                5_000,
            )
            .unwrap()
        else {
            panic!("MFA challenge expected")
        };
        let session = identity
            .complete_recovery_code(&challenge_token, &codes[0], 5_000)
            .expect("one-time recovery");
        assert!(identity
            .authorize(&session.token, "tenant.acme", Permission::LiveTrade, 5_001,)
            .is_ok());

        let LoginOutcome::MfaRequired {
            challenge_token, ..
        } = identity
            .begin_login(
                "tenant.acme",
                "recovery@example.com",
                "Correct-Horse-4!",
                5_010,
            )
            .unwrap()
        else {
            panic!("MFA challenge expected")
        };
        assert!(identity
            .complete_recovery_code(&challenge_token, &codes[0], 5_010)
            .is_err());

        identity
            .change_password(
                &session.token,
                "tenant.acme",
                "Correct-Horse-4!",
                "Different-Horse-5!",
                5_020,
            )
            .expect("password rotation");
        assert!(identity
            .authorize(&session.token, "tenant.acme", Permission::LiveTrade, 5_021,)
            .is_err());
        assert!(identity
            .begin_login(
                "tenant.acme",
                "recovery@example.com",
                "Correct-Horse-4!",
                5_022,
            )
            .is_err());
        assert!(identity
            .begin_login(
                "tenant.acme",
                "recovery@example.com",
                "Different-Horse-5!",
                5_023,
            )
            .is_ok());
    }
}

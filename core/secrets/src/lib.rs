//! Secret-provider boundary for future adapters and hosted infrastructure.
//!
//! This crate deliberately has no implementation that reads environment
//! variables, files, or broker credentials. Implementations belong at the
//! deployment edge and must be auditable.

use std::fmt;

/// Opaque, validated reference to a secret managed outside the trading core.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SecretReference(String);

impl SecretReference {
    /// Creates a reference such as `secret.broker.ibkr.account-001`.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
        {
            return Err(SecretError(
                "secret reference must be a canonical ID".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    /// Returns the non-sensitive reference used for audit and access policy.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Secret bytes intentionally omit `Debug`, `Clone`, and string conversion.
pub struct SecretMaterial(Vec<u8>);

impl SecretMaterial {
    /// Constructs secret material at a trusted provider boundary.
    pub fn new(bytes: Vec<u8>) -> Result<Self, SecretError> {
        if bytes.is_empty() {
            return Err(SecretError("secret material cannot be empty".to_owned()));
        }
        Ok(Self(bytes))
    }

    /// Provides bytes only to the adapter operation that requires them.
    pub fn expose_to<T>(&self, operation: impl FnOnce(&[u8]) -> T) -> T {
        operation(&self.0)
    }
}

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// Provider errors must be safe to log and must never embed secret material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretError(pub String);

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SecretError {}

/// Deployment-edge interface for managed vaults and operating-system keychains.
pub trait SecretProvider: Send + Sync {
    /// Retrieves a secret without exposing it to strategy code or logs.
    fn resolve(&self, reference: &SecretReference) -> Result<SecretMaterial, SecretError>;
}

#[cfg(test)]
mod tests {
    use super::SecretReference;

    #[test]
    fn secret_reference_never_contains_display_whitespace() {
        assert!(SecretReference::new("secret.broker.ibkr.account-001").is_ok());
        assert!(SecretReference::new("IBKR token").is_err());
    }
}

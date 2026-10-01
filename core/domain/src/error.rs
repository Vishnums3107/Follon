//! Domain error type and canonical identifier/timestamp validation.

use std::fmt;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::DecimalError;

/// Validates one canonical identifier used as a durable domain key.
pub fn validate_canonical_id(name: &str, value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(DomainError(format!(
            "{name} must be a non-empty canonical ID"
        )));
    }
    Ok(())
}

/// A violation of an accepted Follon domain contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainError(pub String);

impl From<DecimalError> for DomainError {
    fn from(error: DecimalError) -> Self {
        Self(error.0)
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DomainError {}

/// Validates the canonical UTC timestamp representation used for deterministic ordering.
///
/// Follon deliberately accepts only second-precision `YYYY-MM-DDTHH:MM:SSZ`
/// values at persistence and replay boundaries. This makes lexical ordering
/// identical to temporal ordering and prevents equivalent instants from
/// acquiring multiple hashes through offsets or fractional formatting.
pub fn validate_utc_timestamp(name: &str, value: &str) -> Result<(), DomainError> {
    let bytes = value.as_bytes();
    let canonical_shape = bytes.len() == 20
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    if !canonical_shape || OffsetDateTime::parse(value, &Rfc3339).is_err() {
        return Err(DomainError(format!(
            "{name} must be canonical second-precision UTC"
        )));
    }
    Ok(())
}

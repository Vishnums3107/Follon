//! Fixed-point decimal arithmetic.

use std::fmt;
use std::str::FromStr;

/// Fixed decimal precision used by quantities and monetary values.
pub const DECIMAL_SCALE: i128 = 100_000_000;

/// An exact decimal represented as a signed integer scaled by [`DECIMAL_SCALE`].
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Decimal(i128);

/// An error returned when a decimal cannot be represented exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalError(pub String);

impl fmt::Display for DecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DecimalError {}

impl Decimal {
    /// The additive identity.
    pub const ZERO: Self = Self(0);

    /// Creates a value from its exact scaled representation.
    pub const fn from_scaled(value: i128) -> Self {
        Self(value)
    }

    /// Creates an exact whole-number value.
    pub fn from_integer(value: i64) -> Result<Self, DecimalError> {
        let scaled = i128::from(value)
            .checked_mul(DECIMAL_SCALE)
            .ok_or_else(|| DecimalError("integer decimal overflow".to_owned()))?;
        Ok(Self(scaled))
    }

    /// Returns the scaled representation for storage and comparison.
    pub const fn scaled(self) -> i128 {
        self.0
    }

    /// Adds two decimals without losing precision.
    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or_else(|| DecimalError("decimal addition overflow".to_owned()))
    }

    /// Subtracts two decimals without losing precision.
    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or_else(|| DecimalError("decimal subtraction overflow".to_owned()))
    }

    /// Multiplies two decimals, retaining the configured fixed precision.
    pub fn checked_mul(self, other: Self) -> Result<Self, DecimalError> {
        self.0
            .checked_mul(other.0)
            .and_then(|value| value.checked_div(DECIMAL_SCALE))
            .map(Self)
            .ok_or_else(|| DecimalError("decimal multiplication overflow".to_owned()))
    }

    /// Divides two decimals, retaining the configured fixed precision.
    pub fn checked_div(self, other: Self) -> Result<Self, DecimalError> {
        if other.0 == 0 {
            return Err(DecimalError("division by zero".to_owned()));
        }
        self.0
            .checked_mul(DECIMAL_SCALE)
            .and_then(|value| value.checked_div(other.0))
            .map(Self)
            .ok_or_else(|| DecimalError("decimal division overflow".to_owned()))
    }
}

/// Returns the absolute distance between a requested price and a positive
/// reference price in exact basis points.
///
/// This shared calculation keeps simulation, PAPER, and controlled-LIVE price
/// collars identical. A non-positive input fails closed instead of producing a
/// misleading risk result.
pub fn price_deviation_bps(
    reference_price: Decimal,
    requested_price: Decimal,
) -> Result<Decimal, DecimalError> {
    if reference_price <= Decimal::ZERO || requested_price <= Decimal::ZERO {
        return Err(DecimalError(
            "price-collar inputs must both be positive".to_owned(),
        ));
    }
    let distance = if requested_price >= reference_price {
        requested_price.checked_sub(reference_price)?
    } else {
        reference_price.checked_sub(requested_price)?
    };
    distance
        .checked_mul(Decimal::from_integer(10_000)?)?
        .checked_div(reference_price)
}

impl FromStr for Decimal {
    type Err = DecimalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let source = value.trim();
        if source.is_empty() {
            return Err(DecimalError("decimal is empty".to_owned()));
        }

        let (negative, unsigned) = match source.as_bytes()[0] {
            b'-' => (true, &source[1..]),
            b'+' => (false, &source[1..]),
            _ => (false, source),
        };
        if unsigned.is_empty() || unsigned.matches('.').count() > 1 {
            return Err(DecimalError(format!("invalid decimal: {source}")));
        }

        let mut parts = unsigned.split('.');
        let whole_text = parts.next().unwrap_or("0");
        let fraction_text = parts.next().unwrap_or("");
        if whole_text.is_empty() && fraction_text.is_empty()
            || !whole_text.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction_text.bytes().all(|byte| byte.is_ascii_digit())
            || fraction_text.len() > 8
        {
            return Err(DecimalError(format!("invalid decimal: {source}")));
        }

        let whole = if whole_text.is_empty() {
            0
        } else {
            whole_text
                .parse::<i128>()
                .map_err(|_| DecimalError(format!("decimal overflow: {source}")))?
        };
        let fraction = if fraction_text.is_empty() {
            0
        } else {
            let mut padded = fraction_text.to_owned();
            while padded.len() < 8 {
                padded.push('0');
            }
            padded
                .parse::<i128>()
                .map_err(|_| DecimalError(format!("decimal overflow: {source}")))?
        };
        let scaled = whole
            .checked_mul(DECIMAL_SCALE)
            .and_then(|whole_scaled| whole_scaled.checked_add(fraction))
            .ok_or_else(|| DecimalError(format!("decimal overflow: {source}")))?;
        Ok(Self(if negative { -scaled } else { scaled }))
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        if value < 0 {
            formatter.write_str("-")?;
        }
        let magnitude = value.unsigned_abs();
        write!(
            formatter,
            "{}.{:08}",
            magnitude / DECIMAL_SCALE as u128,
            magnitude % DECIMAL_SCALE as u128
        )
    }
}

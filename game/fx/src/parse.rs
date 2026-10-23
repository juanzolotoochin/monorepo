use std::fmt;
use std::str::FromStr;

use crate::Fx;

/// The most fractional digits accepted. 10^19 still fits in a `u64`, and 19
/// digits are far beyond the 2^-32 resolution.
const MAX_FRAC_DIGITS: usize = 19;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseFxError {
    Empty,
    InvalidDigit,
    TooManyDecimals,
    OutOfRange,
}

impl fmt::Display for ParseFxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ParseFxError::Empty => "no digits in fixed-point number",
            ParseFxError::InvalidDigit => "invalid character in fixed-point number",
            ParseFxError::TooManyDecimals => "fixed-point number has more than 19 decimals",
            ParseFxError::OutOfRange => "fixed-point number out of range",
        })
    }
}

impl std::error::Error for ParseFxError {}

impl FromStr for Fx {
    type Err = ParseFxError;

    /// Parses a decimal such as `1.5`, `-0.25` or `3`, rounding to the nearest
    /// representable value with integer arithmetic only.
    fn from_str(text: &str) -> Result<Fx, ParseFxError> {
        let text = text.trim();
        let (negative, digits) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        let (int_text, frac_text) = digits.split_once('.').unwrap_or((digits, ""));
        if int_text.is_empty() && frac_text.is_empty() {
            return Err(ParseFxError::Empty);
        }
        if frac_text.len() > MAX_FRAC_DIGITS {
            return Err(ParseFxError::TooManyDecimals);
        }

        let mut int: u128 = 0;
        for byte in int_text.bytes() {
            int = int * 10 + digit(byte)? as u128;
            if int > 1 << 31 {
                return Err(ParseFxError::OutOfRange);
            }
        }
        let mut frac: u128 = 0;
        for byte in frac_text.bytes() {
            frac = frac * 10 + digit(byte)? as u128;
        }

        let scale = 10u128.pow(frac_text.len() as u32);
        let frac_bits = ((frac << Fx::FRAC_BITS) + scale / 2) / scale;
        let magnitude = (int << Fx::FRAC_BITS) + frac_bits;
        let limit = if negative { 1u128 << 63 } else { (1u128 << 63) - 1 };
        if magnitude > limit {
            return Err(ParseFxError::OutOfRange);
        }
        let bits = if negative { (magnitude as i128).wrapping_neg() } else { magnitude as i128 };
        Ok(Fx::from_bits(bits as i64))
    }
}

fn digit(byte: u8) -> Result<u8, ParseFxError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        _ => Err(ParseFxError::InvalidDigit),
    }
}

//! Serde support.
//!
//! Binary formats (saves, network) store the raw `i64` bits. Human-readable
//! formats (RON content files and save dumps) use numbers, so content can write
//! `radius: 0.5`:
//!
//! - Serializing writes an `f64` when that is exact (any value whose bits fit
//!   in 53 bits, which covers every position in the world) and a decimal
//!   string otherwise.
//! - Deserializing accepts integers, decimal strings, and `f64`. An `f64` is
//!   converted by decoding its bits with integer arithmetic, so no
//!   floating-point operation is involved and the result is the same on every
//!   platform. Values with more precision than `Fx` round to nearest.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Fx;

const EXACT_F64_BITS: i64 = 1 << 53;

impl Serialize for Fx {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !serializer.is_human_readable() {
            return serializer.serialize_i64(self.to_bits());
        }
        let bits = self.to_bits();
        if bits > -EXACT_F64_BITS && bits < EXACT_F64_BITS {
            // Both steps are exact: the bits fit in the mantissa, and the
            // division only changes the exponent.
            serializer.serialize_f64(bits as f64 / (1u64 << Fx::FRAC_BITS) as f64)
        } else {
            serializer.collect_str(self)
        }
    }
}

impl<'de> Deserialize<'de> for Fx {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Fx, D::Error> {
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(FxVisitor)
        } else {
            i64::deserialize(deserializer).map(Fx::from_bits)
        }
    }
}

struct FxVisitor;

impl Visitor<'_> for FxVisitor {
    type Value = Fx;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number or a decimal string")
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Fx, E> {
        i32::try_from(v).map(Fx::from_int).map_err(|_| E::custom("fixed-point number out of range"))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Fx, E> {
        i32::try_from(v).map(Fx::from_int).map_err(|_| E::custom("fixed-point number out of range"))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Fx, E> {
        from_f64_bits(v.to_bits()).ok_or_else(|| E::custom("fixed-point number out of range"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Fx, E> {
        v.parse().map_err(E::custom)
    }
}

/// Converts the bits of an `f64` to `Fx`, rounding to nearest (ties away
/// from zero), with integer arithmetic only. Returns `None` for NaN,
/// infinities and values out of range.
fn from_f64_bits(bits: u64) -> Option<Fx> {
    let negative = bits >> 63 != 0;
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let fraction = bits & ((1 << 52) - 1);
    if exponent == 0x7ff {
        return None;
    }
    // value = mantissa * 2^power
    let (mantissa, power) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | 1 << 52, exponent - 1075)
    };
    // Fx bits = mantissa * 2^(power + 32)
    let shift = power + Fx::FRAC_BITS as i64;
    let magnitude: u128 = if shift >= 0 {
        if shift > 74 {
            return None;
        }
        (mantissa as u128) << shift
    } else if -shift > 64 {
        0
    } else {
        let s = -shift as u32;
        (mantissa as u128 + (1 << (s - 1))) >> s
    };
    let limit = if negative { 1u128 << 63 } else { (1u128 << 63) - 1 };
    if magnitude > limit {
        return None;
    }
    let value = if negative { (magnitude as i128).wrapping_neg() } else { magnitude as i128 };
    Some(Fx::from_bits(value as i64))
}

#[cfg(test)]
mod tests {
    use super::from_f64_bits;
    use crate::Fx;

    #[test]
    fn f64_conversion_is_exact_when_possible() {
        assert_eq!(from_f64_bits(0.5f64.to_bits()), Some(Fx::HALF));
        assert_eq!(from_f64_bits((-3.25f64).to_bits()), Some("-3.25".parse().unwrap()));
        assert_eq!(from_f64_bits(0.0f64.to_bits()), Some(Fx::ZERO));
        assert_eq!(from_f64_bits((-0.0f64).to_bits()), Some(Fx::ZERO));
        assert_eq!(from_f64_bits(f64::MIN_POSITIVE.to_bits()), Some(Fx::ZERO));
        assert_eq!(from_f64_bits(f64::NAN.to_bits()), None);
        assert_eq!(from_f64_bits(3e9f64.to_bits()), None);
        assert_eq!(from_f64_bits((-2147483648.0f64).to_bits()), Some(Fx::from_int(i32::MIN)));
        // 0.1 is not exact in f64 either; both paths round to the same Fx.
        assert_eq!(from_f64_bits(0.1f64.to_bits()), Some("0.1".parse().unwrap()));
    }

    #[test]
    fn exact_values_round_trip_through_f64() {
        for bits in [0i64, 1, -1, 12345678901, -(1 << 52), (1 << 53) - 1] {
            let as_f64 = bits as f64 / (1u64 << 32) as f64;
            assert_eq!(from_f64_bits(as_f64.to_bits()), Some(Fx::from_bits(bits)));
        }
    }
}

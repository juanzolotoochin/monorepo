use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};


use crate::trig;

/// A signed 32.32 fixed-point number.
///
/// The range is about ±2.1e9 with a resolution of 2^-32 (about 2.3e-10).
/// Overflow panics in builds with debug assertions and wraps otherwise, the
/// same as Rust integers. Wrapping is still deterministic, but it is a bug.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fx(i64);

impl Fx {
    pub const FRAC_BITS: u32 = 32;
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(1 << Self::FRAC_BITS);
    pub const HALF: Fx = Fx(1 << (Self::FRAC_BITS - 1));
    /// The smallest positive value, 2^-32.
    pub const EPSILON: Fx = Fx(1);
    pub const MIN: Fx = Fx(i64::MIN);
    pub const MAX: Fx = Fx(i64::MAX);
    pub const PI: Fx = Fx(trig::PI_BITS);
    pub const TAU: Fx = Fx(trig::TAU_BITS);
    pub const FRAC_PI_2: Fx = Fx(trig::FRAC_PI_2_BITS);

    const FRAC_MASK: i64 = (1 << Self::FRAC_BITS) - 1;

    /// Builds a value from its raw 32.32 representation.
    #[inline]
    pub const fn from_bits(bits: i64) -> Fx {
        Fx(bits)
    }

    /// The raw 32.32 representation, for hashing and saves.
    #[inline]
    pub const fn to_bits(self) -> i64 {
        self.0
    }

    /// Converts an integer. Every `i32` is representable, so this cannot overflow.
    #[inline]
    pub const fn from_int(n: i32) -> Fx {
        Fx((n as i64) << Self::FRAC_BITS)
    }

    /// `num / den`, rounded toward zero. Panics if `den` is zero.
    #[inline]
    pub const fn from_ratio(num: i64, den: i64) -> Fx {
        Fx(narrow(((num as i128) << Self::FRAC_BITS) / den as i128))
    }

    /// The largest integer less than or equal to `self`.
    #[inline]
    pub const fn floor_to_int(self) -> i64 {
        self.0 >> Self::FRAC_BITS
    }

    #[inline]
    pub const fn floor(self) -> Fx {
        Fx(self.0 & !Self::FRAC_MASK)
    }

    #[inline]
    pub fn ceil(self) -> Fx {
        (self + Fx(Self::FRAC_MASK)).floor()
    }

    /// Rounds half toward positive infinity.
    #[inline]
    pub fn round(self) -> Fx {
        (self + Self::HALF).floor()
    }

    /// The fractional part, always in `[0, 1)`.
    #[inline]
    pub const fn fract(self) -> Fx {
        Fx(self.0 & Self::FRAC_MASK)
    }

    #[inline]
    pub fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    #[inline]
    pub const fn signum(self) -> Fx {
        Fx::from_int(self.0.signum() as i32)
    }

    #[inline]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Multiplies by an integer.
    #[inline]
    pub fn mul_int(self, n: i64) -> Fx {
        Fx(self.0 * n)
    }

    /// Divides by an integer, rounding toward zero.
    #[inline]
    pub fn div_int(self, n: i64) -> Fx {
        Fx(self.0 / n)
    }

    /// `a + (b - a) * t`.
    #[inline]
    pub fn lerp(a: Fx, b: Fx, t: Fx) -> Fx {
        a + (b - a) * t
    }

    /// The square root, rounded down. Negative inputs return zero.
    pub fn sqrt(self) -> Fx {
        if self.0 <= 0 {
            return Fx::ZERO;
        }
        // sqrt(v * 2^32) has 16 integer bits more than v's 32.32 needs, so
        // shifting the radicand by 32 yields a 32.32 result directly.
        Fx((((self.0 as u128) << Self::FRAC_BITS).isqrt()) as i64)
    }

    /// The sine of an angle in radians.
    #[inline]
    pub fn sin(self) -> Fx {
        Fx(trig::sin_bits(self.0))
    }

    /// The cosine of an angle in radians.
    #[inline]
    pub fn cos(self) -> Fx {
        Fx(trig::cos_bits(self.0))
    }

    #[inline]
    pub fn sin_cos(self) -> (Fx, Fx) {
        let (s, c) = trig::sin_cos_bits(self.0);
        (Fx(s), Fx(c))
    }

    /// The angle of `(x, y)` in radians, in `[-PI, PI]`. Returns zero for the origin.
    #[inline]
    pub fn atan2(y: Fx, x: Fx) -> Fx {
        Fx(trig::atan2_bits(y.0, x.0))
    }

    /// For the view only. The simulation must never convert to floating point.
    #[inline]
    pub fn to_f32(self) -> f32 {
        self.to_f64() as f32
    }

    /// For the view and tests only. The simulation must never convert to floating point.
    #[inline]
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / (1u64 << Self::FRAC_BITS) as f64
    }
}

/// Narrows a wide intermediate to `i64`, panicking on overflow when debug
/// assertions are on and wrapping otherwise.
#[inline]
pub(crate) const fn narrow(v: i128) -> i64 {
    debug_assert!(v >= i64::MIN as i128 && v <= i64::MAX as i128, "Fx overflow");
    v as i64
}

impl Add for Fx {
    type Output = Fx;
    #[inline]
    fn add(self, rhs: Fx) -> Fx {
        Fx(self.0 + rhs.0)
    }
}

impl Sub for Fx {
    type Output = Fx;
    #[inline]
    fn sub(self, rhs: Fx) -> Fx {
        Fx(self.0 - rhs.0)
    }
}

impl Neg for Fx {
    type Output = Fx;
    #[inline]
    fn neg(self) -> Fx {
        Fx(-self.0)
    }
}

impl Mul for Fx {
    type Output = Fx;
    /// Rounds to nearest, ties toward positive infinity.
    #[inline]
    fn mul(self, rhs: Fx) -> Fx {
        let wide = self.0 as i128 * rhs.0 as i128;
        Fx(narrow((wide + (1 << (Fx::FRAC_BITS - 1))) >> Fx::FRAC_BITS))
    }
}

impl Div for Fx {
    type Output = Fx;
    /// Rounds toward zero. Panics if `rhs` is zero.
    #[inline]
    fn div(self, rhs: Fx) -> Fx {
        Fx(narrow(((self.0 as i128) << Fx::FRAC_BITS) / rhs.0 as i128))
    }
}

macro_rules! assign_op {
    ($trait:ident, $method:ident, $op:tt) => {
        impl $trait for Fx {
            #[inline]
            fn $method(&mut self, rhs: Fx) {
                *self = *self $op rhs;
            }
        }
    };
}

assign_op!(AddAssign, add_assign, +);
assign_op!(SubAssign, sub_assign, -);
assign_op!(MulAssign, mul_assign, *);
assign_op!(DivAssign, div_assign, /);

impl Sum for Fx {
    fn sum<I: Iterator<Item = Fx>>(iter: I) -> Fx {
        iter.fold(Fx::ZERO, Add::add)
    }
}

impl fmt::Display for Fx {
    /// Prints up to 10 decimals, which is enough for parsing to return the same value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DIGITS: u32 = 10;
        const SCALE: u128 = 10u128.pow(DIGITS);
        let magnitude = self.0.unsigned_abs();
        let mut int = magnitude >> Fx::FRAC_BITS;
        let frac = (magnitude & Fx::FRAC_MASK as u64) as u128;
        let mut decimals = (frac * SCALE + (1 << (Fx::FRAC_BITS - 1))) >> Fx::FRAC_BITS;
        if decimals == SCALE {
            int += 1;
            decimals = 0;
        }
        let sign = if self.0 < 0 { "-" } else { "" };
        if decimals == 0 {
            return write!(f, "{sign}{int}");
        }
        let text = format!("{decimals:0width$}", width = DIGITS as usize);
        write!(f, "{sign}{int}.{}", text.trim_end_matches('0'))
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fx({self})")
    }
}

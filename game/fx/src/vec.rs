use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use crate::Fx;

/// Length of a vector given its components' raw bits. Squares are summed in
/// `u128`, so this cannot overflow and is exact up to the final rounding down.
fn length_bits(components: &[i64]) -> Fx {
    let sum: u128 = components.iter().map(|&c| (c.unsigned_abs() as u128).pow(2)).sum();
    // The sum has 64 fractional bits; its square root has 32.
    Fx::from_bits(sum.isqrt() as i64)
}

macro_rules! fx_vec {
    ($name:ident { $($field:ident),+ }) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
        pub struct $name {
            $(pub $field: Fx),+
        }

        impl $name {
            pub const ZERO: $name = $name { $($field: Fx::ZERO),+ };

            #[inline]
            pub const fn new($($field: Fx),+) -> $name {
                $name { $($field),+ }
            }

            #[inline]
            pub fn dot(self, rhs: $name) -> Fx {
                Fx::ZERO $(+ self.$field * rhs.$field)+
            }

            #[inline]
            pub fn length_squared(self) -> Fx {
                self.dot(self)
            }

            /// Exact up to rounding down, and never overflows.
            #[inline]
            pub fn length(self) -> Fx {
                length_bits(&[$(self.$field.to_bits()),+])
            }

            #[inline]
            pub fn distance(self, rhs: $name) -> Fx {
                (self - rhs).length()
            }

            /// The unit vector in the same direction, or zero for the zero vector.
            pub fn normalize_or_zero(self) -> $name {
                let length = self.length();
                if length == Fx::ZERO {
                    return $name::ZERO;
                }
                self / length
            }

            #[inline]
            pub fn lerp(a: $name, b: $name, t: Fx) -> $name {
                a + (b - a) * t
            }
        }

        impl Add for $name {
            type Output = $name;
            #[inline]
            fn add(self, rhs: $name) -> $name {
                $name { $($field: self.$field + rhs.$field),+ }
            }
        }

        impl Sub for $name {
            type Output = $name;
            #[inline]
            fn sub(self, rhs: $name) -> $name {
                $name { $($field: self.$field - rhs.$field),+ }
            }
        }

        impl Neg for $name {
            type Output = $name;
            #[inline]
            fn neg(self) -> $name {
                $name { $($field: -self.$field),+ }
            }
        }

        impl Mul<Fx> for $name {
            type Output = $name;
            #[inline]
            fn mul(self, rhs: Fx) -> $name {
                $name { $($field: self.$field * rhs),+ }
            }
        }

        impl Div<Fx> for $name {
            type Output = $name;
            #[inline]
            fn div(self, rhs: Fx) -> $name {
                $name { $($field: self.$field / rhs),+ }
            }
        }

        impl AddAssign for $name {
            #[inline]
            fn add_assign(&mut self, rhs: $name) {
                *self = *self + rhs;
            }
        }

        impl SubAssign for $name {
            #[inline]
            fn sub_assign(&mut self, rhs: $name) {
                *self = *self - rhs;
            }
        }
    };
}

fx_vec!(FxVec2 { x, y });
fx_vec!(FxVec3 { x, y, z });

impl FxVec2 {
    /// The unit vector at `radians`.
    pub fn from_angle(radians: Fx) -> FxVec2 {
        let (sin, cos) = radians.sin_cos();
        FxVec2::new(cos, sin)
    }

    /// The angle of this vector in radians, in `[-PI, PI]`.
    pub fn angle(self) -> Fx {
        Fx::atan2(self.y, self.x)
    }

    /// The z component of the 3D cross product.
    pub fn perp_dot(self, rhs: FxVec2) -> Fx {
        self.x * rhs.y - self.y * rhs.x
    }
}

impl FxVec3 {
    pub fn cross(self, rhs: FxVec3) -> FxVec3 {
        FxVec3::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }

    pub fn xy(self) -> FxVec2 {
        FxVec2::new(self.x, self.y)
    }
}

//! The golden report: every `Fx` operation run over fixed pseudo-random
//! inputs, folded into one hash per operation. The report must be identical on
//! every platform. `testdata/golden.txt` holds the expected report; regenerate
//! it with `bazel run //kestrel/fx:update_golden` after an intended change.

use std::fmt::Write;

use crate::{Fx, FxVec2, FxVec3};

const SAMPLES: usize = 100_000;

/// splitmix64, so the inputs never depend on a library's RNG.
struct Inputs(u64);

impl Inputs {
    fn next(&mut self) -> i64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as i64
    }

    /// Any bit pattern.
    fn any(&mut self) -> Fx {
        Fx::from_bits(self.next())
    }

    /// Magnitude below 2^14, so products and sums cannot overflow.
    fn small(&mut self) -> Fx {
        Fx::from_bits(self.next() >> 18)
    }

    /// Magnitude in [2^-8, 2^14), so quotients of small values cannot overflow.
    fn divisor(&mut self) -> Fx {
        let bits = self.next() >> 18;
        Fx::from_bits(if bits < 0 { bits.min(-(1 << 24)) } else { bits.max(1 << 24) })
    }
}

/// FNV-1a over the raw bits of every result.
struct Fold(u64);

impl Fold {
    fn new() -> Fold {
        Fold(0xcbf2_9ce4_8422_2325)
    }

    fn add(&mut self, value: Fx) {
        for byte in value.to_bits().to_le_bytes() {
            self.0 = (self.0 ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn line(report: &mut String, name: &str, seed: u64, mut op: impl FnMut(&mut Inputs) -> Vec<Fx>) {
    let mut inputs = Inputs(seed);
    let mut fold = Fold::new();
    for _ in 0..SAMPLES {
        for value in op(&mut inputs) {
            fold.add(value);
        }
    }
    writeln!(report, "{name} {:016x}", fold.0).unwrap();
}

/// Builds the golden report.
pub fn report() -> String {
    let mut out = String::new();
    line(&mut out, "add", 1, |i| vec![i.small() + i.small()]);
    line(&mut out, "sub", 2, |i| vec![i.small() - i.small()]);
    line(&mut out, "mul", 3, |i| vec![i.small() * i.small()]);
    line(&mut out, "div", 4, |i| vec![i.small() / i.divisor()]);
    line(&mut out, "from_ratio", 5, |i| {
        vec![Fx::from_ratio(i.next() >> 34, (i.next() >> 34) | 1)]
    });
    line(&mut out, "rounding", 6, |i| {
        let x = i.small();
        vec![x.floor(), x.ceil(), x.round(), x.fract(), x.abs(), x.signum()]
    });
    line(&mut out, "sqrt", 7, |i| vec![i.any().abs().sqrt()]);
    line(&mut out, "sin_cos", 8, |i| {
        let x = i.any();
        vec![x.sin(), x.cos()]
    });
    line(&mut out, "atan2", 9, |i| vec![Fx::atan2(i.any(), i.any())]);
    line(&mut out, "parse", 10, |i| vec![i.any().to_string().parse().unwrap()]);
    line(&mut out, "vec2", 11, |i| {
        let a = FxVec2::new(i.small(), i.small());
        let b = FxVec2::new(i.small(), i.small());
        vec![a.dot(b), a.length(), a.normalize_or_zero().x, a.angle(), a.perp_dot(b)]
    });
    line(&mut out, "vec3", 12, |i| {
        let a = FxVec3::new(i.small(), i.small(), i.small());
        let b = FxVec3::new(i.small(), i.small(), i.small());
        let n = a.normalize_or_zero();
        let c = a.cross(b);
        vec![a.length(), n.x, n.y, n.z, c.x, c.y, c.z]
    });
    writeln!(out, "pi {}", Fx::PI.to_bits()).unwrap();
    writeln!(out, "sin(1) {}", Fx::ONE.sin().to_bits()).unwrap();
    writeln!(out, "atan2(1, 2) {}", Fx::atan2(Fx::ONE, Fx::from_int(2)).to_bits()).unwrap();
    writeln!(out, "sqrt(2) {}", Fx::from_int(2).sqrt().to_bits()).unwrap();
    out
}

use crate::{golden, Fx, FxVec2, FxVec3, ParseFxError};

fn fx(text: &str) -> Fx {
    text.parse().unwrap()
}

fn from_f64(v: f64) -> Fx {
    Fx::from_bits((v * (1u64 << 32) as f64).round() as i64)
}

/// A deterministic stream of test inputs.
fn inputs(seed: u64, count: usize) -> impl Iterator<Item = i64> {
    let mut state = seed;
    (0..count).map(move |_| {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as i64
    })
}

fn assert_close(actual: Fx, expected: f64, tolerance: f64, context: &str) {
    let error = (actual.to_f64() - expected).abs();
    assert!(error <= tolerance, "{context}: got {actual}, expected {expected}, error {error:e}");
}

#[test]
fn constants() {
    assert_close(Fx::PI, std::f64::consts::PI, 1.2e-10, "pi");
    assert_close(Fx::TAU, std::f64::consts::TAU, 1.2e-10, "tau");
    assert_close(Fx::FRAC_PI_2, std::f64::consts::FRAC_PI_2, 1.2e-10, "pi/2");
    assert_eq!(Fx::ONE.to_f64(), 1.0);
    assert_eq!(Fx::HALF.to_f64(), 0.5);
}

#[test]
fn arithmetic_matches_f64() {
    let values: Vec<i64> = inputs(1, 20_000).map(|v| v >> 18).collect();
    for pair in values.chunks(2) {
        let (a, b) = (Fx::from_bits(pair[0]), Fx::from_bits(pair[1]));
        let (fa, fb) = (a.to_f64(), b.to_f64());
        assert_eq!((a + b).to_f64(), fa + fb);
        assert_eq!((a - b).to_f64(), fa - fb);
        // Rounded to nearest: within half an ulp, plus f64's own rounding.
        assert_close(a * b, fa * fb, 1.2e-10 + (fa * fb).abs() * 1e-15, "mul");
        if b.abs() > Fx::from_ratio(1, 256) {
            assert_close(a / b, fa / fb, 2.4e-10 + (fa / fb).abs() * 1e-15, "div");
        }
    }
}

#[test]
fn integer_conversions() {
    assert_eq!(Fx::from_int(-3).to_f64(), -3.0);
    assert_eq!(Fx::from_int(i32::MAX).floor_to_int(), i32::MAX as i64);
    assert_eq!(Fx::from_int(i32::MIN).floor_to_int(), i32::MIN as i64);
    assert_eq!(Fx::from_ratio(1, 3).to_bits(), (1i64 << 32) / 3);
    assert_eq!(Fx::from_int(7).mul_int(3), Fx::from_int(21));
    assert_eq!(Fx::from_int(7).div_int(2), fx("3.5"));
}

#[test]
fn rounding() {
    assert_eq!(fx("2.5").floor(), fx("2"));
    assert_eq!(fx("-2.5").floor(), fx("-3"));
    assert_eq!(fx("2.25").ceil(), fx("3"));
    assert_eq!(fx("-2.25").ceil(), fx("-2"));
    assert_eq!(fx("2.5").round(), fx("3"));
    assert_eq!(fx("-2.5").round(), fx("-2"));
    assert_eq!(fx("-2.25").fract(), fx("0.75"));
    assert_eq!(fx("-2.25").floor_to_int(), -3);
    assert_eq!(fx("-0.5").signum(), Fx::from_int(-1));
    assert_eq!(Fx::ZERO.signum(), Fx::ZERO);
}

#[test]
fn sqrt_is_exact_floor() {
    for bits in inputs(2, 20_000).map(|v| v.unsigned_abs() as i64 & i64::MAX) {
        let root = Fx::from_bits(bits).sqrt().to_bits() as u128;
        let radicand = (bits as u128) << 32;
        assert!(root * root <= radicand && (root + 1) * (root + 1) > radicand, "sqrt of {bits}");
    }
    assert_eq!(Fx::from_int(4).sqrt(), Fx::from_int(2));
    assert_eq!(Fx::from_int(-4).sqrt(), Fx::ZERO);
    assert_close(Fx::from_int(2).sqrt(), std::f64::consts::SQRT_2, 2.4e-10, "sqrt(2)");
}

#[test]
fn sin_cos_match_f64() {
    // Bits below 2^53 so the input converts to f64 exactly.
    for bits in inputs(3, 50_000).map(|v| v >> 11) {
        let x = Fx::from_bits(bits);
        let (sin, cos) = x.sin_cos();
        assert_eq!(sin, x.sin());
        assert_eq!(cos, x.cos());
        assert_close(sin, x.to_f64().sin(), 3e-8, &format!("sin({x})"));
        assert_close(cos, x.to_f64().cos(), 3e-8, &format!("cos({x})"));
    }
}

#[test]
fn sin_cos_exact_points() {
    assert_eq!(Fx::ZERO.sin(), Fx::ZERO);
    assert_eq!(Fx::ZERO.cos(), Fx::ONE);
    assert_close(Fx::FRAC_PI_2.sin(), 1.0, 1e-9, "sin(pi/2)");
    assert_close((-Fx::FRAC_PI_2).sin(), -1.0, 1e-9, "sin(-pi/2)");
    assert_close(Fx::PI.sin(), 0.0, 1e-9, "sin(pi)");
    assert_close(Fx::PI.cos(), -1.0, 1e-9, "cos(pi)");
}

#[test]
fn trig_handles_any_input() {
    for bits in inputs(4, 10_000) {
        let x = Fx::from_bits(bits);
        let (sin, cos) = x.sin_cos();
        assert!(sin.abs() <= Fx::ONE && cos.abs() <= Fx::ONE);
        let radius = (sin * sin + cos * cos).to_f64();
        assert!((radius - 1.0).abs() < 1e-7, "sin^2 + cos^2 = {radius} at {x}");
    }
}

#[test]
fn atan2_matches_f64() {
    let values: Vec<i64> = inputs(5, 40_000).map(|v| v >> 11).collect();
    for pair in values.chunks(2) {
        let (y, x) = (Fx::from_bits(pair[0]), Fx::from_bits(pair[1]));
        let expected = y.to_f64().atan2(x.to_f64());
        assert_close(Fx::atan2(y, x), expected, 1e-8, &format!("atan2({y}, {x})"));
    }
}

#[test]
fn atan2_quadrants() {
    let one = Fx::ONE;
    assert_eq!(Fx::atan2(Fx::ZERO, Fx::ZERO), Fx::ZERO);
    assert_eq!(Fx::atan2(Fx::ZERO, one), Fx::ZERO);
    assert_eq!(Fx::atan2(one, Fx::ZERO), Fx::FRAC_PI_2);
    assert_eq!(Fx::atan2(-one, Fx::ZERO), -Fx::FRAC_PI_2);
    assert_eq!(Fx::atan2(Fx::ZERO, -one), Fx::PI);
    assert_close(Fx::atan2(-one, -one), -3.0 * std::f64::consts::FRAC_PI_4, 1e-9, "third quadrant");
    assert_close(Fx::atan2(Fx::MIN, Fx::MIN), -3.0 * std::f64::consts::FRAC_PI_4, 1e-9, "extremes");
}

#[test]
fn parse_decimals() {
    assert_eq!(fx("1.5"), Fx::from_int(3).div_int(2));
    assert_eq!(fx("-0.25").to_f64(), -0.25);
    assert_eq!(fx("3"), Fx::from_int(3));
    assert_eq!(fx("+3."), Fx::from_int(3));
    assert_eq!(fx(".5"), Fx::HALF);
    assert_eq!(fx(" 2 "), Fx::from_int(2));
    // 0.1 * 2^32 = 429496729.6, rounded to nearest.
    assert_eq!(fx("0.1").to_bits(), 429_496_730);
    assert_eq!(fx("-2147483648"), Fx::from_int(i32::MIN));
    assert_eq!(fx("2147483647.9999999998"), Fx::MAX);
    assert_eq!(fx("0.0000000000000000001"), Fx::ZERO);
}

#[test]
fn parse_errors() {
    let parse = |text: &str| text.parse::<Fx>();
    assert_eq!(parse(""), Err(ParseFxError::Empty));
    assert_eq!(parse("-"), Err(ParseFxError::Empty));
    assert_eq!(parse("."), Err(ParseFxError::Empty));
    assert_eq!(parse("1.2.3"), Err(ParseFxError::InvalidDigit));
    assert_eq!(parse("1e5"), Err(ParseFxError::InvalidDigit));
    assert_eq!(parse("--1"), Err(ParseFxError::InvalidDigit));
    assert_eq!(parse("2147483648"), Err(ParseFxError::OutOfRange));
    assert_eq!(parse("-2147483648.1"), Err(ParseFxError::OutOfRange));
    assert_eq!(parse("99999999999999999999999"), Err(ParseFxError::OutOfRange));
    assert_eq!(parse("0.12345678901234567890"), Err(ParseFxError::TooManyDecimals));
}

#[test]
fn display_round_trips() {
    assert_eq!(fx("1.5").to_string(), "1.5");
    assert_eq!(fx("-0.25").to_string(), "-0.25");
    assert_eq!(Fx::from_int(-7).to_string(), "-7");
    assert_eq!(format!("{:?}", Fx::HALF), "Fx(0.5)");
    for bits in inputs(6, 20_000).chain([i64::MIN, i64::MAX, 0, 1, -1]) {
        let value = Fx::from_bits(bits);
        assert_eq!(value.to_string().parse::<Fx>(), Ok(value), "round trip of {bits}");
    }
}

#[test]
fn vectors() {
    let a = FxVec2::new(fx("3"), fx("4"));
    assert_eq!(a.length(), fx("5"));
    assert_eq!(a.length_squared(), fx("25"));
    assert_eq!(a.dot(FxVec2::new(fx("2"), fx("-1"))), fx("2"));
    // Division rounds toward zero, so 3/5 lands one ulp below the nearest value to 0.6.
    let unit = a.normalize_or_zero();
    assert_close(unit.x, 0.6, 2.4e-10, "normalized x");
    assert_close(unit.y, 0.8, 2.4e-10, "normalized y");
    assert_eq!(FxVec2::ZERO.normalize_or_zero(), FxVec2::ZERO);
    assert_eq!(FxVec2::lerp(FxVec2::ZERO, a, Fx::HALF), FxVec2::new(fx("1.5"), fx("2")));
    assert_close(FxVec2::from_angle(a.angle()).y, 0.8, 1e-8, "from_angle");

    let b = FxVec3::new(fx("2"), fx("3"), fx("6"));
    assert_eq!(b.length(), fx("7"));
    assert_eq!(b.distance(FxVec3::ZERO), fx("7"));
    let x = FxVec3::new(Fx::ONE, Fx::ZERO, Fx::ZERO);
    let y = FxVec3::new(Fx::ZERO, Fx::ONE, Fx::ZERO);
    assert_eq!(x.cross(y), FxVec3::new(Fx::ZERO, Fx::ZERO, Fx::ONE));
    // Components near the limit still give a length, because squares are summed in u128.
    let big = FxVec3::new(Fx::from_int(1 << 30), Fx::from_int(1 << 30), Fx::ZERO);
    assert_close(big.length(), (1u64 << 30) as f64 * std::f64::consts::SQRT_2, 1e-9 * (1u64 << 30) as f64, "big");
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "overflow")]
fn mul_overflow_panics_in_debug() {
    let _ = Fx::from_int(1 << 20) * Fx::from_int(1 << 20);
}

#[test]
fn golden_report_matches() {
    let expected = include_str!("../testdata/golden.txt");
    let actual = golden::report();
    // Compare by line: `lines` drops a trailing CR, in case a checkout converted line endings.
    assert!(
        actual.lines().eq(expected.lines()),
        "Fx results changed. If this is intended, run `bazel run //game/fx:update_golden`.\n\
         expected:\n{expected}\nactual:\n{actual}"
    );
}

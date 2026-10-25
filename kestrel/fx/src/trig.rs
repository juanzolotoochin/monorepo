//! Table-based trigonometry.
//!
//! The tables are computed at compile time with integer arithmetic at 60
//! fractional bits, so they never depend on a platform's floating-point math
//! library. Lookups interpolate linearly between entries.

/// Fractional bits of the compile-time arithmetic.
const P: u32 = 60;
const ONE_P: i128 = 1 << P;
/// round(pi * 2^60).
const PI_P: i128 = 3622009729038561421;

pub(crate) const PI_BITS: i64 = to_fx_bits(PI_P);
pub(crate) const TAU_BITS: i64 = to_fx_bits(PI_P * 2);
pub(crate) const FRAC_PI_2_BITS: i64 = to_fx_bits(PI_P / 2);

/// floor(2^64 / tau): converts radians (32 fractional bits) into turns
/// (96 fractional bits) with a single multiplication.
const INV_TAU_64: i128 = (1i128 << (64 + P)) / (2 * PI_P);

/// Entries per quarter turn in the sine table and per unit in the atan table.
const TABLE_BITS: u32 = 12;
const TABLE_LEN: usize = 1 << TABLE_BITS;

/// sin(k / TABLE_LEN * pi / 2) for k in 0..=TABLE_LEN, as 32.32 bits.
static SIN_TABLE: [i64; TABLE_LEN + 1] = build_sin_table();
/// atan(k / TABLE_LEN) for k in 0..=TABLE_LEN, as 32.32 bits.
static ATAN_TABLE: [i64; TABLE_LEN + 1] = build_atan_table();

const fn to_fx_bits(v: i128) -> i64 {
    ((v + (1 << (P - 33))) >> (P - 32)) as i64
}

const fn mul_p(a: i128, b: i128) -> i128 {
    (a * b) >> P
}

/// Taylor series for sin(x), x in [0, pi/2], at 60 fractional bits.
const fn sin_p(x: i128) -> i128 {
    let x2 = mul_p(x, x);
    let mut term = x;
    let mut sum = x;
    let mut n = 1;
    while term != 0 {
        term = -mul_p(term, x2) / ((2 * n) * (2 * n + 1));
        sum += term;
        n += 1;
    }
    sum
}

/// atan(x) for x in [0, 1], at 60 fractional bits. Halves the angle once with
/// atan(x) = 2 atan(x / (1 + sqrt(1 + x^2))) so the Taylor series converges fast.
const fn atan_p(x: i128) -> i128 {
    let root = (((ONE_P + mul_p(x, x)) as u128) << P).isqrt() as i128;
    let u = (x << P) / (ONE_P + root);
    let u2 = mul_p(u, u);
    let mut power = u;
    let mut sum = 0;
    let mut n = 0;
    while power != 0 {
        let term = power / (2 * n + 1);
        sum += if n % 2 == 0 { term } else { -term };
        power = mul_p(power, u2);
        n += 1;
    }
    2 * sum
}

const fn build_sin_table() -> [i64; TABLE_LEN + 1] {
    let mut table = [0; TABLE_LEN + 1];
    let mut k = 0;
    while k <= TABLE_LEN {
        let x = (PI_P / 2) * k as i128 / TABLE_LEN as i128;
        table[k] = to_fx_bits(sin_p(x));
        k += 1;
    }
    table
}

const fn build_atan_table() -> [i64; TABLE_LEN + 1] {
    let mut table = [0; TABLE_LEN + 1];
    let mut k = 0;
    while k <= TABLE_LEN {
        let x = ONE_P * k as i128 / TABLE_LEN as i128;
        table[k] = to_fx_bits(atan_p(x));
        k += 1;
    }
    table
}

/// Interpolates `table` at `pos`, a position with `frac_bits` bits below the index.
#[inline]
fn lookup(table: &[i64; TABLE_LEN + 1], pos: u64, frac_bits: u32) -> i64 {
    let index = (pos >> frac_bits) as usize;
    if index >= TABLE_LEN {
        return table[TABLE_LEN];
    }
    let weight = (pos & ((1 << frac_bits) - 1)) as i64;
    let (a, b) = (table[index], table[index + 1]);
    a + (((b - a) * weight) >> frac_bits)
}

/// The angle as a fraction of a full turn, with 32 fractional bits. Wraps, so
/// any radian value is valid.
#[inline]
fn phase(radians: i64) -> u32 {
    ((radians as i128 * INV_TAU_64) >> 64) as u32
}

/// Sine of a phase (a 32-bit fraction of a turn).
#[inline]
fn sin_phase(phase: u32) -> i64 {
    const QUARTER_BITS: u32 = 30;
    const QUARTER: u64 = 1 << QUARTER_BITS;
    let within = (phase as u64) & (QUARTER - 1);
    let frac_bits = QUARTER_BITS - TABLE_BITS;
    match phase >> QUARTER_BITS {
        0 => lookup(&SIN_TABLE, within, frac_bits),
        1 => lookup(&SIN_TABLE, QUARTER - within, frac_bits),
        2 => -lookup(&SIN_TABLE, within, frac_bits),
        _ => -lookup(&SIN_TABLE, QUARTER - within, frac_bits),
    }
}

const QUARTER_TURN: u32 = 1 << 30;

pub(crate) fn sin_bits(radians: i64) -> i64 {
    sin_phase(phase(radians))
}

pub(crate) fn cos_bits(radians: i64) -> i64 {
    sin_phase(phase(radians).wrapping_add(QUARTER_TURN))
}

pub(crate) fn sin_cos_bits(radians: i64) -> (i64, i64) {
    let p = phase(radians);
    (sin_phase(p), sin_phase(p.wrapping_add(QUARTER_TURN)))
}

pub(crate) fn atan2_bits(y: i64, x: i64) -> i64 {
    if x == 0 && y == 0 {
        return 0;
    }
    let (ax, ay) = (x.unsigned_abs() as u128, y.unsigned_abs() as u128);
    // The ratio of the smaller to the larger magnitude, in [0, 1] with 32 fractional bits.
    let angle = if ay <= ax {
        lookup(&ATAN_TABLE, ((ay << 32) / ax) as u64, 32 - TABLE_BITS)
    } else {
        FRAC_PI_2_BITS - lookup(&ATAN_TABLE, ((ax << 32) / ay) as u64, 32 - TABLE_BITS)
    };
    let angle = if x < 0 { PI_BITS - angle } else { angle };
    if y < 0 {
        -angle
    } else {
        angle
    }
}

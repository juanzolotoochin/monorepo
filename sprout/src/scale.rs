//! Pure value→pixel scales and axis tick generation (no gpui).

/// Maps a value in `0..=domain_max` to a pixel length in `0..=range_px`.
pub struct LinearScale {
    domain_max: f64,
    range_px: f64,
}

impl LinearScale {
    pub fn new(domain_max: f64, range_px: f64) -> Self {
        Self { domain_max, range_px }
    }

    /// Pixel length corresponding to `value` (0 if the domain is empty).
    pub fn pixels(&self, value: f64) -> f64 {
        if self.domain_max <= 0.0 {
            0.0
        } else {
            value / self.domain_max * self.range_px
        }
    }
}

/// Round, ascending tick values from 0 covering `max`, ~`target` steps.
/// Steps snap to 1/2/5 × 10ⁿ. Always returns at least `[0, step]`.
pub fn nice_ticks(max: f64, target: usize) -> Vec<f64> {
    let target = target.max(1);
    if max <= 0.0 {
        return vec![0.0, 1.0];
    }
    let raw_step = max / target as f64;
    let magnitude = 10f64.powf(raw_step.log10().floor());
    let residual = raw_step / magnitude;
    let nice = if residual > 5.0 {
        10.0
    } else if residual > 2.0 {
        5.0
    } else if residual > 1.0 {
        2.0
    } else {
        1.0
    };
    let step = nice * magnitude;
    let last = (max / step).ceil() as i64;
    (0..=last).map(|i| i as f64 * step).collect()
}

/// Divides a width into `count` equal bands with fractional `padding`,
/// placing a centered bar in each band.
pub struct BandScale {
    band: f64,
    padding: f64,
}

impl BandScale {
    pub fn new(count: usize, range_px: f64, padding: f64) -> Self {
        let band = if count == 0 { 0.0 } else { range_px / count as f64 };
        Self { band, padding }
    }

    pub fn bar_width(&self) -> f64 {
        self.band * (1.0 - self.padding)
    }

    /// Left edge (px from range start) of bar `i`.
    pub fn left(&self, i: usize) -> f64 {
        self.band * i as f64 + self.band * self.padding / 2.0
    }

    /// Center (px from range start) of bar `i`.
    pub fn center(&self, i: usize) -> f64 {
        self.band * i as f64 + self.band / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_scale_maps_domain_to_pixels() {
        let s = LinearScale::new(4.0, 200.0);
        assert!((s.pixels(0.0) - 0.0).abs() < 1e-9);
        assert!((s.pixels(4.0) - 200.0).abs() < 1e-9);
        assert!((s.pixels(2.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn linear_scale_zero_domain_is_safe() {
        let s = LinearScale::new(0.0, 200.0);
        assert_eq!(s.pixels(0.0), 0.0); // no NaN/inf
    }

    #[test]
    fn nice_ticks_round_values_cover_max() {
        assert_eq!(nice_ticks(3.2, 5), vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        assert_eq!(nice_ticks(5.0, 5), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn nice_ticks_handles_zero_max() {
        let t = nice_ticks(0.0, 5);
        assert!(t.len() >= 2);
        assert_eq!(t[0], 0.0);
        assert!(*t.last().unwrap() > 0.0);
    }

    #[test]
    fn band_scale_centers_bars() {
        let b = BandScale::new(2, 100.0, 0.2);
        // bands of 50px; bar width = 40px; centers at 25 and 75.
        assert!((b.center(0) - 25.0).abs() < 1e-9);
        assert!((b.center(1) - 75.0).abs() < 1e-9);
        assert!((b.bar_width() - 40.0).abs() < 1e-9);
        assert!((b.left(0) - 5.0).abs() < 1e-9);
    }
}

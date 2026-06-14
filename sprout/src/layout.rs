//! Pure chart geometry: plot area, bar rects, tick positions (no painting).

use gpui::{bounds, point, px, size, Bounds, Pixels, Point};

use crate::data::Bar;
use crate::scale::{nice_ticks, BandScale, LinearScale};
use crate::theme::Theme;

/// Computed pixel geometry for one render pass.
#[derive(Clone)]
pub struct Layout {
    pub plot_area: Bounds<Pixels>,
    pub bars: Vec<Bounds<Pixels>>,
    pub y_ticks: Vec<(f64, Pixels)>,
    pub x_labels: Vec<(String, Pixels)>,
}

impl Layout {
    /// Index of the bar whose rectangle contains `point`, if any.
    pub fn bar_at(&self, point: Point<Pixels>) -> Option<usize> {
        self.bars.iter().position(|b| b.contains(&point))
    }

    /// Compute geometry for `area` (absolute window pixels), `bars`, and `theme`.
    pub fn compute(
        area: Bounds<Pixels>,
        bars: &[Bar],
        theme: &Theme,
        y_max_override: Option<f64>,
    ) -> Layout {
        let plot_area = bounds(
            point(area.origin.x + theme.margin_left, area.origin.y + theme.margin_top),
            size(
                area.size.width - theme.margin_left - theme.margin_right,
                area.size.height - theme.margin_top - theme.margin_bottom,
            ),
        );

        let plot_w: f32 = plot_area.size.width.into();
        let plot_h: f32 = plot_area.size.height.into();
        let baseline: f32 = (plot_area.origin.y + plot_area.size.height).into();
        let left: f32 = plot_area.origin.x.into();

        let data_max = bars.iter().map(|b| b.value).fold(0.0_f64, f64::max);
        let basis = y_max_override.unwrap_or(data_max);
        let ticks = nice_ticks(basis, theme.y_tick_count);
        let y_max = *ticks.last().unwrap_or(&1.0);

        let y_scale = LinearScale::new(y_max, plot_h as f64);
        let band = BandScale::new(bars.len(), plot_w as f64, 0.3);

        let mut bar_rects = Vec::with_capacity(bars.len());
        for (i, b) in bars.iter().enumerate() {
            let h = y_scale.pixels(b.value).min(plot_h as f64) as f32;
            let x = left + band.left(i) as f32;
            bar_rects.push(bounds(
                point(px(x), px(baseline - h)),
                size(px(band.bar_width() as f32), px(h)),
            ));
        }

        let y_ticks = ticks
            .iter()
            .map(|&v| (v, px(baseline - y_scale.pixels(v) as f32)))
            .collect();

        let x_labels = bars
            .iter()
            .enumerate()
            .map(|(i, b)| (b.label.clone(), px(left + band.center(i) as f32)))
            .collect();

        Layout { plot_area, bars: bar_rects, y_ticks, x_labels }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Bar;
    use crate::theme::Theme;
    use gpui::{bounds, point, px, size};

    fn sample() -> Vec<Bar> {
        vec![
            Bar { label: "a".into(), value: 1.0 },
            Bar { label: "b".into(), value: 3.0 },
        ]
    }

    fn at(origin_x: f32, origin_y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        bounds(point(px(origin_x), px(origin_y)), size(px(w), px(h)))
    }

    #[test]
    fn plot_area_honors_margins() {
        let t = Theme::default();
        let l = Layout::compute(at(0., 0., 400., 300.), &sample(), &t, None);
        assert_eq!(l.plot_area.origin.x, t.margin_left);
        assert_eq!(l.plot_area.origin.y, t.margin_top);
        assert_eq!(l.plot_area.size.width, px(400.) - t.margin_left - t.margin_right);
        assert_eq!(l.plot_area.size.height, px(300.) - t.margin_top - t.margin_bottom);
    }

    #[test]
    fn one_bar_rect_per_datum_on_baseline() {
        let l = Layout::compute(at(0., 0., 400., 300.), &sample(), &Theme::default(), None);
        assert_eq!(l.bars.len(), 2);
        let baseline = l.plot_area.origin.y + l.plot_area.size.height;
        for b in &l.bars {
            assert!(((b.origin.y + b.size.height) - baseline).abs() < px(0.5));
        }
        assert!(l.bars[1].size.height > l.bars[0].size.height);
    }

    #[test]
    fn bar_at_finds_and_misses() {
        let l = Layout::compute(at(0., 0., 400., 300.), &sample(), &Theme::default(), None);
        let b = l.bars[1];
        let inside = point(b.origin.x + b.size.width / 2., b.origin.y + b.size.height / 2.);
        assert_eq!(l.bar_at(inside), Some(1));
        assert_eq!(l.bar_at(point(px(1.), px(1.))), None);
    }

    #[test]
    fn empty_data_has_no_bars_but_has_ticks() {
        let l = Layout::compute(at(0., 0., 400., 300.), &[], &Theme::default(), None);
        assert!(l.bars.is_empty());
        assert!(!l.y_ticks.is_empty());
    }

    #[test]
    fn y_max_override_fixes_tick_basis() {
        let small = vec![Bar { label: "a".into(), value: 1.0 }];
        let big = vec![Bar { label: "a".into(), value: 9.0 }];
        let l1 = Layout::compute(at(0., 0., 400., 300.), &small, &Theme::default(), Some(10.0));
        let l2 = Layout::compute(at(0., 0., 400., 300.), &big, &Theme::default(), Some(10.0));
        let t1: Vec<f64> = l1.y_ticks.iter().map(|(v, _)| *v).collect();
        let t2: Vec<f64> = l2.y_ticks.iter().map(|(v, _)| *v).collect();
        assert_eq!(t1, t2); // same override => identical ticks regardless of data
        assert!(!t1.is_empty());
    }

    #[test]
    fn bar_value_above_y_max_clamps_to_plot() {
        // value 20 with y_max override 10 => bar fills the plot height, top at plot top.
        let bars = vec![Bar { label: "a".into(), value: 20.0 }];
        let l = Layout::compute(at(0., 0., 400., 300.), &bars, &Theme::default(), Some(10.0));
        let b = l.bars[0];
        let top = l.plot_area.origin.y;
        let h = l.plot_area.size.height;
        assert!((b.size.height - h).abs() < px(0.5));
        assert!((b.origin.y - top).abs() < px(0.5));
    }
}

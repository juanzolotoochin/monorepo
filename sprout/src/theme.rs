//! Visual theme: colors and sizes with sensible defaults.

use gpui::{px, rgb, Hsla, Pixels};

#[derive(Clone, Copy)]
pub struct Theme {
    pub bar: Hsla,
    pub bar_hover: Hsla,
    pub axis: Hsla,
    pub gridline: Hsla,
    pub label: Hsla,
    pub tooltip_bg: Hsla,
    pub tooltip_text: Hsla,
    pub font_size: Pixels,
    pub y_tick_count: usize,
    /// Plot margins (px): space reserved for labels/axes.
    pub margin_left: Pixels,
    pub margin_bottom: Pixels,
    pub margin_top: Pixels,
    pub margin_right: Pixels,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bar: rgb(0x3a6ea5).into(),
            bar_hover: rgb(0x5fa8e0).into(),
            axis: rgb(0x888888).into(),
            gridline: rgb(0xdddddd).into(),
            label: rgb(0x333333).into(),
            tooltip_bg: rgb(0x222222).into(),
            tooltip_text: rgb(0xffffff).into(),
            font_size: px(12.),
            y_tick_count: 5,
            margin_left: px(52.),
            margin_bottom: px(28.),
            margin_top: px(28.),
            margin_right: px(16.),
        }
    }
}

//! Sprout: a small, native charting library for gpui.
//!
//! v1 draws vertical bar charts with labeled axes, gridlines, and hover
//! tooltips, painted directly on a gpui `canvas`.

mod bar_chart;
mod data;
mod layout;
mod render;
mod scale;
mod theme;

pub use bar_chart::BarChart;
pub use data::Bar;
pub use theme::Theme;

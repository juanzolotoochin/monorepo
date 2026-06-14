//! Canvas painting for a computed `Layout`.

use gpui::{bounds, fill, point, px, size, Hsla, Pixels, TextRun, Window};

use crate::data::Bar;
use crate::layout::Layout;
use crate::theme::Theme;

/// Horizontal anchoring for a text label relative to its anchor x.
#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

/// Paint the whole chart for one frame.
pub fn paint(
    layout: &Layout,
    title: Option<&str>,
    x_label: Option<&str>,
    y_label: Option<&str>,
    theme: &Theme,
    hovered: Option<usize>,
    bars: &[Bar],
    tooltips: Option<&[String]>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let plot = layout.plot_area;
    let line_height = theme.font_size * 1.4;
    let baseline_y = plot.origin.y + plot.size.height;

    // Plot background.
    window.paint_quad(fill(plot, gpui::white()));

    // Horizontal gridlines + right-aligned Y tick labels.
    for (value, y) in &layout.y_ticks {
        let gridline = bounds(point(plot.origin.x, *y), size(plot.size.width, px(1.)));
        window.paint_quad(fill(gridline, theme.gridline));
        draw(
            window,
            cx,
            &format!("{value:.0}"),
            plot.origin.x - px(8.),
            *y - line_height / 2.,
            Align::Right,
            theme.label,
            theme.font_size,
        );
    }

    // Axis lines (left + bottom).
    window.paint_quad(fill(
        bounds(point(plot.origin.x, plot.origin.y), size(px(1.), plot.size.height)),
        theme.axis,
    ));
    window.paint_quad(fill(
        bounds(point(plot.origin.x, baseline_y), size(plot.size.width, px(1.))),
        theme.axis,
    ));

    // Bars (hovered one highlighted) + centered X labels (from the computed layout).
    for (i, (label, center_x)) in layout.x_labels.iter().enumerate() {
        if let Some(rect) = layout.bars.get(i) {
            let color = if Some(i) == hovered { theme.bar_hover } else { theme.bar };
            window.paint_quad(fill(*rect, color));
        }
        draw(window, cx, label, *center_x, baseline_y + px(6.), Align::Center, theme.label, theme.font_size);
    }

    // Title and axis captions.
    if let Some(title) = title {
        draw(window, cx, title, plot.origin.x, plot.origin.y - px(22.), Align::Left, theme.label, theme.font_size);
    }
    if let Some(x) = x_label {
        let cx_px = plot.origin.x + plot.size.width / 2.;
        draw(window, cx, x, cx_px, baseline_y + px(20.), Align::Center, theme.label, theme.font_size);
    }
    if let Some(y) = y_label {
        draw(window, cx, y, plot.origin.x - px(40.), plot.origin.y - px(22.), Align::Left, theme.label, theme.font_size);
    }

    // Tooltip for the hovered bar (caller text if provided, else "label: value").
    if let Some(i) = hovered {
        if let (Some(rect), Some(bar)) = (layout.bars.get(i), bars.get(i)) {
            let text = tooltips
                .and_then(|t| t.get(i))
                .cloned()
                .unwrap_or_else(|| format!("{}: {:.2}", bar.label, bar.value));
            let lines: Vec<&str> = text.lines().collect();
            let n = lines.len().max(1) as f32;
            let max_w = lines
                .iter()
                .map(|l| measure(window, l, theme.font_size))
                .fold(px(0.), |a, b| if b > a { b } else { a });
            let origin = point(rect.origin.x, rect.origin.y - line_height * n - px(8.));
            let tip = bounds(origin, size(max_w + px(10.), line_height * n + px(6.)));
            window.paint_quad(fill(tip, theme.tooltip_bg));
            for (j, l) in lines.iter().enumerate() {
                draw(
                    window,
                    cx,
                    l,
                    origin.x + px(5.),
                    origin.y + px(3.) + line_height * j as f32,
                    Align::Left,
                    theme.tooltip_text,
                    theme.font_size,
                );
            }
        }
    }
}

/// Width of `text` if shaped at `font_size` (color-independent).
fn measure(window: &mut Window, text: &str, font_size: Pixels) -> Pixels {
    if text.is_empty() {
        return px(0.);
    }
    shape(window, text, gpui::white(), font_size).width
}

/// Shape and paint one line of text, anchored horizontally per `align`.
#[allow(clippy::too_many_arguments)]
fn draw(
    window: &mut Window,
    cx: &mut gpui::App,
    text: &str,
    anchor_x: Pixels,
    top_y: Pixels,
    align: Align,
    color: Hsla,
    font_size: Pixels,
) {
    if text.is_empty() {
        return;
    }
    let shaped = shape(window, text, color, font_size);
    let x = match align {
        Align::Left => anchor_x,
        Align::Center => anchor_x - shaped.width / 2.,
        Align::Right => anchor_x - shaped.width,
    };
    let _ = shaped.paint(point(x, top_y), font_size * 1.4, window, cx);
}

fn shape(window: &mut Window, text: &str, color: Hsla, font_size: Pixels) -> gpui::ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.to_string().into(), font_size, &[run], None)
}

//! Bottom strip: thumbnails of the selected action's frames; click to scrub.

use crate::SpriteStudio;
use gpui::{div, img, prelude::*, px, rgb, Context, Window};

pub fn render(app: &mut SpriteStudio, _window: &mut Window, cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let current = app.state.current_frame;
    let dir = app.state.project_dir.clone();
    let frames: Vec<std::path::PathBuf> = app
        .state
        .selected()
        .map(|a| a.frames.clone())
        .unwrap_or_default();

    let mut strip = div()
        .flex()
        .flex_row()
        .h(px(96.))
        .gap_2()
        .px_2()
        .items_center()
        .overflow_x_hidden()
        .bg(rgb(0x202020));

    for (i, rel) in frames.into_iter().enumerate() {
        let abs = dir.as_ref().map(|d| d.join(&rel));
        let is_current = i == current;
        let mut cell = div()
            .id(("frame", i))
            .w(px(72.))
            .h(px(80.))
            .flex_none()
            .cursor_pointer()
            .border_2()
            .border_color(if is_current { rgb(0x3a6ea5) } else { rgb(0x202020) })
            .on_click(cx.listener(move |app, _, _, cx| {
                app.state.scrub_to(i);
                cx.notify();
            }));
        if let Some(abs) = abs {
            cell = cell.child(img(abs).max_w_full().max_h_full());
        }
        strip = strip.child(cell);
    }

    strip
}

//! Center stage: renders the current frame of the selected action.

use crate::SpriteStudio;
use gpui::{div, img, prelude::*, rgb, Context, Window};

pub fn render(app: &mut SpriteStudio, _window: &mut Window, _cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let frame_path = current_frame_path(app);

    let mut stage = div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(rgb(0x1a1a1a));

    if let Some(path) = frame_path {
        stage = stage.child(img(path).max_w_full().max_h_full());
    } else {
        stage = stage.child("Open or import a project to begin");
    }

    stage
}

fn current_frame_path(app: &SpriteStudio) -> Option<std::path::PathBuf> {
    let dir = app.state.project_dir.as_ref()?;
    let action = app.state.selected()?;
    let rel = action.frames.get(app.state.current_frame)?;
    Some(dir.join(rel))
}

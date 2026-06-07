//! Sprite Studio GPUI front-end.

mod frame_strip;
mod sidebar;
mod stage;
mod toolbar;

use gpui::{div, prelude::*, rgb, px, Context, Window};
use sprite_studio_core::state::AppState;

pub struct SpriteStudio {
    pub state: AppState,
}

impl SpriteStudio {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self { state: AppState::default() }
    }
}

impl Render for SpriteStudio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xffffff))
            .child(toolbar::render(self, window, cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.))
                    .child(sidebar::render(self, window, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .child(stage::render(self, window, cx))
                            .child(frame_strip::render(self, window, cx)),
                    ),
            )
    }
}

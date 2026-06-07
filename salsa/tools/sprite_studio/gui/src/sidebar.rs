//! Left sidebar: the list of actions in the open project.

use crate::SpriteStudio;
use gpui::{div, prelude::*, px, rgb, Context, Window};

pub fn render(app: &mut SpriteStudio, _window: &mut Window, cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let selected = app.state.selected_action;
    let names: Vec<String> = app
        .state
        .project
        .as_ref()
        .map(|p| p.actions.iter().map(|a| a.name.clone()).collect())
        .unwrap_or_default();

    let mut list = div()
        .flex()
        .flex_col()
        .w(px(180.))
        .h_full()
        .bg(rgb(0x252525))
        .py_2();

    for (i, name) in names.into_iter().enumerate() {
        let is_selected = selected == Some(i);
        list = list.child(
            div()
                .id(("action", i))
                .px_3()
                .py_1()
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgb(0x3a6ea5)))
                .child(name)
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.state.select_action(i);
                    cx.notify();
                })),
        );
    }

    list
}

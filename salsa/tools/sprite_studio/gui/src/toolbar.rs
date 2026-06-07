//! Top toolbar: Open · Import · Export · Play/Pause · FPS.

use crate::SpriteStudio;
use gpui::{div, prelude::*, px, rgb, Context, Div, Stateful, Window};
use sprite_studio_core::{import::import_directory, project::Project, resolver::HeuristicResolver};
use std::path::PathBuf;

fn button(id: &'static str, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .mr_2()
        .rounded_md()
        .bg(rgb(0x3a3a3a))
        .cursor_pointer()
        .child(label.to_string())
}

pub fn render(app: &mut SpriteStudio, _window: &mut Window, cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let playing = app.state.playing;
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(40.))
        .px_2()
        .bg(rgb(0x252525))
        .child(button("open", "Open").on_click(cx.listener(|_, _, _, cx| open_project(cx))))
        .child(button("import", "Import").on_click(cx.listener(|_, _, _, cx| import(cx))))
        .child(button("export", "Export")) // wired in Task 13
        .child(
            button("play", if playing { "Pause" } else { "Play" })
                .on_click(cx.listener(|app, _, _, cx| {
                    app.state.toggle_play();
                    cx.notify();
                })),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .ml_4()
                .child(
                    div().id("fps_down").px_2().cursor_pointer().child("-").on_click(
                        cx.listener(|app, _, _, cx| {
                            let fps = app.state.current_fps().saturating_sub(1);
                            app.state.set_selected_fps(fps);
                            cx.notify();
                        }),
                    ),
                )
                .child(div().px_2().child(format!("FPS: {}", app.state.current_fps())))
                .child(
                    div().id("fps_up").px_2().cursor_pointer().child("+").on_click(
                        cx.listener(|app, _, _, cx| {
                            let fps = app.state.current_fps() + 1;
                            app.state.set_selected_fps(fps);
                            cx.notify();
                        }),
                    ),
                ),
        )
}

/// Prompts for a directory and opens it as a project.
fn open_project(cx: &mut Context<SpriteStudio>) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(dirs))) = paths.await {
            if let Some(dir) = dirs.into_iter().next() {
                if let Ok(project) = Project::open(&dir) {
                    let _ = this.update(cx, |this, cx| {
                        this.state.set_project(project, dir);
                        cx.notify();
                    });
                }
            }
        }
    })
    .detach();
}

/// Prompts for a source directory and imports it into the current project dir.
fn import(cx: &mut Context<SpriteStudio>) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(dirs))) = paths.await {
            if let Some(source) = dirs.into_iter().next() {
                let project_dir: Option<PathBuf> =
                    this.read_with(cx, |this, _| this.state.project_dir.clone()).ok().flatten();
                let project_dir = project_dir.unwrap_or_else(|| source.clone());
                if import_directory(&project_dir, &source, &HeuristicResolver).is_ok() {
                    if let Ok(project) = Project::open(&project_dir) {
                        let _ = this.update(cx, |this, cx| {
                            this.state.set_project(project, project_dir);
                            cx.notify();
                        });
                    }
                }
            }
        }
    })
    .detach();
}

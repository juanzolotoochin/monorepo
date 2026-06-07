//! SPIKE body for the Sprite Studio GUI (Task 8).
//!
//! This is a temporary file that exists solely to confirm the gpui 0.2.2 APIs
//! the real app will be built on. It is replaced in a later task. Do NOT run it
//! here (it opens a blocking window); verification is `bazel build` only.
//!
//! Every API used below was confirmed against the gpui 0.2.2 source. See
//! `salsa/tools/sprite_studio/SPIKE_NOTES.md` for the exact signatures.

use gpui::{div, img, prelude::*, px, rgb, Context, SharedString, Timer, Window};
use std::time::Duration;

pub struct Spike {
    pub tick: usize,
    pub picked: SharedString,
    pub image_path: SharedString,
}

impl Spike {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Redraw/advance timer. `gpui::Timer` is a re-export of `smol::Timer`
        // and `Timer::after(Duration) -> Timer` is itself a Future.
        // `cx.spawn` takes an `AsyncFnOnce(WeakEntity<Self>, &mut AsyncApp) -> R`,
        // i.e. an `async move |this, cx|` closure where `cx: &mut AsyncApp`.
        cx.spawn(async move |this, cx| loop {
            Timer::after(Duration::from_millis(500)).await;
            if this
                .update(cx, |this, cx| {
                    this.tick += 1;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        })
        .detach();

        Self {
            tick: 0,
            picked: "(none)".into(),
            image_path: "/home/juanique/Downloads/freeknight/png/Idle (1).png".into(),
        }
    }
}

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .bg(rgb(0x2e2e2e))
            .size_full()
            .text_color(rgb(0xffffff))
            .child(format!("tick: {}", self.tick))
            .child(format!("picked: {}", self.picked))
            // `img()` takes `impl Into<ImageSource>`; `ImageSource: From<PathBuf>`,
            // so a local file path can be passed directly. `Img: Styled`, so the
            // sizing helpers `.w()` / `.h()` are available.
            .child(
                img(std::path::PathBuf::from(self.image_path.to_string()))
                    .w(px(200.))
                    .h(px(200.)),
            )
            .child(
                // `on_click` is an `InteractiveElement` method requiring a stateful
                // element, hence `.id("pick")`. The listener it wants is
                // `Fn(&ClickEvent, &mut Window, &mut App)`; `cx.listener` adapts a
                // 4-arg closure `|this, event, window, cx|` to that shape.
                div()
                    .id("pick")
                    .child("Pick a directory")
                    .on_click(cx.listener(|_this, _event, _window, cx| {
                        // `prompt_for_paths` is reachable on `Context` via Deref to
                        // `App`. It returns `oneshot::Receiver<Result<Option<Vec<PathBuf>>>>`,
                        // so awaiting yields `Result<Result<Option<Vec<PathBuf>>>, Canceled>`
                        // => the `Ok(Ok(Some(paths)))` match below.
                        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                            // gpui 0.2.2 added a 4th field vs the draft spec:
                            // `prompt: Option<SharedString>` (the dialog's confirm label).
                            prompt: None,
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = paths.await {
                                if let Some(p) = paths.first() {
                                    let p = p.display().to_string();
                                    let _ = this.update(cx, |this, cx| {
                                        this.picked = p.into();
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .detach();
                    })),
            )
    }
}

use gpui::{px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use sprite_studio_gui::SpriteStudio;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| SpriteStudio::new(cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}

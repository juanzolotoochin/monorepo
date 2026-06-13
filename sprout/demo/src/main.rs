use gpui::{px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use sprout::BarChart;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(720.), px(480.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| {
                    BarChart::new()
                        .title("Weekly sales")
                        .x_label("Day")
                        .y_label("Units")
                        .data([
                            ("Mon", 3.2),
                            ("Tue", 5.1),
                            ("Wed", 2.4),
                            ("Thu", 6.7),
                            ("Fri", 4.3),
                            ("Sat", 7.9),
                            ("Sun", 1.8),
                        ])
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

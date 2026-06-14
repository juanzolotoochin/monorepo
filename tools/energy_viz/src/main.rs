mod data;

use data::{hour_tooltip, Dataset, Metric};
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Div, Entity, Stateful,
    Window, WindowBounds, WindowOptions,
};
use sprout::BarChart;

const DEFAULT_CSV: &str = "/home/juanique/Downloads/pge-90dfb574-174b-11ef-9e9a-000017009d23-DailyUsageData/electricity.csv";

struct EnergyViz {
    dataset: Dataset,
    current_day: usize,
    metric: Metric,
    y_max: f64,
    dropdown_open: bool,
    chart: Entity<BarChart>,
}

fn button(id: &'static str, label: impl Into<String>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .mr_2()
        .rounded_md()
        .bg(rgb(0x3a3a3a))
        .cursor_pointer()
        .child(label.into())
}

/// Build a fresh chart for the given day/metric, pinned to `y_max`, with a
/// per-hour tooltip showing kWh, price, and unit rate.
fn build_chart(
    dataset: &Dataset,
    day: usize,
    metric: Metric,
    y_max: f64,
    cx: &mut Context<EnergyViz>,
) -> Entity<BarChart> {
    let d = &dataset.days[day];
    let data: Vec<(String, f64)> =
        (0..24).map(|h| (h.to_string(), d.hours[h].value(metric))).collect();
    let tips: Vec<String> =
        (0..24).map(|h| hour_tooltip(h, d.hours[h].kwh, d.hours[h].cost)).collect();
    let title = d.date.clone();
    cx.new(|_| {
        BarChart::new()
            .data(data)
            .tooltips(tips)
            .title(title)
            .x_label("Hour")
            .y_label(metric.label())
            .y_max(y_max)
    })
}

impl EnergyViz {
    fn new(dataset: Dataset, cx: &mut Context<Self>) -> Self {
        let y_max = dataset.max_hourly(Metric::Kwh);
        let chart = build_chart(&dataset, 0, Metric::Kwh, y_max, cx);
        Self { dataset, current_day: 0, metric: Metric::Kwh, y_max, dropdown_open: false, chart }
    }

    fn rebuild_chart(&mut self, cx: &mut Context<Self>) {
        self.chart = build_chart(&self.dataset, self.current_day, self.metric, self.y_max, cx);
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let day = &self.dataset.days[self.current_day];
        let last = self.dataset.days.len() - 1;
        let metric = self.metric;

        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(48.))
            .px_3()
            .gap_2()
            .bg(rgb(0x252525))
            .child(button("prev", "\u{25C0} Prev").on_click(cx.listener(|app, _, _, cx| {
                app.current_day = app.current_day.saturating_sub(1);
                app.dropdown_open = false;
                app.rebuild_chart(cx);
                cx.notify();
            })))
            .child(
                button("day_select", format!("{} \u{25BE}", day.date)).on_click(cx.listener(
                    |app, _, _, cx| {
                        app.dropdown_open = !app.dropdown_open;
                        cx.notify();
                    },
                )),
            )
            .child(button("next", "Next \u{25B6}").on_click(cx.listener(move |app, _, _, cx| {
                if app.current_day < last {
                    app.current_day += 1;
                }
                app.dropdown_open = false;
                app.rebuild_chart(cx);
                cx.notify();
            })))
            .child(
                div()
                    .flex_1()
                    .child(format!("Total: {:.1} {}", day.total(metric), metric.label())),
            )
            .child(button("ymin", "Y \u{2212}").on_click(cx.listener(|app, _, _, cx| {
                app.y_max = (app.y_max * 0.8).max(0.1);
                app.rebuild_chart(cx);
                cx.notify();
            })))
            .child(button("ymax", "Y +").on_click(cx.listener(|app, _, _, cx| {
                app.y_max *= 1.25;
                app.rebuild_chart(cx);
                cx.notify();
            })))
            .child(button("toggle", format!("Show: {}", metric.label())).on_click(
                cx.listener(|app, _, _, cx| {
                    app.metric = match app.metric {
                        Metric::Kwh => Metric::Cost,
                        Metric::Cost => Metric::Kwh,
                    };
                    app.y_max = app.dataset.max_hourly(app.metric);
                    app.rebuild_chart(cx);
                    cx.notify();
                }),
            ))
    }

    /// A scrollable list of every date, overlaid under the day-select button.
    fn day_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current_day;
        let mut list = div()
            .id("day_list")
            .absolute()
            .top(px(52.))
            .left(px(110.))
            .w(px(150.))
            .max_h(px(320.))
            .overflow_y_scroll()
            .bg(rgb(0x2a2a2a))
            .border_1()
            .border_color(rgb(0x444444))
            .rounded_md()
            .shadow_lg();

        for (i, day) in self.dataset.days.iter().enumerate() {
            let selected = i == current;
            list = list.child(
                div()
                    .id(("day_item", i))
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .bg(if selected { rgb(0x3a6ea5) } else { rgb(0x2a2a2a) })
                    .hover(|s| s.bg(rgb(0x3a3a3a)))
                    .child(day.date.clone())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.current_day = i;
                        app.dropdown_open = false;
                        app.rebuild_chart(cx);
                        cx.notify();
                    })),
            );
        }
        list
    }
}

impl Render for EnergyViz {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xffffff))
            .child(header)
            .child(div().flex_1().child(self.chart.clone()));
        if self.dropdown_open {
            root = root.child(self.day_dropdown(cx));
        }
        root
    }
}

/// Minimal view that just shows a load error message.
struct ErrorView {
    message: String,
}

impl Render for ErrorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xff6b6b))
            .items_center()
            .justify_center()
            .p_8()
            .child(self.message.clone())
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_CSV.to_string());
    let loaded = Dataset::from_path(&path);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.), px(560.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        match loaded {
            Ok(dataset) => {
                cx.open_window(options, |_, cx| cx.new(|cx| EnergyViz::new(dataset, cx))).unwrap();
            }
            Err(message) => {
                cx.open_window(options, |_, cx| cx.new(|_| ErrorView { message })).unwrap();
            }
        }
        cx.activate(true);
    });
}

//! The BarChart view and its fluent builder.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{canvas, div, prelude::*, Context, Hsla, MouseMoveEvent, Window};

use crate::data::Bar;
use crate::theme::Theme;

pub struct BarChart {
    data: Rc<Vec<Bar>>,
    title: Option<String>,
    x_label: Option<String>,
    y_label: Option<String>,
    theme: Theme,
    hovered: Option<usize>,
    geometry: Rc<RefCell<Option<crate::layout::Layout>>>,
}

impl BarChart {
    pub fn new() -> Self {
        Self {
            data: Rc::new(Vec::new()),
            title: None,
            x_label: None,
            y_label: None,
            theme: Theme::default(),
            hovered: None,
            geometry: Rc::new(RefCell::new(None)),
        }
    }

    pub fn data(mut self, data: impl IntoIterator<Item = (impl Into<String>, f64)>) -> Self {
        self.data = Rc::new(
            data.into_iter()
                .map(|(label, value)| Bar { label: label.into(), value })
                .collect(),
        );
        self
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    pub fn x_label(mut self, t: impl Into<String>) -> Self {
        self.x_label = Some(t.into());
        self
    }

    pub fn y_label(mut self, t: impl Into<String>) -> Self {
        self.y_label = Some(t.into());
        self
    }

    pub fn bar_color(mut self, c: impl Into<Hsla>) -> Self {
        self.theme.bar = c.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Default for BarChart {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for BarChart {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Captured by value into the 'static canvas closures.
        let data = self.data.clone();
        let theme = self.theme;
        let hovered = self.hovered;
        let title = self.title.clone();
        let x_label = self.x_label.clone();
        let y_label = self.y_label.clone();

        let prepaint_data = data.clone();
        let prepaint_geometry = self.geometry.clone();

        div()
            .relative()
            .size_full()
            .bg(gpui::white())
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _window, cx| {
                let hit = this
                    .geometry
                    .borrow()
                    .as_ref()
                    .and_then(|l| l.bar_at(ev.position));
                if hit != this.hovered {
                    this.hovered = hit;
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |area, _window, _cx| {
                        let layout = crate::layout::Layout::compute(area, &prepaint_data, &theme);
                        *prepaint_geometry.borrow_mut() = Some(layout.clone());
                        layout
                    },
                    move |_area, layout, window, cx| {
                        crate::render::paint(
                            &layout,
                            title.as_deref(),
                            x_label.as_deref(),
                            y_label.as_deref(),
                            &theme,
                            hovered,
                            &data,
                            window,
                            cx,
                        );
                    },
                )
                .size_full(),
            )
    }
}

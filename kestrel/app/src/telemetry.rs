//! Bounded performance samples. CPU wall time is kept separate from deterministic game time.
mod network;
pub use network::{LaserOutcome, NetworkFrame, PreviewUpdate};
#[cfg(test)]
pub use network::CorrectionKind;
use bevy::prelude::*;
use debug_ui::{egui, GameInput, ImguiManager, ImguiMenu, MenuHandle, MenuId, MenuSpec};
use serde_json::{json, Value};
use service::{AppLayer, Service};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};
use web_time::Instant;
const WINDOW: usize = 240;
#[derive(Default)]
struct Samples {
    values: VecDeque<f64>,
    count: u64,
    total: f64,
    max: f64,
}
impl Samples {
    fn push(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.count += 1;
        self.total += value;
        self.max = self.max.max(value);
        if self.values.len() == WINDOW {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }
    /// The mean and 95th percentile over the window.
    fn recent(&self) -> (f64, f64) {
        if self.values.is_empty() {
            return (0., 0.);
        }
        let mut v: Vec<_> = self.values.iter().copied().collect();
        v.sort_by(f64::total_cmp);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        (mean, v[((v.len() - 1) as f64 * 0.95).round() as usize])
    }
    fn snapshot(&self) -> Value {
        let mut v: Vec<_> = self.values.iter().copied().collect();
        v.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            v.get(((v.len().saturating_sub(1)) as f64 * p).round() as usize)
                .copied()
                .unwrap_or(0.)
        };
        json!({"samples":self.count,"last_ms":self.values.back(),"mean_ms":self.total/self.count.max(1) as f64,"p50_ms":percentile(0.5),"p95_ms":percentile(0.95),"p99_ms":percentile(0.99),"max_ms":self.max,"window":v.len()})
    }
}
#[derive(Resource, Default)]
pub struct Telemetry {
    pub network: network::NetworkDiagnostics,
    samples: BTreeMap<&'static str, Samples>,
    pub counters: BTreeMap<&'static str, u64>,
    pub gauges: BTreeMap<&'static str, f64>,
    pub enabled_overlay: bool,
    started: Option<Instant>,
    previous: Option<Instant>,
    overlay: Option<Entity>,
}
impl Telemetry {
    pub fn network_frame(&mut self, mut frame: NetworkFrame) {
        frame.frame_ms = self.samples.get("frame_wall").and_then(|samples| samples.values.back().copied());
        self.network.record(frame);
    }

    pub fn sample(&mut self, name: &'static str, ms: f64) {
        self.samples.entry(name).or_default().push(ms);
    }
    pub fn count(&mut self, name: &'static str, value: u64) {
        *self.counters.entry(name).or_default() += value;
    }
    pub fn gauge(&mut self, name: &'static str, value: f64) {
        self.gauges.insert(name, value);
    }
    /// One frame's session update (network and every tick due), and each
    /// tick's stages plus copying its settlement for the view.
    pub fn session_frame(&mut self, update: Duration, ticks: impl Iterator<Item = (sim::StageTimes, Duration)>) {
        let ms = |d: Duration| d.as_secs_f64() * 1000.;
        self.sample("session_update", ms(update));
        let mut count = 0;
        for (times, copy) in ticks {
            count += 1;
            for (name, d) in TICK_STAGES.iter().map(|(name, _)| *name).zip(stages(&times)) {
                self.sample(name, ms(d));
            }
            self.sample("tick_total", ms(times.total()));
            self.sample("tick_copy", ms(copy));
        }
        self.gauge("ticks_last_frame", count as f64);
        // A count, not ms: kept as a sample for its average.
        self.sample("ticks_per_frame", count as f64);
    }
    /// A sample's mean and 95th percentile over the window, in ms.
    fn recent(&self, name: &str) -> (f64, f64) {
        self.samples.get(name).map_or((0., 0.), Samples::recent)
    }
}
/// The world tick's stages: sample name and overlay label.
const TICK_STAGES: [(&str, &str); 7] = [
    ("tick_commands", "commands"),
    ("tick_self_update", "self update"),
    ("tick_read", "read"),
    ("tick_merge", "merge"),
    ("tick_apply", "apply"),
    ("tick_housekeeping", "housekeeping"),
    ("tick_hash", "hash"),
];
fn stages(t: &sim::StageTimes) -> [Duration; 7] {
    [t.commands, t.self_update, t.read, t.merge, t.apply, t.housekeeping, t.hash]
}
pub fn elapsed(world: &mut World, name: &'static str, start: Instant) {
    world
        .resource_mut::<Telemetry>()
        .sample(name, start.elapsed().as_secs_f64() * 1000.);
}
/// Times the frame from its start: runs first in `First`.
pub fn begin(world: &mut World) {
    let now = Instant::now();
    let mut t = world.resource_mut::<Telemetry>();
    if let Some(previous) = t.previous {
        let ms = now.duration_since(previous).as_secs_f64() * 1000.;
        t.sample("frame_wall", ms);
        if ms > 16.667 {
            t.count("frames_over_16_7ms", 1);
        }
        if ms > 33.334 {
            t.count("frames_over_33_3ms", 1);
        }
    }
    t.previous = Some(now);
    t.started = Some(now);
    t.count("frames", 1);
}
pub fn end(world: &mut World) {
    let start = world.resource::<Telemetry>().started;
    if let Some(start) = start {
        elapsed(world, "update_cpu", start);
    }
    let entity_slots = world.entities().len();
    let entities = world
        .archetypes()
        .iter()
        .map(|a| a.len() as usize)
        .sum::<usize>();
    let meshes = world.resource::<Assets<Mesh>>().len();
    let materials = world.resource::<Assets<StandardMaterial>>().len();
    let cache = crate::style::cache_metrics(world);
    let mut t = world.remove_resource::<Telemetry>().unwrap();
    t.gauge("entities", entities as f64);
    t.gauge("allocated_entity_slots", entity_slots as f64);
    t.gauge("mesh_assets", meshes as f64);
    t.gauge("material_assets", materials as f64);
    t.gauge("style_cache_entries", cache.0 as f64);
    t.gauge("style_palette_cache_entries", cache.1 as f64);
    // Frames per second always show in a window; F3 adds the breakdown.
    let options = world.resource::<crate::control::Options>();
    let shown = !options.simulation && options.frames_dir.is_none();
    if shown {
        let e = *t.overlay.get_or_insert_with(|| {
            world
                .spawn((
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(12.),
                        ..default()
                    },
                    TextColor(Color::srgb(0.7, 0.95, 0.75)),
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(36),
                        top: px(135),
                        ..default()
                    },
                ))
                .id()
        });
        let text = overlay_text(&t, entities, meshes);
        let mut current = world.get_mut::<Text>(e).unwrap();
        if current.0 != text {
            current.0 = text;
        }
    }
    world.insert_resource(t);
}
/// The FPS line, and with F3 the frame and world tick breakdown (mean and
/// 95th percentile over the window).
fn overlay_text(t: &Telemetry, entities: usize, meshes: usize) -> String {
    let (frame, frame_p95) = t.recent("frame_wall");
    let fps = if frame > 0. { 1000. / frame } else { 0. };
    let mut text = format!("{fps:.0} FPS / {frame:.1} ms frame / p95 {frame_p95:.1}");
    if !t.enabled_overlay {
        return text + "  [F3]";
    }
    let (update, _) = t.recent("update_cpu");
    let (session, session_p95) = t.recent("session_update");
    let (ticks, _) = t.recent("ticks_per_frame");
    let (total, total_p95) = t.recent("tick_total");
    let render = (frame - update).max(0.);
    text += &format!("\nCPU {update:.1} ms update / {render:.1} ms render and wait");
    text += &format!("\nSESSION {session:.2} ms / p95 {session_p95:.2} / {ticks:.2} ticks per frame");
    text += &network_line(t, Instant::now());
    text += &format!("\n\nWORLD TICK {total:.2} ms / p95 {total_p95:.2}");
    for (name, label) in TICK_STAGES.iter().chain(&[("tick_copy", "copy for view")]) {
        let (mean, p95) = t.recent(name);
        text += &format!("\n   {label}  {mean:.2} / {p95:.2}");
    }
    text += &format!(
        "\n\n{entities} entities / {meshes} meshes\n{} active rigs / {} pooled",
        t.gauges.get("active_rigs").unwrap_or(&0.),
        t.gauges.get("pooled_rigs").unwrap_or(&0.)
    );
    text
}
/// The round trip to the host, smoothed and latest, and the input margin
/// during a networked game. Only clients measure them.
fn network_line(t: &Telemetry, now: Instant) -> String {
    let Some(frame) = t.network.current(now) else {
        return String::new();
    };
    let ms = |d: Option<Duration>| d.map_or("--".into(), |d| format!("{:.0}", d.as_secs_f64() * 1000.));
    let margin = net::TICK * frame.diagnostics.input_delay as u32;
    match frame.role {
        "Client" => format!(
            "\nNET RTT {} ms / last {} · margin {} ms",
            ms(frame.stats.rtt),
            ms(frame.diagnostics.rtt_sample),
            ms(Some(margin))
        ),
        _ => "\nNET host / RTT is measured by clients".into(),
    }
}
/// F3 adds the breakdown to the HUD line, unless the debug UI has the key.
pub fn shortcut(input: Res<GameInput>, mut telemetry: ResMut<Telemetry>) {
    if input.keys().just_pressed(KeyCode::F3) {
        telemetry.enabled_overlay = !telemetry.enabled_overlay;
    }
}

/// Recent movement preview corrections, when Diagnostics → Network asks for them.
pub fn draw_preview_corrections(telemetry: Res<Telemetry>, mut gizmos: Gizmos) {
    telemetry.network.draw_corrections(Instant::now(), &mut gizmos);
}

/// The debug menu's Diagnostics windows, drawn from the live samples. They
/// read what the last completed frames recorded.
const MENUS: &[MenuSpec] = &[
    MenuSpec::new("frame_timing", "Diagnostics", "Frame timing"),
    MenuSpec::new("counters", "Diagnostics", "Counters and gauges"),
    MenuSpec::new("network", "Diagnostics", "Network"),
];

impl ImguiMenu for Telemetry {
    fn menus() -> &'static [MenuSpec] {
        MENUS
    }

    fn show_menu(&mut self, id: MenuId, root: &mut egui::Ui, open: &mut bool, world: &mut World) {
        let window = id.window().open(open);
        match id.id() {
            "network" => window.default_width(520.0).vscroll(true).show(root.ctx(), |ui| {
                let active = world.get_resource::<crate::lockstep::SessionService>().is_some_and(|service| service.networked());
                self.network.show(ui, active);
            }),
            "frame_timing" => window.default_pos([16.0, 40.0]).default_width(330.0).show(root.ctx(), |ui| self.frame_timing(ui)),
            _ => window.default_pos([360.0, 40.0]).default_width(280.0).show(root.ctx(), |ui| self.counters(ui)),
        };
    }
}

impl Telemetry {
    fn frame_timing(&self, ui: &mut egui::Ui) {
        let (frame, frame_p95) = self.recent("frame_wall");
        let fps = if frame > 0. { 1000. / frame } else { 0. };
        let (update, update_p95) = self.recent("update_cpu");
        let (session, session_p95) = self.recent("session_update");
        let (ticks, _) = self.recent("ticks_per_frame");
        let (total, total_p95) = self.recent("tick_total");
        ui.label(format!("{fps:.0} FPS, mean and p95 over the last {WINDOW} frames"));
        self.frame_graph(ui);
        egui::Grid::new("frame_timing_grid").num_columns(3).striped(true).show(ui, |ui| {
            let mut row = |label: &str, mean: f64, p95: f64| {
                ui.label(label);
                ui.monospace(format!("{mean:7.2} ms"));
                ui.monospace(format!("{p95:7.2} ms"));
                ui.end_row();
            };
            row("Frame (wall)", frame, frame_p95);
            row("CPU update", update, update_p95);
            row("Render and wait", (frame - update).max(0.), (frame_p95 - update_p95).max(0.));
            row("Session update", session, session_p95);
            row("World tick", total, total_p95);
            for (name, label) in TICK_STAGES.iter().chain(&[("tick_copy", "copy for view")]) {
                let (mean, p95) = self.recent(name);
                row(&format!("   {label}"), mean, p95);
            }
        });
        ui.label(format!("{ticks:.2} ticks per frame"));
        let over = |name| self.counters.get(name).copied().unwrap_or(0);
        ui.label(format!("{} frames over 16.7 ms, {} over 33.3 ms", over("frames_over_16_7ms"), over("frames_over_33_3ms")));
    }

    /// The recent frame times as bars, with 60 and 30 FPS marks.
    fn frame_graph(&self, ui: &mut egui::Ui) {
        let Some(samples) = self.samples.get("frame_wall") else { return };
        let width = ui.available_width().max(120.0);
        let (response, painter) = ui.allocate_painter(egui::vec2(width, 64.0), egui::Sense::hover());
        let rect = response.rect;
        let ceiling = 50.0_f32;
        let y = |ms: f32| rect.bottom() - (ms.min(ceiling) / ceiling) * rect.height();
        painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(90));
        let step = rect.width() / WINDOW as f32;
        for (i, &ms) in samples.values.iter().enumerate() {
            let x = rect.left() + i as f32 * step;
            let ms = ms as f32;
            let color = if ms > 33.334 {
                egui::Color32::from_rgb(235, 90, 70)
            } else if ms > 16.667 {
                egui::Color32::from_rgb(235, 175, 70)
            } else {
                egui::Color32::from_rgb(110, 200, 140)
            };
            painter.line_segment([egui::pos2(x, rect.bottom()), egui::pos2(x, y(ms))], egui::Stroke::new(step.max(1.0), color));
        }
        for (ms, label) in [(16.667, "60"), (33.334, "30")] {
            let mark = y(ms);
            painter.line_segment([egui::pos2(rect.left(), mark), egui::pos2(rect.right(), mark)], egui::Stroke::new(1.0, egui::Color32::from_white_alpha(60)));
            painter.text(egui::pos2(rect.right() - 2.0, mark), egui::Align2::RIGHT_BOTTOM, label, egui::FontId::monospace(10.0), egui::Color32::from_white_alpha(140));
        }
    }

    fn counters(&self, ui: &mut egui::Ui) {
        egui::Grid::new("telemetry_counters").num_columns(2).striped(true).show(ui, |ui| {
            for (name, value) in &self.counters {
                ui.label(*name);
                ui.monospace(value.to_string());
                ui.end_row();
            }
            for (name, value) in &self.gauges {
                ui.label(*name);
                ui.monospace(format!("{value:.0}"));
                ui.end_row();
            }
        });
    }
}

/// Owns the telemetry windows' registration (App layer). `Telemetry` itself
/// is an ordinary resource that lives for the whole run.
#[derive(Resource)]
pub struct TelemetryPanels {
    menu: Option<MenuHandle>,
}

impl Service for TelemetryPanels {
    type Layer = AppLayer;
    type Deps = (ImguiManager,);

    fn init(world: &mut World) -> Self {
        let menu = ImguiManager::register::<Telemetry>(world).map_err(|e| warn!("{e}")).ok();
        TelemetryPanels { menu }
    }

    fn shutdown(self, world: &mut World) {
        if let Some(menu) = self.menu {
            ImguiManager::unregister(world, menu);
        }
    }
}

pub fn snapshot(world: &World) -> Value {
    let t = world.resource::<Telemetry>();
    let timings: BTreeMap<_, _> = t.samples.iter().map(|(k, v)| (*k, v.snapshot())).collect();
    let diagnostics:Vec<_>=world.get_resource::<bevy::diagnostic::DiagnosticsStore>().map(|d|d.iter().filter_map(|d|d.value().map(|v|json!({"path":d.path().to_string(),"value":v,"average":d.average(),"smoothed":d.smoothed(),"unit":d.suffix}))).collect()).unwrap_or_default();
    json!({"version":1,"timings":timings,"counters":t.counters,"gauges":t.gauges,"network":t.network.snapshot(),"render_diagnostics":diagnostics,"timing_scope":"CPU wall time; frame_wall includes rendering/pacing/readback. GPU durations only when reported by render diagnostics.","rolling_window":WINDOW})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_are_bounded_and_percentiles_do_not_hide_hitches() {
        let mut s = Samples::default();
        for i in 1..=1000 {
            s.push(i as f64);
        }
        s.push(f64::NAN);
        let v = s.snapshot();
        assert_eq!(v["samples"], 1000);
        assert_eq!(v["window"], 240);
        assert_eq!(v["max_ms"], 1000.);
        assert!(v["p95_ms"].as_f64().unwrap() > 980.);
    }
    #[test]
    fn network_line_shows_client_rtt_and_margin_only_while_the_session_records() {
        let now = Instant::now();
        let frame = |role| NetworkFrame {
            at: now,
            role,
            status: "playing".into(),
            world: 1,
            tick: Some(12),
            pending: 0,
            stats: net::NetStats { rtt: Some(Duration::from_millis(152)), ..default() },
            diagnostics: net::NetDiagnostics { rtt_sample: Some(Duration::from_millis(160)), input_delay: 7, ..default() },
            update_ms: 1.,
            tick_ms: None,
            frame_ms: None,
        };
        let mut t = Telemetry::default();
        assert_eq!(network_line(&t, now), "");
        t.network.record(frame("Client"));
        assert_eq!(network_line(&t, now), "\nNET RTT 152 ms / last 160 · margin 117 ms");
        assert_eq!(network_line(&t, now + Duration::from_secs(5)), "", "history after the session is not live");
        t.network.record(frame("Host"));
        assert_eq!(network_line(&t, now), "\nNET host / RTT is measured by clients");
    }
}

//! Bounded performance samples. CPU wall time is kept separate from deterministic game time.
use bevy::prelude::*;
use serde_json::{json, Value};
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
    samples: BTreeMap<&'static str, Samples>,
    pub counters: BTreeMap<&'static str, u64>,
    pub gauges: BTreeMap<&'static str, f64>,
    pub enabled_overlay: bool,
    started: Option<Instant>,
    previous: Option<Instant>,
    overlay: Option<Entity>,
}
impl Telemetry {
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
pub fn begin(world: &mut World) {
    let now = Instant::now();
    let toggle = world
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::F3);
    let mut t = world.resource_mut::<Telemetry>();
    if toggle {
        t.enabled_overlay = !t.enabled_overlay;
    }
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
pub fn snapshot(world: &World) -> Value {
    let t = world.resource::<Telemetry>();
    let timings: BTreeMap<_, _> = t.samples.iter().map(|(k, v)| (*k, v.snapshot())).collect();
    let diagnostics:Vec<_>=world.get_resource::<bevy::diagnostic::DiagnosticsStore>().map(|d|d.iter().filter_map(|d|d.value().map(|v|json!({"path":d.path().to_string(),"value":v,"average":d.average(),"smoothed":d.smoothed(),"unit":d.suffix}))).collect()).unwrap_or_default();
    json!({"version":1,"timings":timings,"counters":t.counters,"gauges":t.gauges,"render_diagnostics":diagnostics,"timing_scope":"CPU wall time; frame_wall includes rendering/pacing/readback. GPU durations only when reported by render diagnostics.","rolling_window":WINDOW})
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
}

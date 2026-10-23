//! Bounded performance samples. CPU wall time is kept separate from deterministic game time.
use bevy::prelude::*;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};
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
    if t.enabled_overlay {
        let e = *t.overlay.get_or_insert_with(|| {
            world
                .spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 12.,
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
        let line = |name: &str| {
            t.samples
                .get(name)
                .map(|s| {
                    format!(
                        "{name} {:.2} ms p95",
                        s.snapshot()["p95_ms"].as_f64().unwrap()
                    )
                })
                .unwrap_or_default()
        };
        world.get_mut::<Text>(e).unwrap().0=format!("F3 / PERFORMANCE\n{}\n{}\n{}\n{entities} entities / {meshes} meshes\n{} active rigs / {} pooled",line("frame_wall"),line("update_cpu"),line("settlement_simulation"),t.gauges.get("active_rigs").unwrap_or(&0.),t.gauges.get("pooled_rigs").unwrap_or(&0.));
        world.entity_mut(e).insert(Visibility::Inherited);
    } else if let Some(e) = t.overlay {
        world.entity_mut(e).insert(Visibility::Hidden);
    }
    world.insert_resource(t);
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

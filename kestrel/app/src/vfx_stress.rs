//! Debug only: the VFX spike's burst stress harness (step 4 of
//! `kestrel/VFX_SPIKE_HANDOFF.md`). `--vfx-stress <spec>` fires effects at
//! fixed rates around the player and records frame, CPU, GPU and memory
//! numbers, then exits. Remove this file and its two hooks (`Options::vfx_stress`,
//! the plugin in `main.rs`) when the spike's numbers are no longer needed.
//!
//! The spec is comma-separated `key=value`s, all optional:
//! `impacts=50` stone impacts per second, `lasers=8` players firing laser
//! bolts at the laser's cooldown (2.5 shots/s each), `arcs=8` arcs per second,
//! `blood=50` cuts per second (`effects::blood`: droplets, and floor splats
//! that last 3 s),
//! `frames=4200` frames to run, `warmup=300` frames left out of the stats,
//! `out=path` (writes `path.json` with the summary and `path.csv` per frame).
//!
//! Rates are per second of game time. Run it with a demo script, whose
//! `ManualDuration` time step makes every frame 1/fps s of game time however
//! long it takes, so each frame carries the same load on either backend and
//! wall-clock frame time measures what that load costs. In a window the
//! harness turns vsync off; offscreen runs are paced by the schedule runner,
//! so their wall-clock times mean nothing.
use bevy::{
    diagnostic::DiagnosticsStore,
    prelude::*,
    render::{renderer::RenderDevice, Render, RenderApp, RenderSystems},
    window::{PresentMode, PrimaryWindow},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

/// Seconds between one player's laser shots: the laser's 24-tick cooldown
/// at 60 Hz (`content::scenario::WeaponTuning::defaults`).
const LASER_COOLDOWN: f32 = 24.0 / 60.0;
/// The laser's reach in meters (same table).
const LASER_REACH: f32 = 12.0;

#[derive(Clone, Debug)]
pub struct Spec {
    impacts: f32,
    lasers: u32,
    arcs: f32,
    blood: f32,
    frames: u64,
    warmup: u64,
    /// Stop early (and say so in the report) once this many wall-clock
    /// seconds have passed since the warmup ended. 0: never.
    max_seconds: f64,
    /// Stop early once the mean of the last 300 measured frames is over this
    /// many ms. 0: never.
    abort_ms: f64,
    out: String,
}

impl Spec {
    pub fn parse(text: &str) -> Result<Spec, String> {
        let mut spec = Spec { impacts: 0.0, lasers: 0, arcs: 0.0, blood: 0.0, frames: 4200, warmup: 300, max_seconds: 0.0, abort_ms: 0.0, out: "vfx_stress".into() };
        for pair in text.split(',').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').ok_or_else(|| format!("--vfx-stress: expected key=value, got {pair}"))?;
            let bad = |e: String| format!("--vfx-stress {key}: {e}");
            match key {
                "impacts" => spec.impacts = value.parse().map_err(|e: std::num::ParseFloatError| bad(e.to_string()))?,
                "arcs" => spec.arcs = value.parse().map_err(|e: std::num::ParseFloatError| bad(e.to_string()))?,
                "blood" => spec.blood = value.parse().map_err(|e: std::num::ParseFloatError| bad(e.to_string()))?,
                "lasers" => spec.lasers = value.parse().map_err(|e: std::num::ParseIntError| bad(e.to_string()))?,
                "frames" => spec.frames = value.parse().map_err(|e: std::num::ParseIntError| bad(e.to_string()))?,
                "warmup" => spec.warmup = value.parse().map_err(|e: std::num::ParseIntError| bad(e.to_string()))?,
                "max_seconds" => spec.max_seconds = value.parse().map_err(|e: std::num::ParseFloatError| bad(e.to_string()))?,
                "abort_ms" => spec.abort_ms = value.parse().map_err(|e: std::num::ParseFloatError| bad(e.to_string()))?,
                "out" => spec.out = value.to_string(),
                _ => return Err(format!("--vfx-stress: unknown key {key}")),
            }
        }
        if spec.warmup >= spec.frames {
            return Err("--vfx-stress: warmup must be shorter than frames".into());
        }
        Ok(spec)
    }
}

pub struct StressPlugin(pub Spec);

impl Plugin for StressPlugin {
    fn build(&self, app: &mut App) {
        let shared = Shared::default();
        app.insert_resource(Stress::new(self.0.clone()))
            .insert_resource(shared.clone())
            .add_systems(Startup, no_vsync)
            .add_systems(First, begin_frame.before(crate::telemetry::begin))
            .add_systems(
                Update,
                (fire_impacts, fire_arcs, fire_blood).run_if(in_state(service::WorldState::Active)),
            )
            .add_systems(
                PostUpdate,
                fire_lasers
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy_sprinkles::SprinklesLateSetup)
                    .run_if(in_state(service::WorldState::Active)),
            )
            .add_systems(Last, end_frame.after(crate::telemetry::end));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .insert_resource(shared.clone())
            .init_resource::<RenderMarks>()
            .add_systems(
                Render,
                (
                    mark::<0>.before(RenderSystems::ExtractCommands),
                    mark::<1>.after(RenderSystems::Queue).before(RenderSystems::PhaseSort),
                    mark::<2>.after(RenderSystems::PhaseSort).before(RenderSystems::Prepare),
                    mark::<3>.after(RenderSystems::Prepare).before(RenderSystems::Render),
                    mark::<4>.after(RenderSystems::Render).before(RenderSystems::Cleanup),
                    end_render.after(RenderSystems::PostCleanup),
                ),
            );
        // Extraction runs on the main thread between frames: time it by
        // wrapping Bevy's extract function.
        let mut extract = render_app.take_extract();
        if extract.is_none() {
            warn!("vfx stress: the render app has no extract function; extract time not recorded");
        }
        render_app.set_extract(move |main, render| {
            let start = Instant::now();
            if let Some(extract) = extract.as_mut() {
                extract(main, render);
            }
            shared.0.lock().unwrap().extract.push(ms(start));
        });
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// Filled by the render world and the extract wrapper, drained each frame.
#[derive(Resource, Clone, Default)]
struct Shared(Arc<Mutex<FromRender>>);

#[derive(Default)]
struct FromRender {
    extract: Vec<f64>,
    /// One per render frame: the `SEGMENTS` in ms.
    render: Vec<[f64; SEGMENTS]>,
    /// wgpu's allocator report: allocated bytes, reserved bytes, allocations.
    gpu_memory: Option<(u64, u64, usize)>,
}

const SEGMENTS: usize = 11;
const SEGMENT_NAMES: [&str; SEGMENTS] = [
    "render_queue_ms",   // extract commands, prepare assets, views, specialize, queue
    "render_sort_ms",    // phase sort
    "render_prepare_ms", // batching, buffers and bind groups (Bevy's and the fork's)
    "render_graph_ms",   // encoding, submit and present
    "render_cleanup_ms",
    "render_total_ms",
    // From the fork's counters (`bevy_sprinkles::stats`).
    "sprinkles_extract_ms",
    "sprinkles_prepare_compute_ms",
    "sprinkles_prepare_sort_ms",
    "sprinkles_buffers_created",
    "sprinkles_bind_groups_created",
];

#[derive(Resource, Default)]
struct RenderMarks {
    marks: [Option<Instant>; 5],
    frames: u64,
}

fn mark<const I: usize>(mut marks: ResMut<RenderMarks>) {
    marks.marks[I] = Some(Instant::now());
}

fn end_render(mut marks: ResMut<RenderMarks>, shared: Res<Shared>, device: Option<Res<RenderDevice>>) {
    let end = Instant::now();
    let m = marks.marks;
    marks.frames += 1;
    let Some(start) = m[0] else { return };
    let mut segments = [0.0; SEGMENTS];
    let points: Vec<Instant> = m.iter().copied().map(|t| t.unwrap_or(start)).chain([end]).collect();
    for i in 0..5 {
        segments[i] = points[i + 1].saturating_duration_since(points[i]).as_secs_f64() * 1000.0;
    }
    segments[5] = end.duration_since(start).as_secs_f64() * 1000.0;
    use bevy_sprinkles::stats::{self, take};
    let ns = |c: &stats::Counter| take(c) as f64 / 1e6;
    segments[6] = ns(&stats::EXTRACT_NS);
    segments[7] = ns(&stats::PREPARE_COMPUTE_NS);
    segments[8] = ns(&stats::PREPARE_SORT_NS);
    segments[9] = take(&stats::BUFFERS_CREATED) as f64;
    segments[10] = take(&stats::BIND_GROUPS_CREATED) as f64;
    let report = (marks.frames % 120 == 0)
        .then(|| device.and_then(|d| d.wgpu_device().generate_allocator_report()))
        .flatten();
    let mut shared = shared.0.lock().unwrap();
    shared.render.push(segments);
    if let Some(report) = report {
        shared.gpu_memory = Some((report.total_allocated_bytes, report.total_reserved_bytes, report.allocations.len()));
    }
}

/// Deterministic random numbers, the same sequence on either backend.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next()
    }
}

#[derive(Resource)]
struct Stress {
    spec: Spec,
    rng: Rng,
    center: Option<Vec3>,
    impact_debt: f32,
    arc_debt: f32,
    blood_debt: f32,
    /// Its own sequence, so adding blood to a load keeps the impacts' places.
    blood_rng: Rng,
    /// Game seconds until each player's next shot.
    shooters: Vec<f32>,
    seed: u32,
    fired: BTreeMap<&'static str, u64>,
    frame: u64,
    frame_start: Option<Instant>,
    previous_start: Option<Instant>,
    run_start: Option<Instant>,
    /// The last measurement taken per diagnostic path.
    diagnostics_seen: BTreeMap<String, bevy::platform::time::Instant>,
    /// Per measured frame.
    rows: Vec<BTreeMap<String, f64>>,
    /// Once a second of game time, every frame.
    gauges: Vec<Value>,
    last_render: [f64; SEGMENTS],
    last_extract: f64,
    /// When the warmup ended.
    measure_start: Option<Instant>,
    /// Why the run stopped early, if it did.
    aborted: Option<String>,
}

impl Stress {
    fn new(spec: Spec) -> Self {
        let shooters = (0..spec.lasers).map(|i| i as f32 * LASER_COOLDOWN / spec.lasers.max(1) as f32).collect();
        Stress {
            spec,
            rng: Rng(0x5eed),
            center: None,
            impact_debt: 0.0,
            arc_debt: 0.0,
            blood_debt: 0.0,
            blood_rng: Rng(0xb100d),
            shooters,
            seed: 1,
            fired: BTreeMap::new(),
            frame: 0,
            frame_start: None,
            previous_start: None,
            run_start: None,
            diagnostics_seen: BTreeMap::new(),
            rows: Vec::new(),
            gauges: Vec::new(),
            last_render: [0.0; SEGMENTS],
            last_extract: 0.0,
            measure_start: None,
            aborted: None,
        }
    }

    fn next_seed(&mut self) -> u32 {
        self.seed = self.seed.wrapping_add(1);
        self.seed
    }

    fn count(&mut self, what: &'static str) {
        *self.fired.entry(what).or_default() += 1;
    }
}

fn no_vsync(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    for mut window in &mut windows {
        window.present_mode = PresentMode::AutoNoVsync;
    }
}

/// Where the effects go: around the player when the world starts.
fn center(world: &mut World) -> Option<Vec3> {
    if let Some(center) = world.resource::<Stress>().center {
        return Some(center);
    }
    let mut players = world.query_filtered::<&GlobalTransform, With<crate::character::Player>>();
    let at = players.iter(world).next().map(|t| t.translation() * Vec3::new(1.0, 0.0, 1.0)).unwrap_or(Vec3::ZERO);
    world.resource_mut::<Stress>().center = Some(at);
    Some(at)
}

fn running(world: &World) -> bool {
    let stress = world.resource::<Stress>();
    stress.frame < stress.spec.frames
}

fn fire_impacts(world: &mut World) {
    if !running(world) {
        return;
    }
    let Some(center) = center(world) else { return };
    let dt = world.resource::<Time>().delta_secs();
    let mut hits = Vec::new();
    {
        let mut stress = world.resource_mut::<Stress>();
        stress.impact_debt += stress.spec.impacts * dt;
        while stress.impact_debt >= 1.0 {
            stress.impact_debt -= 1.0;
            let (angle, radius) = (stress.rng.range(0.0, std::f32::consts::TAU), stress.rng.range(1.0, 7.0));
            let height = stress.rng.range(0.1, 1.2);
            let facing = stress.rng.range(0.0, std::f32::consts::TAU);
            let up = stress.rng.range(0.2, 1.0);
            let position = center + Vec3::new(angle.cos() * radius, height, angle.sin() * radius);
            let normal = Vec3::new(facing.cos(), up, facing.sin()).normalize();
            let number = stress.next_seed();
            stress.count("impacts");
            hits.push((position, normal, number));
        }
    }
    if hits.is_empty() {
        return;
    }
    let mut queue = bevy::ecs::world::CommandQueue::default();
    {
        let assets = world.resource::<crate::effects::FxAssets>();
        let mut commands = Commands::new(&mut queue, world);
        for (position, normal, number) in hits {
            crate::effects::impact(&mut commands, assets, position, normal, number, voxel_assets::ImpactResponse::Stone);
        }
    }
    queue.apply(world);
}

/// Cuts around the center, 0.65 to 1 m up, spraying away in a random
/// direction, as a sword cut on a ghoul does (`settlement.rs`).
fn fire_blood(world: &mut World) {
    if !running(world) {
        return;
    }
    let Some(center) = center(world) else { return };
    let dt = world.resource::<Time>().delta_secs();
    let mut cuts = Vec::new();
    {
        let mut stress = world.resource_mut::<Stress>();
        stress.blood_debt += stress.spec.blood * dt;
        while stress.blood_debt >= 1.0 {
            stress.blood_debt -= 1.0;
            let rng = &mut stress.blood_rng;
            let (angle, radius) = (rng.range(0.0, std::f32::consts::TAU), rng.range(1.0, 7.0));
            let height = rng.range(0.65, 1.0);
            let facing = rng.range(0.0, std::f32::consts::TAU);
            let position = center + Vec3::new(angle.cos() * radius, height, angle.sin() * radius);
            let direction = Vec3::new(facing.cos(), 0.0, facing.sin());
            let seed = stress.next_seed();
            stress.count("blood");
            cuts.push((position, direction, seed));
        }
    }
    for (position, direction, seed) in cuts {
        crate::effects::blood(world, position, direction, seed);
    }
}

fn fire_arcs(world: &mut World) {
    if !running(world) {
        return;
    }
    let Some(center) = center(world) else { return };
    let dt = world.resource::<Time>().delta_secs();
    let mut arcs = Vec::new();
    {
        let mut stress = world.resource_mut::<Stress>();
        stress.arc_debt += stress.spec.arcs * dt;
        while stress.arc_debt >= 1.0 {
            stress.arc_debt -= 1.0;
            let angle = stress.rng.range(0.0, std::f32::consts::TAU);
            let radius = stress.rng.range(0.0, 6.0);
            let from = center + Vec3::new(angle.cos() * radius, stress.rng.range(0.8, 1.4), angle.sin() * radius);
            let (heading, length) = (stress.rng.range(0.0, std::f32::consts::TAU), stress.rng.range(2.0, 4.0));
            let to = from + Vec3::new(heading.cos() * length, stress.rng.range(-0.4, 0.2), heading.sin() * length);
            let seed = stress.next_seed();
            stress.count("arcs");
            arcs.push((from, to, seed));
        }
    }
    for (from, to, seed) in arcs {
        crate::effects::arc(world, from, to, seed);
    }
}

/// Eight players on a 5 m ring, each firing at the laser's cooldown at a
/// random point in reach, staggered.
fn fire_lasers(world: &mut World) {
    if !running(world) {
        return;
    }
    let Some(center) = center(world) else { return };
    let dt = world.resource::<Time>().delta_secs();
    let mut shots = Vec::new();
    {
        let mut stress = world.resource_mut::<Stress>();
        let count = stress.shooters.len();
        for i in 0..count {
            stress.shooters[i] -= dt;
            while stress.shooters[i] <= 0.0 {
                stress.shooters[i] += LASER_COOLDOWN;
                let angle = i as f32 / count as f32 * std::f32::consts::TAU;
                let muzzle = center + Vec3::new(angle.cos() * 5.0, 1.3, angle.sin() * 5.0);
                // Roughly across the ring, within reach.
                let aim = angle + std::f32::consts::PI + stress.rng.range(-0.6, 0.6);
                let reach = stress.rng.range(4.0, LASER_REACH);
                let target = muzzle + Vec3::new(aim.cos() * reach, stress.rng.range(-1.1, 0.2), aim.sin() * reach);
                let seed = stress.next_seed();
                stress.count("laser_bolts");
                shots.push((muzzle, target, seed));
            }
        }
    }
    for (from, target, seed) in shots {
        crate::effects::stress_bolt(world, from, target, seed);
    }
}

fn begin_frame(mut stress: ResMut<Stress>) {
    let now = Instant::now();
    stress.previous_start = stress.frame_start.replace(now);
    stress.run_start.get_or_insert(now);
}

fn end_frame(world: &mut World) {
    let Some(start) = world.resource::<Stress>().frame_start else { return };
    let update_ms = ms(start);
    let active = world.get_resource::<State<service::WorldState>>().is_some_and(|s| *s.get() == service::WorldState::Active);
    if !active || world.resource::<Stress>().center.is_none() {
        return;
    }
    // The render world's numbers arrive a frame or two late; keep the latest.
    let (extract, render, gpu_memory) = {
        let shared = world.resource::<Shared>().clone();
        let mut shared = shared.0.lock().unwrap();
        (std::mem::take(&mut shared.extract), std::mem::take(&mut shared.render), shared.gpu_memory)
    };
    let mut row = BTreeMap::new();
    row.insert("update_ms".to_string(), update_ms);
    // The fork's main-world systems, this frame.
    row.insert("sprinkles_setup_ms".to_string(), bevy_sprinkles::stats::take(&bevy_sprinkles::stats::SETUP_NS) as f64 / 1e6);
    row.insert("sprinkles_update_ms".to_string(), bevy_sprinkles::stats::take(&bevy_sprinkles::stats::UPDATE_NS) as f64 / 1e6);
    {
        let mut stress = world.resource_mut::<Stress>();
        if let Some(&e) = extract.last() {
            stress.last_extract = e;
        }
        if let Some(&r) = render.last() {
            stress.last_render = r;
        }
        if let Some(previous) = stress.previous_start {
            row.insert("frame_ms".to_string(), start.duration_since(previous).as_secs_f64() * 1000.0);
        }
        row.insert("extract_ms".to_string(), stress.last_extract);
        for (name, value) in SEGMENT_NAMES.iter().zip(stress.last_render) {
            row.insert(name.to_string(), value);
        }
    }
    // GPU (and render CPU) time per pass, when the diagnostics have a new
    // measurement.
    let mut fresh = Vec::new();
    if let Some(store) = world.get_resource::<DiagnosticsStore>() {
        for diagnostic in store.iter() {
            let path = diagnostic.path().as_str();
            if !path.starts_with("render/") || !(path.ends_with("/elapsed_gpu") || path.ends_with("/elapsed_cpu")) {
                continue;
            }
            if let Some(m) = diagnostic.measurement() {
                fresh.push((path.to_string(), m.value, m.time));
            }
        }
    }
    let frame = {
        let mut stress = world.resource_mut::<Stress>();
        for (path, value, time) in fresh {
            if stress.diagnostics_seen.insert(path.clone(), time) != Some(time) {
                row.insert(path, value);
            }
        }
        stress.frame += 1;
        stress.frame
    };
    let (warmup, frames) = {
        let spec = &world.resource::<Stress>().spec;
        (spec.warmup, spec.frames)
    };
    if frame > warmup {
        let mut stress = world.resource_mut::<Stress>();
        stress.rows.push(row);
        stress.measure_start.get_or_insert_with(Instant::now);
        stress.aborted = over_budget(&stress);
    }
    let aborted = world.resource::<Stress>().aborted.is_some();
    let fps = world.resource::<crate::control::Options>().fps() as u64;
    if frame % fps == 0 || frame == frames || aborted {
        let gauges = gauges(world, frame, gpu_memory);
        world.resource_mut::<Stress>().gauges.push(gauges);
    }
    if frame == frames || aborted {
        write_results(world);
        world.write_message(AppExit::Success);
    }
}

/// Whether the run should stop early (`max_seconds`, `abort_ms`), and why.
fn over_budget(stress: &Stress) -> Option<String> {
    let spec = &stress.spec;
    let elapsed = stress.measure_start.map_or(0.0, |s| s.elapsed().as_secs_f64());
    if spec.max_seconds > 0.0 && elapsed > spec.max_seconds {
        return Some(format!("wall-clock budget: {:.0} s after warmup", spec.max_seconds));
    }
    const WINDOW: usize = 300;
    if spec.abort_ms > 0.0 && stress.rows.len() >= WINDOW {
        let recent = &stress.rows[stress.rows.len() - WINDOW..];
        let mean = recent.iter().filter_map(|r| r.get("frame_ms")).sum::<f64>() / WINDOW as f64;
        if mean > spec.abort_ms {
            return Some(format!("mean frame {mean:.1} ms over the last {WINDOW} frames is above {} ms", spec.abort_ms));
        }
    }
    None
}

fn gauges(world: &mut World, frame: u64, gpu_memory: Option<(u64, u64, usize)>) -> Value {
    fn count<F: bevy::ecs::query::QueryFilter>(world: &mut World) -> usize {
        world.query_filtered::<(), F>().iter(world).count()
    }
    let entities: usize = world.archetypes().iter().map(|a| a.len() as usize).sum();
    let wall = world.resource::<Stress>().run_start.map_or(0.0, |s| s.elapsed().as_secs_f64());
    let (allocated, reserved, allocations) = gpu_memory.unwrap_or_default();
    let (pools, pooled_bursts) = world.get_resource::<bevy_sprinkles::pool::ParticlePools>().map_or((0, 0), |p| p.counts());
    json!({
        "frame": frame,
        "game_s": world.resource::<Time>().elapsed_secs_f64(),
        "wall_s": wall,
        "entities": entities,
        "particle_systems": count::<With<bevy_sprinkles::prelude::Particles3d>>(world),
        "emitters": count::<With<bevy_sprinkles::EmitterEntity>>(world),
        "pools": pools,
        "pooled_emitters": pooled_bursts,
        "legacy_particles": count::<With<crate::effects::Particle>>(world),
        "point_lights": count::<With<PointLight>>(world),
        "shader_buffers": world.resource::<Assets<bevy::render::storage::ShaderBuffer>>().len(),
        "particle_materials": world.get_resource::<Assets<bevy_sprinkles::ParticleMaterial>>().map_or(0, |a| a.len()),
        "standard_materials": world.resource::<Assets<StandardMaterial>>().len(),
        "meshes": world.resource::<Assets<Mesh>>().len(),
        "gpu_allocated_mb": allocated as f64 / 1048576.0,
        "gpu_reserved_mb": reserved as f64 / 1048576.0,
        "gpu_allocations": allocations,
    })
}

fn stats(values: &mut [f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(f64::total_cmp);
    let at = |p: f64| values[((values.len() - 1) as f64 * p).round() as usize];
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    json!({"n": values.len(), "mean": mean, "p50": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": values[values.len() - 1]})
}

fn write_results(world: &mut World) {
    let backend = format!("{:?}", world.resource::<crate::vfx::Vfx>().backend).to_lowercase();
    let stress = world.resource::<Stress>();
    let mut columns: Vec<String> = stress.rows.iter().flat_map(|r| r.keys().cloned()).collect();
    columns.sort();
    columns.dedup();
    let mut summary = serde_json::Map::new();
    for column in &columns {
        let mut values: Vec<f64> = stress.rows.iter().filter_map(|r| r.get(column).copied()).collect();
        summary.insert(column.clone(), stats(&mut values));
    }
    let wall = stress.run_start.map_or(0.0, |s| s.elapsed().as_secs_f64());
    let spec = &stress.spec;
    let result = json!({
        "backend": backend,
        "spec": {"impacts_per_s": spec.impacts, "laser_players": spec.lasers, "arcs_per_s": spec.arcs, "blood_per_s": spec.blood, "frames": spec.frames, "warmup": spec.warmup, "max_seconds": spec.max_seconds, "abort_ms": spec.abort_ms},
        "fired": stress.fired,
        "wall_s": wall,
        "measured_frames": stress.rows.len(),
        "aborted": stress.aborted.is_some(),
        "abort_reason": stress.aborted,
        "pooled": bevy_sprinkles::pool::enabled(),
        "stats_ms": summary,
        "gauges": stress.gauges,
        "note": "frame_ms: wall time First to First; update_ms: main world First..Last; extract_ms: render app extraction on the main thread; render_*: render-world schedule segments (pipelined, a frame behind); render/<pass>/elapsed_gpu: GPU timestamp spans; sprinkles_*: the fork's counters (the two *_created values are counts per frame, not ms)",
    });
    let mut csv = columns.join(",") + "\n";
    for row in &stress.rows {
        let line: Vec<String> = columns.iter().map(|c| row.get(c).map_or(String::new(), |v| format!("{v:.4}"))).collect();
        csv += &line.join(",");
        csv.push('\n');
    }
    let out = spec.out.clone();
    std::fs::write(format!("{out}.json"), serde_json::to_vec_pretty(&result).unwrap()).expect("write stress summary");
    std::fs::write(format!("{out}.csv"), csv).expect("write stress frames");
    info!("vfx stress: wrote {out}.json");
}

#[cfg(test)]
mod tests {
    use super::Spec;

    #[test]
    fn a_spec_parses_and_rejects_typos() {
        let spec = Spec::parse("impacts=50,lasers=8,arcs=2.5,blood=20,frames=900,warmup=100,out=x").unwrap();
        assert_eq!((spec.impacts, spec.lasers, spec.arcs, spec.blood, spec.frames, spec.warmup), (50.0, 8, 2.5, 20.0, 900, 100));
        assert!(Spec::parse("impact=50").is_err());
        assert!(Spec::parse("impacts=lots").is_err());
        assert!(Spec::parse("frames=100,warmup=100").is_err());
        let spec = Spec::parse("impacts=200,max_seconds=40,abort_ms=50").unwrap();
        assert_eq!((spec.max_seconds, spec.abort_ms), (40.0, 50.0));
    }
}

//! Authored effects: one RON file per effect in `kestrel/app/vfx/` (format in
//! `kestrel/docs/effect-definition.md`), drawn by our fork of sprinkles
//! (`kestrel/vfx/sprinkles`).
//! This is the VFX spike (`kestrel/VFX_SPIKE_HANDOFF.md`): the impacts, the
//! arc and the laser bolt have files so far, and `VfxBackend` switches
//! between them and the hand-written effects in `effects.rs`.
//!
//! Effects are view-only. Nothing here enters the simulation or its hashes:
//! the files are app data, and what they spawn lives in the rendered world.
use bevy::prelude::*;
use bevy_sprinkles::prelude::{Particles3d, ParticlesAsset, ParticlesCollider3D, ParticlesColliderShape3D, ParticleTrigger};
use debug_ui::{egui, ImguiManager, ImguiMenu, MenuHandle, MenuId, MenuSpec, UiOutbox};
use serde::Deserialize;
use service::{AppLayer, AppServices, Phase, Service, WorldScoped, WorldState};
use std::path::PathBuf;

pub mod catalog;
mod ron_tree;

/// The version of our effect file format (`version:` in each file).
pub const FORMAT_VERSION: u32 = 1;

/// Every effect file, by name: the file name is the effect's ID. Compiled in,
/// like the rest of Kestrel's data; a test checks the list against the folder.
pub const FILES: &[(&str, &str)] = &[
    ("impact_stone", include_str!("../vfx/impact_stone.ron")),
    ("impact_metal", include_str!("../vfx/impact_metal.ron")),
    ("impact_crystal", include_str!("../vfx/impact_crystal.ron")),
    ("impact_polymer", include_str!("../vfx/impact_polymer.ron")),
    ("arc", include_str!("../vfx/arc.ron")),
    ("laser_bolt", include_str!("../vfx/laser_bolt.ron")),
];

/// Which path draws effects that have a file. `--vfx legacy|sprinkles`, or
/// Effects → VFX backend in the debug menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VfxBackend {
    /// The hand-written effects in `effects.rs`.
    #[default]
    Legacy,
    /// Effect files, drawn by sprinkles. Runs without GPU particles (simulation
    /// and the web build for now) still draw the legacy effects.
    Sprinkles,
}

impl VfxBackend {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "sprinkles" => Ok(Self::Sprinkles),
            _ => Err(format!("--vfx: expected legacy or sprinkles, got {value}")),
        }
    }
}

/// One effect file: layers drawn together at the trigger point. A file's
/// `base` is merged in before this parse (`catalog::load`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectFile {
    pub version: u32,
    /// The particle layer: a sprinkles `ParticlesAsset`, embedded as is.
    pub particles: ParticlesAsset,
    /// Point lights spawned next to the particles (GPU particles can't light
    /// the scene).
    #[serde(default)]
    pub lights: Vec<LightLayer>,
}

/// A point light that fades out quadratically.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightLayer {
    /// sRGB.
    pub color: [f32; 3],
    /// Lumens at the start.
    pub intensity: f32,
    pub range: f32,
    pub radius: f32,
    /// Meters along the trigger's normal.
    #[serde(default)]
    pub offset: f32,
    pub fade_seconds: f32,
}

/// Parses and checks one effect file without a `base`. Typos, unknown
/// fields, another sprinkles format version and effects that would never end
/// are errors. Particle colors are converted from the file's sRGB to linear.
pub fn parse(name: &str, source: &str) -> Result<EffectFile, String> {
    let mut file: EffectFile = ron::from_str(source).map_err(|e| format!("vfx/{name}.ron: {e}"))?;
    let fail = |why: String| Err(format!("vfx/{name}.ron: {why}"));
    if file.version != FORMAT_VERSION {
        return fail(format!("version {} (this build reads {FORMAT_VERSION})", file.version));
    }
    let sprinkles = bevy_sprinkles::asset::versions::current_format_version();
    if file.particles.sprinkles_version() != sprinkles {
        return fail(format!("particles are sprinkles {} (this build reads {sprinkles})", file.particles.sprinkles_version()));
    }
    // Effects are bursts: the system must end and despawn itself.
    if !file.particles.despawn_on_finish {
        return fail("particles need despawn_on_finish: true".into());
    }
    if let Some(emitter) = file.particles.emitters.iter().find(|e| !e.time.one_shot) {
        return fail(format!("emitter {} needs one_shot: true", emitter.name));
    }
    if let Some(light) = file.lights.iter().find(|l| l.fade_seconds <= 0.0 || l.intensity < 0.0) {
        return fail(format!("light {light:?} needs a positive fade and intensity"));
    }
    catalog::particles_to_linear(&mut file.particles);
    Ok(file)
}

struct Effect {
    name: String,
    /// None in runs without GPU particles.
    particles: Option<Handle<ParticlesAsset>>,
    emitters: usize,
    lights: Vec<LightLayer>,
    /// An emitter flies to the target (`lifetime_to_target`), so Effects →
    /// Fire places the effect with +Y toward it.
    aims: bool,
}

impl Effect {
    /// `file` as effect `name`. Its particles replace the asset behind
    /// `handle` in place (a reload: running systems see `Modified` and
    /// rebuild their pools) or go into a new asset.
    fn new(name: String, file: EffectFile, handle: Option<Handle<ParticlesAsset>>, assets: Option<&mut Assets<ParticlesAsset>>) -> Self {
        let emitters = file.particles.emitters.len();
        let aims = file.particles.emitters.iter().any(|e| e.time.lifetime_to_target);
        let particles = assets.map(|assets| match handle {
            Some(handle) => {
                assets.insert(handle.id(), file.particles).expect("a live effect handle");
                handle
            }
            None => assets.add(file.particles),
        });
        Effect { name, particles, emitters, lights: file.lights, aims }
    }
}

/// The effect library and backend switch (App layer).
#[derive(Resource)]
pub struct Vfx {
    pub backend: VfxBackend,
    effects: Vec<Effect>,
    /// Where Reload reads the files, or why it can't.
    source: Result<PathBuf, String>,
    /// The last reload's outcome, shown in the Effects window.
    reload: Option<ReloadReport>,
    menu: Option<MenuHandle>,
}

impl Service for Vfx {
    type Layer = AppLayer;
    type Deps = (ImguiManager,);

    fn init(world: &mut World) -> Self {
        let backend = world.get_resource::<crate::control::Options>().map_or_else(VfxBackend::default, |o| o.vfx);
        let effects = catalog::load(FILES.iter().copied())
            .into_iter()
            .map(|(name, file)| {
                let file = file.unwrap_or_else(|e| panic!("{e}"));
                Effect::new(name, file, None, world.get_resource_mut::<Assets<ParticlesAsset>>().as_deref_mut())
            })
            .collect();
        let menu = ImguiManager::register::<Vfx>(world).map_err(|e| warn!("{e}")).ok();
        Vfx { backend, effects, source: source_dir(), reload: None, menu }
    }

    fn shutdown(self, world: &mut World) {
        if let Some(menu) = self.menu {
            ImguiManager::unregister(world, menu);
        }
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(
            Phase::Present,
            (
                fade_lights,
                place_floor
                    .run_if(in_state(WorldState::Active))
                    .run_if(resource_exists::<Assets<ParticlesAsset>>),
            ),
        );
    }
}

/// What an effect is triggered with. Only `position` and `normal` are
/// required; the rest are per-trigger parameters the effect file may use
/// (`docs/effect-definition.md`, "Trigger parameters").
#[derive(Clone, Copy, Debug)]
pub struct Trigger {
    pub position: Vec3,
    /// The effect's local +Y.
    pub normal: Vec3,
    /// A world-space point the effect reaches for: a line's end, a bolt's
    /// destination.
    pub target: Option<Vec3>,
    /// The same seed always draws the same variation. `None`: random.
    pub seed: Option<u32>,
    /// Uniform size of the whole effect: positions, speeds, gravity and
    /// particle sizes (not lifetimes or lights).
    pub scale: f32,
}

impl Trigger {
    pub fn new(position: Vec3, normal: Vec3) -> Self {
        Trigger { position, normal, target: None, seed: None, scale: 1.0 }
    }

    pub fn target(self, target: Vec3) -> Self {
        Trigger { target: Some(target), ..self }
    }

    pub fn seed(self, seed: u32) -> Self {
        Trigger { seed: Some(seed), ..self }
    }
}

/// Draws effect `name` as `trigger` says, when this run draws effect files.
/// Returns false when the caller should draw its legacy effect instead. An
/// unknown name is a bug and panics.
///
/// May be called before transform propagation or after it, up to
/// `bevy_sprinkles::SprinklesLateSetup`; either way the effect draws this frame.
pub fn spawn(world: &mut World, name: &str, trigger: Trigger) -> bool {
    let Some(vfx) = world.get_resource::<Vfx>() else { return false };
    if vfx.backend != VfxBackend::Sprinkles {
        return false;
    }
    let effect = vfx.effects.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("no effect file vfx/{name}.ron"));
    let Some(particles) = effect.particles.clone() else { return false };
    let (label, lights) = (effect.name.clone(), effect.lights.clone());
    let Trigger { position, normal, target, seed, scale } = trigger;
    let rotation = Quat::from_rotation_arc(Vec3::Y, normal.normalize_or(Vec3::Y));
    let transform = Transform::from_translation(position).with_rotation(rotation).with_scale(Vec3::splat(scale));
    world.spawn((
        Name::new(label.clone()),
        Particles3d(particles),
        ParticleTrigger { target, seed },
        transform,
        // A root entity: right before the next propagation too.
        GlobalTransform::from(transform),
        WorldScoped::default(),
    ));
    for light in lights {
        let [r, g, b] = light.color;
        world.spawn((
            Name::new(label.clone()),
            PointLight { color: Color::srgb(r, g, b), intensity: light.intensity, range: light.range, radius: light.radius, ..default() },
            Transform::from_translation(position + rotation * Vec3::Y * light.offset * scale),
            EffectLight { age: 0.0, fade: light.fade_seconds, intensity: light.intensity },
            WorldScoped::default(),
        ));
    }
    true
}

/// A light layer's light, fading out.
#[derive(Component)]
struct EffectLight {
    age: f32,
    fade: f32,
    intensity: f32,
}

fn fade_lights(mut commands: Commands, time: Res<Time>, mut lights: Query<(Entity, &mut PointLight, &mut EffectLight)>) {
    for (entity, mut light, mut effect) in &mut lights {
        effect.age += time.delta_secs().min(0.1);
        if effect.age > effect.fade {
            commands.entity(entity).despawn();
            continue;
        }
        light.intensity = effect.intensity * (1.0 - effect.age / effect.fade).powi(2);
    }
}

/// The ground particles bounce on: every scene's floor is at y = 0 today, as
/// the legacy effects assume.
#[derive(Component)]
struct Floor;

fn place_floor(mut commands: Commands, floors: Query<(), With<Floor>>) {
    if floors.is_empty() {
        commands.spawn((
            Floor,
            Name::new("VFX floor"),
            ParticlesCollider3D { enabled: true, shape: ParticlesColliderShape3D::Box { size: Vec3::new(400.0, 2.0, 400.0) } },
            Transform::from_xyz(0.0, -1.0, 0.0),
            WorldScoped::default(),
        ));
    }
}

/// The effect files in the source tree, for Effects → Reload. Only runs
/// started with `bazel run` know where that is. Not gated on debug builds:
/// the builds people run (and measure) are `-c opt`.
fn source_dir() -> Result<PathBuf, String> {
    let workspace = std::env::var_os("BUILD_WORKSPACE_DIRECTORY").ok_or("needs `bazel run` (BUILD_WORKSPACE_DIRECTORY unset)")?;
    let dir = PathBuf::from(workspace).join("kestrel/app/vfx");
    if !dir.is_dir() {
        return Err(format!("can't find {}", dir.display()));
    }
    Ok(dir)
}

/// What a reload did, shown in the Effects window.
#[derive(Debug, Default)]
struct ReloadReport {
    /// Counts reloads, so a repeated one shows.
    number: u32,
    loaded: Vec<String>,
    /// Each failed file's error; that effect keeps its last good version.
    errors: Vec<String>,
    /// Effects whose file is gone: they stay loaded until a restart.
    kept: Vec<String>,
}

/// Rereads the effect files from the source tree (Effects → Reload). Runs
/// from the UI outbox in `Update`, so the `Modified` asset events reach
/// sprinkles' pool rebuild in this frame's `PostUpdate`.
fn reload(world: &mut World) {
    world.resource_scope(|world, mut vfx: Mut<Vfx>| {
        let read = vfx.source.clone().and_then(|dir| catalog::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display())));
        let mut report = match read {
            Ok(files) => {
                let results = catalog::load(files.iter().map(|(n, t)| (n.as_str(), t.as_str())));
                apply_reload(&mut vfx.effects, results, world.get_resource_mut::<Assets<ParticlesAsset>>().as_deref_mut())
            }
            Err(e) => ReloadReport { errors: vec![e], ..default() },
        };
        for error in &report.errors {
            warn!("Effects reload: {error} (keeping the last good version)");
        }
        report.number = vfx.reload.as_ref().map_or(0, |r| r.number) + 1;
        vfx.reload = Some(report);
    });
}

/// Replaces each effect that loaded and adds new ones. An effect whose file
/// failed, or is gone, keeps its last good version.
fn apply_reload(
    effects: &mut Vec<Effect>,
    results: Vec<(String, Result<EffectFile, String>)>,
    mut assets: Option<&mut Assets<ParticlesAsset>>,
) -> ReloadReport {
    let kept = effects.iter().filter(|e| !results.iter().any(|(n, _)| *n == e.name)).map(|e| e.name.clone()).collect();
    let mut report = ReloadReport { kept, ..default() };
    for (name, file) in results {
        let file = match file {
            Ok(file) => file,
            Err(e) => {
                report.errors.push(e);
                continue;
            }
        };
        report.loaded.push(name.clone());
        let index = effects.iter().position(|e| e.name == name);
        let handle = index.and_then(|i| effects[i].particles.clone());
        let effect = Effect::new(name, file, handle, assets.as_deref_mut());
        match index {
            Some(i) => effects[i] = effect,
            None => effects.push(effect),
        }
    }
    report
}

/// Effects → Fire places an effect this far in front of the player, its
/// target this much further on, at about the height of a blade's impact.
const FIRE_DISTANCE: f32 = 3.0;
const FIRE_REACH: f32 = 4.0;
const FIRE_HEIGHT: f32 = 0.5;

/// Where Effects → Fire triggers an effect for the player at `player`: in
/// front of them (+Z is their facing), normal up, with a target further on.
/// An effect that `aims` points its +Y at the target instead, as the laser
/// is placed at its muzzle.
fn fire_trigger(player: &Transform, aims: bool) -> Trigger {
    let forward = (player.rotation * Vec3::Z).with_y(0.0).normalize_or(Vec3::Z);
    let position = player.translation.with_y(FIRE_HEIGHT) + forward * FIRE_DISTANCE;
    let normal = if aims { forward } else { Vec3::Y };
    Trigger::new(position, normal).target(position + forward * FIRE_REACH)
}

const MENUS: &[MenuSpec] = &[MenuSpec::new("vfx_backend", "Effects", "VFX backend")];

impl ImguiMenu for Vfx {
    fn menus() -> &'static [MenuSpec] {
        MENUS
    }

    fn show_menu(&mut self, id: MenuId, root: &mut egui::Ui, open: &mut bool, world: &mut World) {
        let gpu = self.effects.iter().any(|e| e.particles.is_some());
        id.window().open(open).default_width(300.0).show(root.ctx(), |ui| {
            let mut choice = self.backend;
            ui.radio_value(&mut choice, VfxBackend::Legacy, "Legacy (effects.rs)");
            ui.add_enabled_ui(gpu, |ui| ui.radio_value(&mut choice, VfxBackend::Sprinkles, "Sprinkles (effect files)"));
            if choice != self.backend {
                world.resource_mut::<UiOutbox>().push(move |world| world.resource_mut::<Vfx>().backend = choice);
            }
            ui.separator();
            let button = ui.add_enabled(self.source.is_ok(), egui::Button::new("Reload effect files"));
            if let Err(why) = &self.source {
                ui.weak(format!("Reload {why}."));
            }
            if button.clicked() {
                world.resource_mut::<UiOutbox>().push(reload);
            }
            if let Some(report) = &self.reload {
                ui.label(format!("Reload {}: {} loaded, {} failed", report.number, report.loaded.len(), report.errors.len()));
                for error in &report.errors {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                if !report.errors.is_empty() {
                    ui.weak("Failed files keep their last good version.");
                }
                if !report.kept.is_empty() {
                    ui.weak(format!("No file anymore, kept until restart: {}", report.kept.join(", ")));
                }
            }
            ui.separator();
            let player = world.query_filtered::<&Transform, With<crate::character::Player>>().single(world).ok().copied();
            let blocked = if self.backend != VfxBackend::Sprinkles {
                Some("Fire needs the sprinkles backend.")
            } else if !gpu {
                Some("Fire needs GPU particles.")
            } else if player.is_none() {
                Some("Fire needs a player.")
            } else {
                None
            };
            for effect in &self.effects {
                ui.horizontal(|ui| {
                    let fire = ui.add_enabled(blocked.is_none(), egui::Button::new("Fire"));
                    if let (true, Some(player)) = (fire.clicked(), player) {
                        let (name, trigger) = (effect.name.clone(), fire_trigger(&player, effect.aims));
                        world.resource_mut::<UiOutbox>().push(move |world| {
                            spawn(world, &name, trigger);
                        });
                    }
                    ui.label(format!("{}: {} emitters, {} lights", effect.name, effect.emitters, effect.lights.len()));
                });
            }
            match blocked {
                Some(why) => ui.weak(why),
                None => ui.weak(format!("{FIRE_DISTANCE} m in front of the player, target {FIRE_REACH} m further on.")),
            };
            ui.label("Effects without a file always use the legacy path.");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_file_parses() {
        for (name, file) in catalog::load(FILES.iter().copied()) {
            let file = file.unwrap_or_else(|e| panic!("{e}"));
            assert!(!file.particles.emitters.is_empty() || !file.lights.is_empty(), "{name} draws nothing");
        }
    }

    #[test]
    fn a_misspelled_field_is_an_error() {
        let (name, source) = FILES[0];
        // In our layer and in the embedded sprinkles asset.
        assert!(parse(name, &source.replace("fade_seconds", "fade_secs")).is_err());
        assert!(parse(name, &source.replace("particles_amount", "particle_amount")).is_err());
    }

    #[test]
    fn an_effect_that_never_ends_is_an_error() {
        let (name, source) = FILES[0];
        assert!(parse(name, &source.replacen("one_shot: true", "one_shot: false", 1)).is_err());
    }

    #[test]
    fn a_reload_replaces_good_files_in_place_and_keeps_the_last_good_version() {
        let mut assets = Assets::<ParticlesAsset>::default();
        // Three effects, none built on another.
        let files = FILES.iter().copied().filter(|(n, _)| ["impact_stone", "arc", "laser_bolt"].contains(n));
        let mut effects: Vec<Effect> =
            catalog::load(files).into_iter().map(|(n, f)| Effect::new(n, f.unwrap(), None, Some(&mut assets))).collect();
        let handle = |effects: &[Effect], name: &str| effects.iter().find(|e| e.name == name).unwrap().particles.clone().unwrap();
        let (stone, arc) = (handle(&effects, "impact_stone"), handle(&effects, "arc"));
        let source = |name: &str| FILES.iter().find(|(n, _)| *n == name).unwrap().1;

        // The arc changes, the stone breaks, the laser's file is gone and a new one appears.
        let arc_text = source("arc").replace("lifetime: 0.13", "lifetime: 0.5");
        let stone_text = source("impact_stone").replace("fade_seconds", "fade_secs");
        let sources = [("arc", arc_text.as_str()), ("impact_stone", stone_text.as_str()), ("spark", r#"(base: "impact_stone")"#)];
        let report = apply_reload(&mut effects, catalog::load(sources), Some(&mut assets));

        assert_eq!(report.loaded, ["arc"]);
        assert_eq!(report.errors.len(), 2, "{:?}", report.errors);
        assert!(report.errors[0].starts_with("vfx/impact_stone.ron"), "{}", report.errors[0]);
        assert_eq!(report.kept, ["laser_bolt"]);
        // The arc's asset changed behind the same handle; the stone kept its own.
        assert_eq!(handle(&effects, "arc"), arc);
        assert_eq!(assets.get(&arc).unwrap().emitters[0].time.lifetime, 0.5);
        assert_eq!(handle(&effects, "impact_stone"), stone);
        assert_eq!(assets.get(&stone).unwrap().emitters.len(), 3);
        assert_eq!(effects.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["impact_stone", "arc", "laser_bolt"]);

        // A new file that loads is added.
        let report = apply_reload(&mut effects, catalog::load([("impact_stone", source("impact_stone")), ("spark", r#"(base: "impact_stone")"#)]), Some(&mut assets));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(effects.last().unwrap().name, "spark");
        assert!(effects.last().unwrap().particles.is_some());
    }

    #[test]
    fn fire_places_effects_in_front_of_the_player() {
        // Facing +X.
        let player = Transform::from_xyz(1.0, 0.0, 2.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let trigger = fire_trigger(&player, false);
        assert!(trigger.position.distance(Vec3::new(4.0, 0.5, 2.0)) < 1e-5, "{trigger:?}");
        assert!(trigger.target.unwrap().distance(Vec3::new(8.0, 0.5, 2.0)) < 1e-5, "{trigger:?}");
        assert_eq!(trigger.normal, Vec3::Y);
        assert!(fire_trigger(&player, true).normal.distance(Vec3::X) < 1e-5);
        // Only the laser flies to its target.
        let aims: Vec<_> = catalog::load(FILES.iter().copied()).into_iter().map(|(n, f)| (n, Effect::new(String::new(), f.unwrap(), None, None).aims)).collect();
        assert!(aims.iter().all(|(n, aims)| *aims == (n == "laser_bolt")), "{aims:?}");
    }
}

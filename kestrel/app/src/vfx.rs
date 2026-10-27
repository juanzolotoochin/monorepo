//! Authored effects: one RON file per effect in `kestrel/app/vfx/` (format in
//! its README.md), drawn by our fork of sprinkles (`kestrel/vfx/sprinkles`).
//! This is the VFX spike (`kestrel/VFX_SPIKE_HANDOFF.md`): the stone impact,
//! the arc and the laser bolt have files so far, and `VfxBackend` switches
//! between them and the hand-written effects in `effects.rs`.
//!
//! Effects are view-only. Nothing here enters the simulation or its hashes:
//! the files are app data, and what they spawn lives in the rendered world.
use bevy::prelude::*;
use bevy_sprinkles::prelude::{Particles3d, ParticlesAsset, ParticlesCollider3D, ParticlesColliderShape3D, ParticleTrigger};
use debug_ui::{egui, ImguiManager, ImguiMenu, MenuHandle, MenuId, MenuSpec, UiOutbox};
use serde::Deserialize;
use service::{AppLayer, AppServices, Phase, Service, WorldScoped, WorldState};

/// The version of our effect file format (`version:` in each file).
pub const FORMAT_VERSION: u32 = 1;

/// Every effect file, by name. Compiled in, like the rest of Kestrel's data.
pub const FILES: &[(&str, &str)] = &[
    ("impact_stone", include_str!("../vfx/impact_stone.ron")),
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

/// One effect file: layers drawn together at the trigger point.
#[derive(Deserialize)]
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

/// Parses and checks one effect file. Typos, unknown fields, another
/// sprinkles format version and effects that would never end are errors.
pub fn parse(name: &str, source: &str) -> Result<EffectFile, String> {
    let file: EffectFile = ron::from_str(source).map_err(|e| format!("vfx/{name}.ron: {e}"))?;
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
    Ok(file)
}

struct Effect {
    name: &'static str,
    /// None in runs without GPU particles.
    particles: Option<Handle<ParticlesAsset>>,
    emitters: usize,
    lights: Vec<LightLayer>,
}

/// The effect library and backend switch (App layer).
#[derive(Resource)]
pub struct Vfx {
    pub backend: VfxBackend,
    effects: Vec<Effect>,
    menu: Option<MenuHandle>,
}

impl Service for Vfx {
    type Layer = AppLayer;
    type Deps = (ImguiManager,);

    fn init(world: &mut World) -> Self {
        let backend = world.get_resource::<crate::control::Options>().map_or_else(VfxBackend::default, |o| o.vfx);
        let effects = FILES
            .iter()
            .map(|&(name, source)| {
                let file = parse(name, source).unwrap_or_else(|e| panic!("{e}"));
                let emitters = file.particles.emitters.len();
                let particles = world.get_resource_mut::<Assets<ParticlesAsset>>().map(|mut assets| assets.add(file.particles));
                Effect { name, particles, emitters, lights: file.lights }
            })
            .collect();
        let menu = ImguiManager::register::<Vfx>(world).map_err(|e| warn!("{e}")).ok();
        Vfx { backend, effects, menu }
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
/// (README.md, "Trigger parameters").
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
    let (label, lights) = (effect.name, effect.lights.clone());
    let Trigger { position, normal, target, seed, scale } = trigger;
    let rotation = Quat::from_rotation_arc(Vec3::Y, normal.normalize_or(Vec3::Y));
    let transform = Transform::from_translation(position).with_rotation(rotation).with_scale(Vec3::splat(scale));
    world.spawn((
        Name::new(label),
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
            Name::new(label),
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
            for effect in &self.effects {
                ui.label(format!("{}: {} emitters, {} lights", effect.name, effect.emitters, effect.lights.len()));
            }
            ui.label("Effects without a file always use the legacy path.");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_file_parses() {
        for &(name, source) in FILES {
            let file = parse(name, source).unwrap_or_else(|e| panic!("{e}"));
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
}

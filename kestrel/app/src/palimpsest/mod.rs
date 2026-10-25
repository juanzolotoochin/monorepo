//! A motion recording projects a persistent partner without copying world state or inventory.
use crate::{
    combat::Obstacle, gameplay::Controls,
    sound::{Cue, Sound},
};
use bevy::prelude::*;
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};
use voxel_assets::SceneFeature;
mod view;

impl Service for Memory {
    type Layer = WorldLayer;
    type Deps = (crate::lockstep::WorldMirror, crate::scene_view::SceneView);

    fn init(world: &mut World) -> Self {
        view::spawn(world);
        Memory::default()
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Input, view::reset.run_if(in_state(WorldState::Active)))
            .add_service_systems::<Self, _>(Phase::Mirror, tick.run_if(in_state(WorldState::Active)));
    }
}

const PADS: [Vec3; 2] = [Vec3::new(-4.0, 0.0, -2.0), Vec3::new(-1.0, 0.0, 3.0)];
const CORE: Vec3 = Vec3::new(5.0, 0.8, 0.0);
#[derive(Clone, Copy)]
struct Sample {
    time: f32,
    position: Vec3,
    rotation: Quat,
}
/// The view's copy of the simulated memory trial (World layer).
#[derive(Resource, Default)]
pub struct Memory {
    sample_count: usize,
    recording: bool,
    elapsed: f32,
    replay: f32,
    ghost: Option<Sample>,
    pads: [bool; 2],
    charge: f32,
    pub unlocked: bool,
    pub collected: bool,
}
#[derive(Resource)]
struct Art {
    cube: Handle<Mesh>,
    metal: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    blue: Handle<StandardMaterial>,
    green: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    ghost: Handle<StandardMaterial>,
}
/// A piece of the trial's view; it goes with the world.
#[derive(Component)]
#[require(WorldScoped)]
struct Trial;
#[derive(Component)]
struct Ghost;
#[derive(Component)]
struct Pad(usize);
#[derive(Component)]
struct Cable(usize);
/// The trial's door, which stops blocking once it opens.
#[derive(Component)]
pub(crate) struct Door;
#[derive(Component)]
struct Core;
#[derive(Component)]
struct Meter(usize);
#[derive(Component)]
struct Instructions;

/// Starts recording (scripts and the E key).
pub fn record(world: &mut World) -> Result<(), String> {
    if world.resource::<Memory>().recording {
        return Err("A recording is already in progress".into());
    }
    world.resource_mut::<crate::lockstep::SessionService>().command(sim::CommandKind::MemoryRecord);
    world.resource::<Sound>().play(Cue::Dispense);
    Ok(())
}
/// Projects the recording and returns the player to where it began.
pub fn deploy(world: &mut World) -> Result<(), String> {
    let m = world.resource::<Memory>();
    if !m.recording || m.elapsed < 0.15 {
        return Err("Record at least 0.15 seconds before deploying".into());
    }
    world.resource_mut::<crate::lockstep::SessionService>().command(sim::CommandKind::MemoryDeploy);
    *world.resource_mut::<Controls>() = Controls { weapon: world.resource::<Controls>().weapon, ..Controls::default() };
    world.resource::<Sound>().play(Cue::Crystal);
    Ok(())
}
pub fn clear(world: &mut World) {
    world.resource_mut::<crate::lockstep::SessionService>().command(sim::CommandKind::MemoryClear);
}
impl Memory {
    /// A recording is in progress (as of the latest mirrored tick).
    pub fn recording(&self) -> bool {
        self.recording
    }
}

/// Mirrors the simulated trial for the view, and plays its feedback. E/Q
/// (and the controller's utility panel) record, deploy and clear through
/// the shared Input path (`input::gameplay`).
pub fn tick(world: &mut World) {
    let scene = world.resource::<crate::scene_view::SceneView>();
    let memory_trial = scene.has_feature(SceneFeature::MemoryTrial);
    let dt = world.resource::<Time>().delta_secs().min(0.1);
    let elapsed = world.resource::<Time>().elapsed_secs();
    let mirror = world.resource::<crate::lockstep::WorldMirror>();
    let mirror = mirror.current.as_ref().and_then(|s| s.memory.clone()).map(|sim| {
        let center = mirror.center;
        let sample = |s: &sim::settlement::memory::Sample| Sample {
            time: 0.0,
            position: Vec3::new((s.position.x - center.x).to_f32(), s.height.to_f32(), (s.position.y - center.y).to_f32()),
            rotation: Quat::from_rotation_y(s.heading.to_f32()),
        };
        (sim.recording, sim.elapsed as f32 / crate::legacy::HZ, sim.replay as f32 / crate::legacy::HZ, sim.ghost.as_ref().map(sample),
            [sim.pressed[0], sim.pressed[1]], sim.charge.to_f32(), sim.unlocked, sim.collected, sim.samples.len())
    });
    let mut m = world.remove_resource::<Memory>().unwrap();
    if let Some((recording, recorded, replayed, ghost, pads, charge, unlocked, collected, count)) = mirror {
        let (was_pads, was_unlocked, was_collected) = (m.pads, m.unlocked, m.collected);
        m.recording = recording;
        m.elapsed = recorded;
        m.replay = replayed;
        m.ghost = ghost;
        m.pads = pads;
        m.charge = charge;
        m.unlocked = unlocked;
        m.collected = collected;
        m.sample_count = count;
        if memory_trial && was_pads != m.pads {
            world.resource::<Sound>().play(Cue::Dispense);
        }
        if memory_trial && unlocked && !was_unlocked {
            let doors: Vec<_> = world.query_filtered::<Entity, With<Door>>().iter(world).collect();
            for e in doors {
                world.entity_mut(e).remove::<Obstacle>();
            }
            world.resource::<Sound>().play(Cue::Crystal);
            world.resource_mut::<crate::effects::Feedback>().trauma = 0.25;
        }
        if memory_trial && collected && !was_collected {
            world.resource::<Sound>().play(Cue::Land);
            let fx = world.resource::<crate::effects::FxAssets>();
            let mesh = fx.cube.clone();
            let material = fx.spark[2].clone();
            for n in 0..24 {
                let angle = n as f32 * std::f32::consts::TAU / 24.0;
                world.spawn((
                    Trial,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(CORE).with_scale(Vec3::splat(0.08)),
                    crate::effects::Particle {
                        velocity: Vec3::new(angle.cos() * 3.0, 2.0 + (n % 3) as f32, angle.sin() * 3.0),
                        age: 0.0,
                        life: 0.8,
                        size: Vec3::splat(0.08),
                        gravity: 6.0,
                        bounce: false,
                    },
                ));
            }
        }
    }
    view::update(world, &m, elapsed, dt);
    world.insert_resource(m);
}

pub fn snapshot(world: &World) -> serde_json::Value {
    let Some(m) = world.get_resource::<Memory>() else { return serde_json::Value::Null };
    serde_json::json!({"recording":m.recording,"recorded_seconds":m.elapsed,"replay_seconds":m.replay,"sample_count":m.sample_count,"ghost_position":m.ghost.map(|g|g.position.to_array()),"plates":m.pads,"charge":m.charge,"unlocked":m.unlocked,"collected":m.collected})
}

//! A motion recording projects a persistent partner without copying world state or inventory.
use crate::{
    assets::SceneLibrary,
    gameplay::{Controls, Obstacle, Player},
    navigation::{flat, Disc},
    sound::{Cue, Sound},
};
use bevy::prelude::*;
use voxel_assets::SceneFeature;
mod view;
pub use view::setup;

const LIMIT: f32 = 8.0;
const PADS: [Vec3; 2] = [Vec3::new(-4.0, 0.0, -2.0), Vec3::new(-1.0, 0.0, 3.0)];
const CORE: Vec3 = Vec3::new(5.0, 0.8, 0.0);
#[derive(Clone, Copy)]
struct Sample {
    time: f32,
    position: Vec3,
    rotation: Quat,
}
#[derive(Resource, Default)]
pub struct Memory {
    current: Option<usize>,
    samples: Vec<Sample>,
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
#[derive(Component)]
struct Trial;
#[derive(Component)]
struct Ghost;
#[derive(Component)]
struct Pad(usize);
#[derive(Component)]
struct Cable(usize);
#[derive(Component)]
struct Door;
#[derive(Component)]
struct Core;
#[derive(Component)]
struct Meter(usize);
#[derive(Component)]
struct Instructions;

fn player(world: &mut World) -> (Entity, Transform) {
    world
        .query_filtered::<(Entity, &Transform), With<Player>>()
        .single(world)
        .map(|(e, t)| (e, *t))
        .expect("player")
}
pub fn record(world: &mut World) -> Result<(), String> {
    let (_, t) = player(world);
    let mut m = world.resource_mut::<Memory>();
    if m.recording {
        return Err("A recording is already in progress".into());
    }
    m.samples.clear();
    m.samples.push(Sample {
        time: 0.0,
        position: t.translation,
        rotation: t.rotation,
    });
    m.recording = true;
    m.elapsed = 0.0;
    m.replay = 0.0;
    m.ghost = None;
    world.resource::<Sound>().play(Cue::Dispense);
    Ok(())
}
pub fn deploy(world: &mut World) -> Result<(), String> {
    let (entity, _) = player(world);
    let mut m = world.resource_mut::<Memory>();
    if !m.recording || m.elapsed < 0.15 {
        return Err("Record at least 0.15 seconds before deploying".into());
    }
    let anchor = m.samples[0];
    m.recording = false;
    m.replay = 0.0;
    m.ghost = Some(anchor);
    let mut t = world.get_mut::<Transform>(entity).unwrap();
    t.translation = anchor.position;
    t.rotation = anchor.rotation;
    *world.resource_mut::<Controls>() = Controls::default();
    world.resource_mut::<crate::piles::PileLab>().job = None;
    world.resource::<Sound>().play(Cue::Crystal);
    Ok(())
}
pub fn clear(world: &mut World) {
    let mut m = world.resource_mut::<Memory>();
    m.recording = false;
    m.samples.clear();
    m.ghost = None;
    m.elapsed = 0.0;
    m.replay = 0.0;
}
fn sample_at(samples: &[Sample], time: f32) -> Option<Sample> {
    let last = *samples.last()?;
    if time >= last.time {
        return Some(last);
    }
    let next = samples.partition_point(|s| s.time <= time).max(1);
    let a = samples[next - 1];
    let b = samples[next];
    let t = ((time - a.time) / (b.time - a.time).max(0.00001)).clamp(0.0, 1.0);
    Some(Sample {
        time,
        position: a.position.lerp(b.position, t),
        rotation: a.rotation.slerp(b.rotation, t),
    })
}
pub fn tick(world: &mut World) {
    let options = world.resource::<crate::control::Options>();
    let interactive = !options.simulation && options.frames_dir.is_none();
    let library = world.resource::<crate::assets::SceneLibrary>();
    let reserved_e = library.scenes[library.current].scene.scenario.is_some();
    let memory_trial = library.has_feature(SceneFeature::MemoryTrial);
    if interactive && !reserved_e {
        let keys = world.resource::<ButtonInput<KeyCode>>();
        let e = keys.just_pressed(KeyCode::KeyE);
        let q = keys.just_pressed(KeyCode::KeyQ);
        if q {
            clear(world);
        }
        if e {
            if world.resource::<Memory>().recording {
                let _ = deploy(world);
            } else {
                let _ = record(world);
            }
        }
    }
    let (_, t) = player(world);
    let dt = world.resource::<Time>().delta_secs().min(0.1);
    let elapsed = world.resource::<Time>().elapsed_secs();
    let mut m = world.remove_resource::<Memory>().unwrap();
    if m.recording {
        m.elapsed = (m.elapsed + dt).min(LIMIT);
        m.samples.push(Sample {
            time: m.elapsed,
            position: t.translation,
            rotation: t.rotation,
        });
    } else if m.ghost.is_some() {
        m.replay = (m.replay + dt).min(m.elapsed);
        m.ghost = sample_at(&m.samples, m.replay);
    }
    if memory_trial {
        let old = m.pads;
        for (i, p) in PADS.iter().enumerate() {
            let on = |v: Vec3| flat(v - *p).length() < 0.62 && (v.y - p.y).abs() < 0.25;
            m.pads[i] = on(t.translation) || m.ghost.is_some_and(|g| on(g.position));
        }
        if old != m.pads {
            world.resource::<Sound>().play(Cue::Dispense);
        }
        if !m.unlocked {
            m.charge = if m.pads.iter().all(|&p| p) {
                (m.charge + dt / 1.2).min(1.0)
            } else {
                (m.charge - dt * 2.0).max(0.0)
            };
            if m.charge >= 1.0 {
                m.unlocked = true;
                let doors: Vec<_> = world
                    .query_filtered::<Entity, With<Door>>()
                    .iter(world)
                    .collect();
                for e in doors {
                    world.entity_mut(e).remove::<Obstacle>();
                }
                world.resource::<Sound>().play(Cue::Crystal);
                world.resource_mut::<crate::effects::Feedback>().trauma = 0.25;
            }
        }
        if m.unlocked
            && !m.collected
            && flat(t.translation - CORE).length() < 0.8
            && t.translation.y < 1.0
        {
            m.collected = true;
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
                        velocity: Vec3::new(
                            angle.cos() * 3.0,
                            2.0 + (n % 3) as f32,
                            angle.sin() * 3.0,
                        ),
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

    let auto = m.recording && m.elapsed >= LIMIT;
    world.insert_resource(m);
    if auto {
        let _ = deploy(world);
    }
}

pub fn snapshot(world: &World) -> serde_json::Value {
    let m = world.resource::<Memory>();
    serde_json::json!({"recording":m.recording,"recorded_seconds":m.elapsed,"replay_seconds":m.replay,"sample_count":m.samples.len(),"ghost_position":m.ghost.map(|g|g.position.to_array()),"plates":m.pads,"charge":m.charge,"unlocked":m.unlocked,"collected":m.collected})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_interpolates_in_seconds_and_holds_final_pose() {
        let samples = [
            Sample {
                time: 0.0,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            },
            Sample {
                time: 2.0,
                position: Vec3::X * 4.0,
                rotation: Quat::from_rotation_y(1.0),
            },
        ];
        assert_eq!(sample_at(&samples, 1.0).unwrap().position, Vec3::X * 2.0);
        assert_eq!(sample_at(&samples, 10.0).unwrap().position, Vec3::X * 4.0);
        assert!(sample_at(&[], 0.0).is_none());
    }
}

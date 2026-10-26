//! Other players' avatars in a networked game: a body per avatar this machine
//! doesn't control, built and dressed like the local player's, placed where
//! the simulation has it between ticks and posed like the local player,
//! swings included.

use bevy::prelude::*;

use content::scenario::Weapon;
use sim::settlement::avatar::Swing;

use crate::character::{
    dig_age, jump_motion, GaitClock, MotionState, Part, Player, Pose,
};
use crate::combat::{Attack, attack_end, rebound_end};
use crate::jetpack::Exhaust;
use crate::lockstep::WorldMirror;
use crate::style::WeaponVisual;

#[derive(Component)]
pub struct Peer {
    agent: u16,
    /// The style the body was dressed in; a restyle builds a new one.
    style: Option<usize>,
    gait: GaitClock,
    /// The start tick of the last swing heard, so each whooshes once.
    heard: u64,
    motion: MotionState,
    visual_height: Option<f32>,
}

impl Peer {
    /// The avatar this body shows.
    pub fn agent(&self) -> u16 {
        self.agent
    }
}

/// The swing as the local player's animation sees it, `ticks` after the
/// last simulated tick began: wind-up and cut from the swing's start, or a
/// rebound from the tick its blade hit something.
fn attack(swing: &Swing, ticks: f32, swift_sword: bool) -> Option<Attack> {
    let seconds = |from: u64| ((ticks - from as f32) / content::TICKS_PER_SECOND as f32).max(0.0);
    let mut attack = Attack::new(
        Vec3::new(swing.aim.x.to_f32(), 0.0, swing.aim.y.to_f32()),
        swing.weapon,
    );
    attack.stroke = swing.stroke;
    attack.swift = swift_sword;
    let rebound =
        swing.resolved && swing.hits > 0 && matches!(swing.weapon, Weapon::Sword | Weapon::Spear);
    if rebound {
        // The blade stops where it was when the hit resolved.
        attack.age = swing.resolve_at.saturating_sub(swing.started) as f32
            / content::TICKS_PER_SECOND as f32;
        attack.impact_angle = Some(attack.angle());
        attack.age = seconds(swing.resolve_at);
        (attack.age < rebound_end()).then_some(attack)
    } else {
        attack.age = seconds(swing.started);
        (attack.age < attack_end()).then_some(attack)
    }
}

/// Other players' avatars and name tags, and the local player's hurt
/// flash (World layer).
#[derive(Resource)]
pub struct Peers;

impl service::Service for Peers {
    type Layer = service::WorldLayer;
    type Deps = (WorldMirror, crate::scene_view::SceneView);

    fn init(_world: &mut World) -> Self {
        Peers
    }

    fn systems(app: &mut App) {
        use service::{AppServices, Phase, WorldState};
        let active = in_state(WorldState::Active);
        app.add_service_systems::<Self, _>(Phase::Mirror, update.run_if(crate::control::rendered).run_if(active.clone()))
            .add_service_systems::<Self, _>(Phase::Present, crate::hurt::update.run_if(crate::control::rendered).run_if(active.clone()))
            .add_systems(
                PostUpdate,
                crate::nametags::update
                    .after(bevy::transform::TransformSystems::Propagate)
                    .run_if(crate::control::rendered)
                    .run_if(active),
            );
    }
}

pub fn update(
    mut commands: Commands,
    mirror: Res<WorldMirror>,
    service: Res<crate::lockstep::SessionService>,
    scene: Res<crate::scene_view::SceneView>,
    contacts: Option<Res<crate::character::ActorFootContacts>>,
    time: Res<Time>,
    sound: Res<crate::sound::Sound>,
    mut peers: Query<(Entity, &mut Peer, &mut Transform), (Without<Part>, Without<Player>)>,
    mut joints: crate::character::Joints,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    mut exhausts: Query<(&ChildOf, &mut Visibility), (With<Exhaust>, Without<WeaponVisual>)>,
    mut weapons: Query<(Entity, &WeaponVisual, &mut Visibility), Without<Exhaust>>,
) {
    let me = mirror.player();
    let others: Vec<u16> = mirror.current.as_ref().map_or_else(Vec::new, |s| {
        (0..s.agents.len() as u16).filter(|&i| s.agents[i as usize].avatar().is_some() && Some(i) != me).collect()
    });
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    // Render time in ticks: views sit `alpha` of the way from the previous
    // tick's state to the current one's.
    let ticks = mirror.tick as f32 - 1.0 + mirror.alpha;
    let mut motion_profile = crate::character::motion_profile(&scene.pack);
    if let Some(contacts) = &contacts {
        motion_profile = contacts.adapt_motion(motion_profile);
    }
    let mut poses = Vec::new();
    let mut shown = Vec::new();
    for (entity, mut peer, mut transform) in &mut peers {
        if !others.contains(&peer.agent) || peer.style != Some(scene.index) {
            commands.entity(entity).despawn();
            continue;
        }
        shown.push(peer.agent);
        let current = &mirror.current.as_ref().unwrap().agents[peer.agent as usize];
        let before = mirror.previous.as_ref().and_then(|p| p.agents.get(peer.agent as usize)).unwrap_or(current);
        let (Some(avatar), Some(before_avatar)) = (current.avatar(), before.avatar()) else { continue };
        let (from, to) = (mirror.to_view(before.body.position), mirror.to_view(current.body.position));
        let alpha = if from.distance(to) > 1.5 { 1.0 } else { mirror.alpha };
        let mut position = from.lerp(to, alpha);
        position.y = before_avatar.height.to_f32()
            + (avatar.height - before_avatar.height).to_f32() * alpha;
        position.y = crate::character::visual_step_height(
            if from.distance(to) > 1.5 { None } else { peer.visual_height },
            position.y,
            time.delta_secs(),
            !avatar.flight.airborne
                && matches!(avatar.jump, sim::settlement::avatar::JumpPhase::Grounded),
        );
        peer.visual_height = Some(position.y);
        let moved = position
            .with_y(0.0)
            .distance(transform.translation.with_y(0.0))
            .min(1.0);
        let step_distance = motion_profile.step_distance.clamp(0.2, 4.0);
        let steps = peer
            .gait
            .advance(moved, step_distance, !avatar.flight.airborne);
        for _ in 0..steps {
            sound.play(crate::sound::Cue::Step);
        }
        transform.translation = position;
        let turn = (current.body.heading - before.body.heading).to_f32();
        let turn =
            (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let heading = before.body.heading.to_f32() + turn * alpha;
        transform.rotation = Quat::from_rotation_y(heading);
        let airborne = avatar.flight.airborne;
        let velocity = if airborne {
            Vec3::ZERO
        } else {
            Vec3::new(
                current.body.velocity.x.to_f32(),
                0.0,
                current.body.velocity.y.to_f32(),
            )
        };
        let (speed, turn) =
            peer.motion
                .advance_with_profile(velocity, heading, time.delta_secs(), motion_profile);
        let swing = avatar.swing.as_ref();
        let attack = swing.and_then(|s| {
            attack(
                s,
                ticks,
                scene.player().swift_sword(),
            )
        });
        if let (Some(swing), Some(_)) = (swing, attack) {
            if peer.heard != swing.started {
                peer.heard = swing.started;
                sound.play(crate::sound::Cue::Swing);
            }
        }
        let dig = avatar.dig.as_ref().map(|d| dig_age(ticks, d));
        let pose = Pose {
                motion: voxel_assets::CharacterMotion {
                    step_distance: motion_profile.step_distance / transform.scale.x.max(0.1),
                    ..motion_profile
                },
                stride: peer.gait.phase,
                speed,
                turn,
                aim_yaw: avatar.aim.map_or(0.0, |aim| {
                    crate::lockstep::angle_between(heading, aim.x.to_f32().atan2(aim.y.to_f32()))
                }),
                breathe,
                step_pitch: 0.0,
                airborne,
                jump: jump_motion(
                    avatar.jump,
                    scene.player().jump(),
                    ticks,
                    avatar.height.to_f32(),
                    avatar.flight.support.to_f32(),
                ),
                attack,
                dig,
            };
        crate::character::pose_body(*transform, pose, |point| service.ground_height(point), false,
            time.delta_secs(), contacts.as_deref(), children.iter_descendants(entity), &mut joints);
        poses.push((entity, avatar.weapon, pose));
    }
    for (parent, mut visibility) in &mut exhausts {
        let owner = std::iter::once(parent.parent())
            .chain(parents.iter_ancestors(parent.parent()))
            .find_map(|ancestor| poses.iter().find(|(entity, ..)| *entity == ancestor));
        if let Some((.., pose)) = owner {
            *visibility = if pose.airborne && pose.jump.is_none() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    for (entity, weapon, mut visibility) in &mut weapons {
        let held = parents.iter_ancestors(entity).find_map(|e| {
            poses
                .iter()
                .find(|(p, ..)| *p == e)
                .map(|(_, held, _)| *held)
        });
        if let Some(held) = held {
            *visibility = if weapon.0 == held {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    let style = Some(scene.index);
    for agent in others.into_iter().filter(|a| !shown.contains(a)) {
        let pack = scene.pack.clone();
        commands.queue(move |world: &mut World| {
            let root = crate::scene::spawn_remote_player(world, &pack);
            world.entity_mut(root).insert(
                Peer {
                    agent,
                    style,
                    gait: GaitClock::default(),
                    heard: 0,
                    motion: MotionState::default(),
                    visual_height: None,
                },
            );
        });
    }
}

/// Other players for the control snapshot: where each stands and which
/// weapon models are showing.
pub fn snapshot(world: &mut World) -> Vec<serde_json::Value> {
    let shown: Vec<(Entity, Weapon)> = world
        .query::<(Entity, &WeaponVisual, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, _, visible)| visible.get())
        .map(|(e, w, _)| (e, w.0))
        .collect();
    let owner = |world: &World, mut e: Entity| {
        while let Some(parent) = world.get::<ChildOf>(e) {
            e = parent.parent();
            if world.get::<Peer>(e).is_some() {
                return Some(e);
            }
        }
        None
    };
    let held: Vec<(Entity, Weapon)> = shown
        .iter()
        .filter_map(|&(e, w)| Some((owner(world, e)?, w)))
        .collect();
    let mut peers = world.query_filtered::<(Entity, &Transform), With<Peer>>();
    peers
        .iter(world)
        .map(|(e, t)| {
            let mut weapons: Vec<_> = held
                .iter()
                .filter(|(p, _)| *p == e)
                .map(|(_, w)| format!("{w:?}"))
                .collect();
            weapons.dedup();
            serde_json::json!({"position": t.translation.to_array(), "weapons": weapons})
        })
        .collect()
}


#[cfg(test)]
mod tests {
    use super::*;
    use fx::{Fx, FxVec2};

    const HZ: f32 = content::TICKS_PER_SECOND as f32;
    /// The swings start on tick 100 and resolve a sixth of a second later.
    const RESOLVE: f32 = 1.0 / 6.0;

    /// The tick `seconds` into the swing.
    fn at(seconds: f32) -> f32 {
        100.0 + seconds * HZ
    }

    fn swing(hits: u32) -> Swing {
        Swing {
            weapon: Weapon::Sword,
            stroke: sim::settlement::avatar::SwordStroke::Forehand,
            aim: FxVec2::new(Fx::ONE, Fx::ZERO),
            muzzle: [Fx::ZERO; 3],
            started: 100,
            resolve_at: at(RESOLVE).round() as u64,
            until: at(17.0 / 30.0).round() as u64,
            resolved: hits > 0,
            hits,
            queued_at: None,
        }
    }

    #[test]
    fn a_swing_plays_from_its_start_tick_and_ends() {
        let s = swing(0);
        assert_eq!(attack(&s, 100.0, false).unwrap().age, 0.0);
        let mid = attack(&s, at(0.2), false).unwrap();
        assert!((mid.age - 0.2).abs() < 1e-4 && mid.impact_angle.is_none());
        assert!((mid.aim - Vec3::X).length() < 1e-4);
        assert!(attack(&s, at(attack_end()) + 0.1, false).is_none());
    }

    #[test]
    fn a_hit_rebounds_from_where_the_blade_was() {
        let s = swing(1);
        let at_hit = attack(&swing(0), at(RESOLVE), false).unwrap().angle();
        let rebound = attack(&s, at(RESOLVE + 0.05), false).unwrap();
        assert_eq!(rebound.impact_angle, Some(at_hit));
        assert!((rebound.age - 0.05).abs() < 1e-4);
        assert!(attack(&s, at(RESOLVE + rebound_end()) + 0.1, false).is_none());
    }

    /// Each joint of a body, depth first, by kind and transform.
    fn joints(world: &World, entity: Entity, out: &mut Vec<String>) {
        if let Some(part) = world.get::<Part>(entity) {
            out.push(format!("{:?} {:?}", part.kind, world.get::<Transform>(entity).unwrap()));
        }
        for child in world.get::<Children>(entity).map_or_else(Vec::new, |c| c.iter().collect()) {
            joints(world, child, out);
        }
    }

    #[test]
    fn remote_bodies_pose_like_the_local_player() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let scenes = crate::assets::SceneLibrary::new();
        let mut app = crate::test_app(options, scenes, hub, crate::flow::Launch::single_player(0));
        crate::start_world(&mut app);
        let world = app.world_mut();
        let player = world.query_filtered::<Entity, With<Player>>().single(world).unwrap();
        let pack = world.resource::<crate::scene_view::SceneView>().pack.clone();
        let remote = crate::scene::spawn_remote_player(world, &pack);
        let root = *world.get::<Transform>(player).unwrap();
        *world.get_mut::<Transform>(remote).unwrap() = root;
        // Both bodies start at rest: the local one has run a few frames.
        let parts: Vec<_> = world.query::<(Entity, &Part)>().iter(world).map(|(e, p)| (e, p.rest)).collect();
        for (entity, rest) in parts {
            *world.get_mut::<Transform>(entity).unwrap() = rest;
        }
        let mut rest = Vec::new();
        joints(world, player, &mut rest);
        // Mid-stride at a run, so the planted-foot solver runs too.
        let pose = Pose {
            motion: crate::character::motion_profile(&pack),
            stride: 1.3,
            speed: 1.2,
            turn: 0.2,
            aim_yaw: 0.3,
            breathe: 0.01,
            step_pitch: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        assert!(pose.motion.plant_feet);
        let mut state = bevy::ecs::system::SystemState::<(crate::character::Joints, Query<&Children>)>::new(world);
        let (mut parts, children) = state.get_mut(world).unwrap();
        for body in [player, remote] {
            crate::character::pose_body(root, pose, |_| Some(root.translation.y), false, 1.0 / 60.0, None,
                children.iter_descendants(body), &mut parts);
        }
        let (mut local, mut other) = (Vec::new(), Vec::new());
        joints(world, player, &mut local);
        joints(world, remote, &mut other);
        assert_eq!(local.len(), 18);
        assert_eq!(local, other);
        assert_ne!(local, rest);
    }
}

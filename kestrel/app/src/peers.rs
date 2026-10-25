//! Other players' avatars in a networked game: a copy of the local player's
//! exosuit (as the scene's style dresses it) per avatar this machine doesn't
//! control, placed where the simulation has it between ticks and posed like
//! the local player, swings included.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use std::collections::HashMap;

use content::scenario::Weapon;
use sim::settlement::avatar::Swing;

use crate::character::{
    blend_pose, dig_age, grounded_foot_targets, jump_motion, GaitClock, JointSnapshot,
    MotionState, Part, PartKind, Player, Pose,
};
use crate::combat::{Attack, ATTACK_END, REBOUND_END};
use crate::jetpack::Exhaust;
use crate::lockstep::Game;
use crate::style::{ActiveStyle, WeaponVisual};

#[derive(Component)]
pub struct Peer {
    agent: u16,
    /// The style the copy was made in; a restyle makes a new copy.
    style: Option<usize>,
    gait: GaitClock,
    /// The start tick of the last swing heard, so each whooshes once.
    heard: u64,
    motion: MotionState,
}

impl Peer {
    /// The avatar this copy shows.
    pub fn agent(&self) -> u16 {
        self.agent
    }
}

/// A copied part pivot. Not a `Part`, so the local player's animation and
/// styling leave it alone.
#[derive(Component)]
pub struct PeerPart {
    kind: PartKind,
    rest: Transform,
}

/// A copied jetpack exhaust, shown while that avatar flies.
#[derive(Component)]
pub struct PeerExhaust;

/// A copied weapon model, shown while it's the weapon the avatar holds.
#[derive(Component)]
pub struct PeerWeapon(Weapon);

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
        (attack.age < REBOUND_END).then_some(attack)
    } else {
        attack.age = seconds(swing.started);
        (attack.age < ATTACK_END).then_some(attack)
    }
}

pub fn update(
    mut commands: Commands,
    game: Res<Game>,
    style: Res<ActiveStyle>,
    contacts: Option<Res<crate::character::ActorFootContacts>>,
    library: Res<crate::assets::SceneLibrary>,
    time: Res<Time>,
    sound: Res<crate::sound::Sound>,
    player: Single<Entity, With<Player>>,
    mut peers: Query<(Entity, &mut Peer, &mut Transform), (Without<PeerPart>, Without<Player>)>,
    mut parts: Query<(Entity, &PeerPart, &mut Transform), Without<Peer>>,
    parents: Query<&ChildOf>,
    mut exhausts: Query<(&ChildOf, &mut Visibility), (With<PeerExhaust>, Without<PeerWeapon>)>,
    mut weapons: Query<(Entity, &PeerWeapon, &mut Visibility), Without<PeerExhaust>>,
) {
    let me = game.player();
    let others: Vec<u16> = game.current.as_ref().map_or_else(Vec::new, |s| {
        (0..s.agents.len() as u16)
            .filter(|&i| s.agents[i as usize].controlled && Some(i) != me)
            .collect()
    });
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    // Render time in ticks: views sit `alpha` of the way from the previous
    // tick's state to the current one's.
    let ticks = game.tick as f32 - 1.0 + game.alpha;
    let mut motion_profile = crate::character::motion_profile(&library.scenes[library.current]);
    if let Some(contacts) = &contacts {
        motion_profile = contacts.adapt_motion(motion_profile);
    }
    let mut poses = Vec::new();
    let mut shown = Vec::new();
    for (entity, mut peer, mut transform) in &mut peers {
        if !others.contains(&peer.agent) || peer.style != style.current() {
            commands.entity(entity).despawn();
            continue;
        }
        shown.push(peer.agent);
        let current = &game.current.as_ref().unwrap().agents[peer.agent as usize];
        let before = game
            .previous
            .as_ref()
            .and_then(|p| p.agents.get(peer.agent as usize))
            .unwrap_or(current);
        let (from, to) = (
            game.to_view(before.position),
            game.to_view(current.position),
        );
        let alpha = if from.distance(to) > 1.5 {
            1.0
        } else {
            game.alpha
        };
        let mut position = from.lerp(to, alpha);
        position.y = before.avatar.height.to_f32()
            + (current.avatar.height - before.avatar.height).to_f32() * alpha;
        let moved = position
            .with_y(0.0)
            .distance(transform.translation.with_y(0.0))
            .min(1.0);
        let step_distance = motion_profile.step_distance.clamp(0.2, 4.0);
        let contacts = peer
            .gait
            .advance(moved, step_distance, !current.avatar.flight.airborne);
        for _ in 0..contacts {
            sound.play(crate::sound::Cue::Step);
        }
        transform.translation = position;
        let turn = (current.heading - before.heading).to_f32();
        let turn =
            (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let heading = before.heading.to_f32() + turn * alpha;
        transform.rotation = Quat::from_rotation_y(heading);
        let airborne = current.avatar.flight.airborne;
        let velocity = if airborne {
            Vec3::ZERO
        } else {
            Vec3::new(
                current.avatar.velocity.x.to_f32(),
                0.0,
                current.avatar.velocity.y.to_f32(),
            )
        };
        let (speed, turn) =
            peer.motion
                .advance_with_profile(velocity, heading, time.delta_secs(), motion_profile);
        let swing = current.avatar.swing.as_ref();
        let attack = swing.and_then(|s| {
            attack(
                s,
                ticks,
                library.has_feature(voxel_assets::SceneFeature::SwiftSword),
            )
        });
        if let (Some(swing), Some(_)) = (swing, attack) {
            if peer.heard != swing.started {
                peer.heard = swing.started;
                sound.play(crate::sound::Cue::Swing);
            }
        }
        let dig = current.avatar.dig.as_ref().map(|d| dig_age(&game, d));
        poses.push((
            entity,
            current.avatar.weapon,
            Pose {
                motion: voxel_assets::CharacterMotion {
                    step_distance: motion_profile.step_distance / transform.scale.x.max(0.1),
                    ..motion_profile
                },
                stride: peer.gait.phase,
                speed,
                turn,
                breathe,
                airborne,
                jump: jump_motion(
                    current.avatar.jump,
                    ticks,
                    current.avatar.height.to_f32(),
                    current.avatar.flight.support.to_f32(),
                ),
                attack,
                dig,
            },
        ));
    }
    for (entity, part, mut transform) in &mut parts {
        // Part pivots hang off the root or the body: find the peer above.
        let pose = parents
            .iter_ancestors(entity)
            .find_map(|e| poses.iter().find(|(p, ..)| *p == e).map(|(.., pose)| pose));
        if let Some(pose) = pose {
            blend_pose(
                &mut transform,
                pose.place(part.kind, part.rest),
                time.delta_secs(),
                pose.motion.pose_response,
            );
        }
    }
    for (root, _, pose) in &poses {
        if !pose.motion.plant_feet || pose.speed <= 0.05 || pose.airborne || pose.jump.is_some()
            || pose.attack.is_some() || pose.dig.is_some() {
            continue;
        }
        let mut joints: HashMap<Entity, JointSnapshot> = HashMap::new();
        for (entity, part, transform) in &mut parts {
            if parents.iter_ancestors(entity).any(|ancestor| ancestor == *root) {
                joints.insert(entity, (part.kind, part.rest, *transform,
                    parents.get(entity).ok().map(ChildOf::parent)));
            }
        }
        for (entity, target) in grounded_foot_targets(&joints, *root, pose) {
            if let Ok((_, _, mut transform)) = parts.get_mut(entity) {
                *transform = target;
            }
        }
        if let Some(contacts) = contacts.as_ref() {
            for (entity, target) in crate::character::actor_leg_targets(&joints, *root, pose, contacts) {
                if let Ok((_, _, mut transform)) = parts.get_mut(entity) {
                    *transform = target;
                }
            }
        }
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
    // The player's suit is only complete once the scene's style dresses it.
    if style.current().is_none() {
        return;
    }
    let (player, style) = (*player, style.current());
    for agent in others.into_iter().filter(|a| !shown.contains(a)) {
        commands.queue(move |world: &mut World| {
            let root = copy(world, player, None);
            world.entity_mut(root).insert(Peer {
                agent,
                style,
                gait: GaitClock::default(),
                heard: 0,
                motion: MotionState::default(),
            });
        });
    }
}

/// Other players for the control snapshot: where each stands and which
/// weapon models are showing.
pub fn snapshot(world: &mut World) -> Vec<serde_json::Value> {
    let shown: Vec<(Entity, Weapon)> = world
        .query::<(Entity, &PeerWeapon, &InheritedVisibility)>()
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

/// Copies an entity and its children: their meshes, materials, transforms
/// and visibility, with part pivots and exhausts as their peer versions.
/// Lights are left out, so each peer doesn't add shadow-casting lights.
fn copy(world: &mut World, source: Entity, parent: Option<Entity>) -> Entity {
    let e = world.entity(source);
    let transform = e.get::<Transform>().copied().unwrap_or_default();
    let visibility = e.get::<Visibility>().copied().unwrap_or_default();
    let mesh = e.get::<Mesh3d>().cloned();
    let material = e.get::<MeshMaterial3d<StandardMaterial>>().cloned();
    let part = e.get::<Part>().map(|p| PeerPart {
        kind: p.kind,
        rest: p.rest,
    });
    let exhaust = e.contains::<Exhaust>();
    let weapon = e.get::<WeaponVisual>().map(|w| PeerWeapon(w.0));
    let no_shadow = e.contains::<NotShadowCaster>();
    let children: Vec<Entity> = e
        .get::<Children>()
        .map_or_else(Vec::new, |c| c.iter().collect());
    let mut copied = world.spawn((transform, visibility));
    if let Some(mesh) = mesh {
        copied.insert(mesh);
    }
    if let Some(material) = material {
        copied.insert(material);
    }
    if let Some(part) = part {
        copied.insert(part);
    }
    if exhaust {
        copied.insert(PeerExhaust);
    }
    if let Some(weapon) = weapon {
        copied.insert(weapon);
    }
    if no_shadow {
        copied.insert(NotShadowCaster);
    }
    if let Some(parent) = parent {
        copied.insert(ChildOf(parent));
    }
    let id = copied.id();
    for child in children {
        // Lights stay with the player; so does the backpack (each peer gets
        // its own, see `piles::render`).
        let child_ref = world.entity(child);
        if child_ref.contains::<PointLight>()
            || child_ref.contains::<SpotLight>()
            || child_ref.contains::<crate::piles::CarryVisual>()
        {
            continue;
        }
        copy(world, child, Some(id));
    }
    id
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
        }
    }

    #[test]
    fn a_swing_plays_from_its_start_tick_and_ends() {
        let s = swing(0);
        assert_eq!(attack(&s, 100.0, false).unwrap().age, 0.0);
        let mid = attack(&s, at(0.2), false).unwrap();
        assert!((mid.age - 0.2).abs() < 1e-4 && mid.impact_angle.is_none());
        assert!((mid.aim - Vec3::X).length() < 1e-4);
        assert!(attack(&s, at(ATTACK_END) + 0.1, false).is_none());
    }

    #[test]
    fn a_hit_rebounds_from_where_the_blade_was() {
        let s = swing(1);
        let at_hit = attack(&swing(0), at(RESOLVE), false).unwrap().angle();
        let rebound = attack(&s, at(RESOLVE + 0.05), false).unwrap();
        assert_eq!(rebound.impact_angle, Some(at_hit));
        assert!((rebound.age - 0.05).abs() < 1e-4);
        assert!(attack(&s, at(RESOLVE + REBOUND_END) + 0.1, false).is_none());
    }
}

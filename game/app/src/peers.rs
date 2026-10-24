//! Other players' avatars in a networked game: a copy of the local player's
//! exosuit (as the scene's style dresses it) per avatar this machine doesn't
//! control, placed where the simulation has it between ticks and posed like
//! the local player, swings included.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use content::scenario::Weapon;
use sim::settlement::avatar::Swing;

use crate::gameplay::{Attack, Part, PartKind, Player, Pose, ATTACK_END, REBOUND_END, WALK_SPEED};
use crate::jetpack::Exhaust;
use crate::lockstep::WorldMirror;
use crate::style::WeaponVisual;

#[derive(Component)]
pub struct Peer {
    agent: u16,
    /// The style the copy was made in; a restyle makes a new copy.
    style: Option<usize>,
    /// Walk cycle phase, advanced by distance moved.
    stride: f32,
    /// The start tick of the last swing heard, so each whooshes once.
    heard: u64,
    /// Meters walked since the last footstep.
    foot_distance: f32,
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
fn attack(swing: &Swing, ticks: f32) -> Option<Attack> {
    let seconds = |from: u64| ((ticks - from as f32) / content::TICKS_PER_SECOND as f32).max(0.0);
    let mut attack = Attack::new(Vec3::new(swing.aim.x.to_f32(), 0.0, swing.aim.y.to_f32()), swing.weapon);
    let rebound = swing.resolved && swing.hits > 0 && matches!(swing.weapon, Weapon::Sword | Weapon::Spear);
    if rebound {
        // The blade stops where it was when the hit resolved.
        attack.age = swing.resolve_at.saturating_sub(swing.started) as f32 / content::TICKS_PER_SECOND as f32;
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
    mirror: Res<WorldMirror>,
    scene: Res<crate::scene_view::SceneView>,
    time: Res<Time>,
    sound: Res<crate::sound::Sound>,
    player: Single<Entity, With<Player>>,
    mut peers: Query<(Entity, &mut Peer, &mut Transform), (Without<PeerPart>, Without<Player>)>,
    mut parts: Query<(Entity, &PeerPart, &mut Transform), Without<Peer>>,
    parents: Query<&ChildOf>,
    mut exhausts: Query<(&ChildOf, &mut Visibility), (With<PeerExhaust>, Without<PeerWeapon>)>,
    mut weapons: Query<(Entity, &PeerWeapon, &mut Visibility), Without<PeerExhaust>>,
) {
    let me = mirror.player();
    let others: Vec<u16> = mirror.current.as_ref().map_or_else(Vec::new, |s| {
        (0..s.agents.len() as u16).filter(|&i| s.agents[i as usize].controlled && Some(i) != me).collect()
    });
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    // Render time in ticks: views sit `alpha` of the way from the previous
    // tick's state to the current one's.
    let ticks = mirror.tick as f32 - 1.0 + mirror.alpha;
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
        let (from, to) = (mirror.to_view(before.position), mirror.to_view(current.position));
        let alpha = if from.distance(to) > 1.5 { 1.0 } else { mirror.alpha };
        let mut position = from.lerp(to, alpha);
        position.y = before.avatar.height.to_f32() + (current.avatar.height - before.avatar.height).to_f32() * alpha;
        let moved = position.with_y(0.0).distance(transform.translation.with_y(0.0)).min(1.0);
        peer.stride += moved * 8.5;
        if !current.avatar.flight.airborne {
            peer.foot_distance += moved;
            if peer.foot_distance > 0.72 {
                peer.foot_distance -= 0.72;
                sound.play(crate::sound::Cue::Step);
            }
        }
        transform.translation = position;
        let turn = (current.heading - before.heading).to_f32();
        let turn = (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        transform.rotation = Quat::from_rotation_y(before.heading.to_f32() + turn * alpha);
        let airborne = current.avatar.flight.airborne;
        let speed = if airborne { 0.0 } else { (current.avatar.velocity.length().to_f32() / WALK_SPEED).min(1.4) };
        let swing = current.avatar.swing.as_ref();
        let attack = swing.and_then(|s| attack(s, ticks));
        if let (Some(swing), Some(_)) = (swing, attack) {
            if peer.heard != swing.started {
                peer.heard = swing.started;
                sound.play(crate::sound::Cue::Swing);
            }
        }
        let dig = current.avatar.dig.as_ref().map(|d| crate::gameplay::dig_age(&mirror, d));
        poses.push((entity, current.avatar.weapon, Pose { stride: peer.stride, speed, breathe, airborne, attack, dig }));
    }
    for (entity, part, mut transform) in &mut parts {
        // Part pivots hang off the root or the body: find the peer above.
        let pose = parents.iter_ancestors(entity).find_map(|e| poses.iter().find(|(p, ..)| *p == e).map(|(.., pose)| pose));
        if let Some(pose) = pose {
            *transform = pose.place(part.kind, part.rest);
        }
    }
    for (parent, mut visibility) in &mut exhausts {
        if let Some((.., pose)) = poses.iter().find(|(e, ..)| *e == parent.parent()) {
            *visibility = if pose.airborne { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    for (entity, weapon, mut visibility) in &mut weapons {
        let held = parents.iter_ancestors(entity).find_map(|e| poses.iter().find(|(p, ..)| *p == e).map(|(_, held, _)| *held));
        if let Some(held) = held {
            *visibility = if weapon.0 == held { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    // The world's scene dressed the player's suit when it started.
    let (player, style) = (*player, Some(scene.index));
    for agent in others.into_iter().filter(|a| !shown.contains(a)) {
        commands.queue(move |world: &mut World| {
            let root = copy(world, player, None);
            world.entity_mut(root).insert((Peer { agent, style, stride: 0.0, heard: 0, foot_distance: 0.0 }, service::WorldScoped::default()));
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
    let held: Vec<(Entity, Weapon)> = shown.iter().filter_map(|&(e, w)| Some((owner(world, e)?, w))).collect();
    let mut peers = world.query_filtered::<(Entity, &Transform), With<Peer>>();
    peers
        .iter(world)
        .map(|(e, t)| {
            let mut weapons: Vec<_> = held.iter().filter(|(p, _)| *p == e).map(|(_, w)| format!("{w:?}")).collect();
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
    let part = e.get::<Part>().map(|p| PeerPart { kind: p.kind, rest: p.rest });
    let exhaust = e.contains::<Exhaust>();
    let weapon = e.get::<WeaponVisual>().map(|w| PeerWeapon(w.0));
    let no_shadow = e.contains::<NotShadowCaster>();
    let children: Vec<Entity> = e.get::<Children>().map_or_else(Vec::new, |c| c.iter().collect());
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
        if child_ref.contains::<PointLight>() || child_ref.contains::<SpotLight>() || child_ref.contains::<crate::piles::CarryVisual>() {
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
        assert_eq!(attack(&s, 100.0).unwrap().age, 0.0);
        let mid = attack(&s, at(0.2)).unwrap();
        assert!((mid.age - 0.2).abs() < 1e-4 && mid.impact_angle.is_none());
        assert!((mid.aim - Vec3::X).length() < 1e-4);
        assert!(attack(&s, at(ATTACK_END) + 0.1).is_none());
    }

    #[test]
    fn a_hit_rebounds_from_where_the_blade_was() {
        let s = swing(1);
        let at_hit = attack(&swing(0), at(RESOLVE)).unwrap().angle();
        let rebound = attack(&s, at(RESOLVE + 0.05)).unwrap();
        assert_eq!(rebound.impact_angle, Some(at_hit));
        assert!((rebound.age - 0.05).abs() < 1e-4);
        assert!(attack(&s, at(RESOLVE + REBOUND_END) + 0.1).is_none());
    }
}

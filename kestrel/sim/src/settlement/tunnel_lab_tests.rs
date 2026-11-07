//! The Tunnel Lab (V.2 of the verticality design, `kestrel/VERTICALITY.md`
//! V10): its map is the scene's own, the `volume:` section of
//! `tunnel_lab.ron`, built as `lockstep::scene_setup` builds it. Bodies
//! walk, fall, jump and fly in it by V.3's rules (V3).

use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::{AgentState, PileSetup, SceneSetup};
use crate::terrain::{Terrain, LAYER};
use crate::volume::{height, Volume, VolumeLayout, BODY_HEIGHT, HEADROOM};
use crate::{Command, CommandKind, PlayerId, Simulation, World};

pub(super) const SCENE: &str = include_str!("../../../app/assets/levels/tunnel_lab.ron");

/// The text of the scene's `key: Some(...)` section, inside the `Some`:
/// its parentheses matched past comments and strings.
pub(super) fn section(text: &str, key: &str) -> String {
    let open = format!("{key}: Some(");
    let start = text.find(&open).unwrap_or_else(|| panic!("no {open} in the scene")) + open.len();
    let (mut depth, mut chars) = (1, text[start..].char_indices().peekable());
    while let Some((at, c)) = chars.next() {
        match c {
            '/' if chars.peek().is_some_and(|&(_, n)| n == '/') => {
                chars.by_ref().find(|&(_, n)| n == '\n');
            }
            '"' => {
                chars.by_ref().find(|&(_, n)| n == '"');
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return text[start..start + at].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{key}'s section never closes")
}

pub(super) fn layout() -> VolumeLayout {
    ron::from_str(&section(SCENE, "volume")).unwrap()
}

/// The Tunnel Lab as the app makes it.
pub(super) fn tunnel_lab() -> Simulation {
    let layout = layout();
    let setup = SceneSetup {
        player: "exosuit".into(),
        extent: Fx::from_int(layout.extent),
        diggable: true,
        sloped: true,
        volume: Some(layout.build()),
        piles: Some(PileSetup { capacity: 12, dispensers: Vec::new(), piles: Vec::new(), collector: Some([Fx::from_int(-3), Fx::from_int(-4)]) }),
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// The tile `x`, `z` tiles from the scene's origin.
pub(super) fn tile(terrain: &Terrain, x: i32, z: i32) -> (i32, i32) {
    let center = Terrain::pit_tile(terrain.center);
    (center.0 + x, center.1 + z)
}

#[test]
fn the_tunnel_lab_volume_is_valid() {
    layout().validate().unwrap();
}

/// Over the cave's middle the bridge makes two floors: the cave's at 0 m
/// with 3 m to its ceiling, and the bridge's top at 5 m under open sky.
#[test]
fn the_bridge_has_two_floors_over_the_cave() {
    let sim = tunnel_lab();
    let volume = sim.world().terrain().volume.as_ref().expect("the tunnel lab has a volume");
    let spot = tile(sim.world().terrain(), -11, 13);
    assert_eq!(volume.floors(spot).collect::<Vec<_>>(), [0, 10]);
    assert_eq!(volume.headroom(spot, 0), Some(6));
    assert_eq!(volume.headroom(spot, 10), None);
    assert_eq!(height(10), Fx::from_int(5));
    // The stairs up its west side climb a layer a tile, from the ground to
    // the bridge's top.
    let tops: Vec<_> = (-28..=-18).map(|x| volume.top(tile(sim.world().terrain(), x, 13)).unwrap()).collect();
    assert_eq!(tops, (0..=10).collect::<Vec<_>>());
}

/// Through the mountain the tunnel's floor is at 0 m, 3 m wide, with 3 m
/// of headroom under the mountain all the way from mouth to mouth; the
/// mountain's top is 8 m up.
#[test]
fn the_tunnel_runs_through_the_mountain_with_headroom() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let volume = terrain.volume.as_ref().unwrap();
    for x in 3..=25 {
        for z in -1..=1 {
            let t = tile(terrain, x, z);
            assert_eq!(volume.floors(t).next(), Some(0), "the tunnel's floor at ({x}, {z})");
            // Roofed from 6 to 22, open cuts at the mouths.
            let headroom = volume.headroom(t, 0);
            if (6..=22).contains(&x) {
                assert_eq!(headroom, Some(6), "headroom at ({x}, {z})");
                assert!(headroom.unwrap() >= HEADROOM);
            } else {
                assert_eq!(headroom, None, "open sky at ({x}, {z})");
            }
        }
        // The walls beside it are solid at the tunnel's height.
        assert!(volume.is_solid(tile(terrain, x, 2), 0) && volume.is_solid(tile(terrain, x, -2), 0));
    }
    assert_eq!(volume.top(tile(terrain, 14, 4)), Some(16), "8 m up");
    // Ore inside: amber high up, blue in the tunnel's south wall, green deep.
    assert_eq!(volume.ore(tile(terrain, 12, 6), 10), Some(crate::mine::AMBER));
    assert_eq!(volume.ore(tile(terrain, 14, -2), 2), Some(crate::mine::BLUE));
    assert_eq!(volume.ore(tile(terrain, 17, 2), -3), Some(crate::mine::GREEN));
    assert_eq!(volume.ore(tile(terrain, 0, 0), -1), None, "plain stone");
    // The spawn is on the open ground at 0 m.
    let player = sim.world().settlement().unwrap().player().unwrap();
    let body = &sim.world().settlement().unwrap().agents[player as usize].body;
    assert_eq!(terrain.ground(body.position), Some(Fx::ZERO));
}

/// A dig from the spawn (open ground: nothing in front to take, V.4)
/// must not break the world.
#[test]
fn digging_in_the_tunnel_lab_does_not_panic() {
    let mut sim = tunnel_lab();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    sim.step(&[Command { player: PlayerId(0), kind: CommandKind::DigDown { agent } }]);
    for _ in 0..content::millis_to_ticks(5000) {
        sim.step(&[]);
    }
    sim.world().settlement().unwrap().validate().unwrap();
}

// Bodies with a height (V.3, V3 of the verticality design): in a volume an
// avatar stands on the floor under its feet, steps up at most a layer,
// falls off drops and keeps its head under ceilings.

/// Scene meters: `x` east and `z` south of the scene's origin.
pub(super) fn meters(sim: &Simulation, x: f64, z: f64) -> FxVec2 {
    let m = |v: f64| Fx::from_ratio((v * 1000.0).round() as i64, 1000);
    sim.world().terrain().center + FxVec2::new(m(x), m(z))
}

pub(super) fn me(sim: &Simulation) -> &AgentState {
    let s = sim.world().settlement().unwrap();
    &s.agents[s.player().unwrap() as usize]
}

pub(super) fn feet(sim: &Simulation) -> Fx {
    me(sim).avatar().unwrap().height
}

/// Where the avatar is, in scene meters from the origin.
pub(super) fn offset(sim: &Simulation) -> FxVec2 {
    me(sim).body.position - sim.world().terrain().center
}

/// Puts the avatar at `x`, `z` with its feet at `height`, standing still.
pub(super) fn place(sim: &mut Simulation, x: f64, z: f64, height: Fx) {
    let p = meters(sim, x, z);
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    let a = &mut s.agents[player];
    a.body.position = p;
    a.body.velocity = FxVec2::ZERO;
    let avatar = a.avatar_mut().unwrap();
    avatar.height = height;
    avatar.flight.support = height;
}

pub(super) fn command(sim: &mut Simulation, kind: CommandKind) {
    sim.step(&[Command { player: PlayerId(0), kind }]);
}

/// Walks `direction` until `done` or `seconds` pass, then stops and lets
/// the avatar settle for a second. Returns its position and feet every tick.
pub(super) fn walk(sim: &mut Simulation, direction: (i32, i32), seconds: u64, done: impl Fn(FxVec2) -> bool) -> Vec<(FxVec2, Fx)> {
    let agent = sim.world().settlement().unwrap().player().unwrap();
    let direction = FxVec2::new(Fx::from_int(direction.0), Fx::from_int(direction.1));
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction, run: false } });
    let mut trail = Vec::new();
    for _ in 0..content::millis_to_ticks(seconds * 1000) {
        trail.push((offset(sim), feet(sim)));
        if done(offset(sim)) {
            break;
        }
        sim.step(&[]);
    }
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::Stop });
    for _ in 0..content::millis_to_ticks(1000) {
        sim.step(&[]);
        trail.push((offset(sim), feet(sim)));
    }
    trail
}

/// A box of cells for [`small`]: solid or empty, its tiles from `min` to
/// `max` (from the center tile) and its layers `bottom` up to `top`.
pub(super) type Cells = (bool, (i32, i32), (i32, i32), i32, i32);

/// A small volume scene: 33 m of ground (layers -8 to 0, its floor at 0 m)
/// with `boxes` applied over it, spawning `player` at the center with its
/// feet at `spawn` meters.
pub(super) fn small(player: &str, boxes: &[Cells], spawn: Fx) -> Simulation {
    let mut volume = Volume::default();
    volume.fill_box((-16, -16), (16, 16), -8, 0);
    for &(solid, min, max, bottom, top) in boxes {
        if solid {
            volume.fill_box(min, max, bottom, top);
        } else {
            volume.clear_box(min, max, bottom, top);
        }
    }
    let setup = SceneSetup {
        player: player.into(),
        extent: Fx::from_int(16),
        diggable: true,
        sloped: true,
        volume: Some(volume),
        spawn: [Fx::ZERO, spawn, Fx::ZERO],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// From the spawn east through the tunnel, mouth to mouth and out: the
/// feet stay on the tunnel's floor at 0 m all the way, under the mountain.
#[test]
fn the_avatar_walks_through_the_tunnel_on_its_floor() {
    let mut sim = tunnel_lab();
    let trail = walk(&mut sim, (1, 0), 12, |p| p.x >= Fx::from_int(28));
    let center = sim.world().terrain().center;
    let mut under = 0;
    for &(p, z) in &trail {
        assert_eq!(z, Fx::ZERO, "on the tunnel's floor at {p:?}");
        if sim.world().terrain().ground(center + p).unwrap() > Fx::from_int(3) {
            under += 1;
        }
    }
    assert!(offset(&sim).x >= Fx::from_int(28), "out of the east mouth: {:?}", offset(&sim));
    assert!(under > 60, "under the mountain for {under} ticks");
    assert!(!me(&sim).avatar().unwrap().flight.airborne);
}

/// Into the cave under the bridge, the feet stay at 0 m; up the stairs and
/// over to the same spot, (-11, 13), they end 5 m up on the bridge.
#[test]
fn the_avatar_walks_under_the_bridge_and_climbs_onto_it() {
    let mut sim = tunnel_lab();
    let spot = tile(sim.world().terrain(), -11, 13);
    place(&mut sim, -11.0, 4.0, Fx::ZERO);
    let trail = walk(&mut sim, (0, 1), 6, |p| p.y >= Fx::from_int(13));
    assert!(trail.iter().all(|&(_, z)| z == Fx::ZERO), "under the bridge on the cave's floor");
    assert_eq!(Terrain::pit_tile(me(&sim).body.position), spot);
    assert_eq!(feet(&sim), Fx::ZERO);
    assert_eq!(sim.world().terrain().ground(me(&sim).body.position), Some(Fx::from_int(5)), "the bridge is over it");

    place(&mut sim, -31.0, 13.0, Fx::ZERO);
    let trail = walk(&mut sim, (1, 0), 10, |p| p.x >= Fx::from_int(-11));
    assert_eq!(Terrain::pit_tile(me(&sim).body.position), spot);
    assert_eq!(feet(&sim), Fx::from_int(5), "on the bridge");
    // Up the stairs a ramp at a time: never a layer in a tick.
    for pair in trail.windows(2) {
        assert!((pair[1].1 - pair[0].1).abs() < LAYER, "{pair:?}");
    }
}

/// A passage 1.5 m high (three layers) is a wall to a body 2 m tall; one
/// 2 m high lets it through.
#[test]
fn low_headroom_stops_a_walk() {
    for (top, through) in [(3, false), (4, true)] {
        let mut sim = small("exosuit", &[(true, (3, -3), (6, 3), 0, 8), (false, (3, -1), (6, 1), 0, top)], Fx::ZERO);
        let trail = walk(&mut sim, (1, 0), 4, |p| p.x >= Fx::from_int(9));
        assert!(trail.iter().all(|&(_, z)| z == Fx::ZERO));
        if through {
            assert!(offset(&sim).x >= Fx::from_int(9), "through a 2 m passage: {:?}", offset(&sim));
        } else {
            assert!(offset(&sim).x < Fx::from_ratio(5, 2), "stopped at a 1.5 m passage: {:?}", offset(&sim));
        }
    }
}

/// A step two layers (1 m) up is a wall; one a layer up is climbed.
#[test]
fn a_step_two_layers_up_is_a_wall() {
    for (top, climbs) in [(2, false), (1, true)] {
        let mut sim = small("exosuit", &[(true, (3, -3), (8, 3), 0, top)], Fx::ZERO);
        walk(&mut sim, (1, 0), 3, |p| p.x >= Fx::from_int(5));
        if climbs {
            assert!(offset(&sim).x >= Fx::from_int(5));
            assert_eq!(feet(&sim), LAYER, "on the step");
        } else {
            assert!(offset(&sim).x < Fx::from_ratio(5, 2), "stopped by the step: {:?}", offset(&sim));
            assert_eq!(feet(&sim), Fx::ZERO);
        }
    }
}

/// Walking off a ledge 2 m up falls, over some ticks, to the floor below,
/// with the jetpack's sink and with a jumper's gravity alike.
#[test]
fn walking_off_a_ledge_falls_to_the_floor_below() {
    for player in ["exosuit", "exosuit_moonlit"] {
        let mut sim = small(player, &[(true, (-3, -3), (3, 3), 0, 4)], Fx::from_int(2));
        assert_eq!(feet(&sim), Fx::from_int(2));
        let trail = walk(&mut sim, (1, 0), 8, |p| p.x >= Fx::from_int(6));
        assert!(trail.iter().any(|&(_, z)| z > Fx::ZERO && z < Fx::from_int(2)), "{player} fell, not snapped down");
        assert!(offset(&sim).x >= Fx::from_int(5), "{player}: {:?}", offset(&sim));
        assert_eq!(feet(&sim), Fx::ZERO, "{player} landed");
        assert!(!me(&sim).avatar().unwrap().flight.airborne, "{player} is on the ground");
    }
}

/// A jump under a ceiling 2.5 m up stops with the head at it, half a meter
/// up; without the ceiling it goes higher.
#[test]
fn a_jump_is_stopped_by_the_ceiling() {
    let mut peaks = Vec::new();
    for roofed in [true, false] {
        let room: &[Cells] = if roofed { &[(true, (-4, -4), (4, 4), 0, 8), (false, (-3, -3), (3, 3), 0, 5)] } else { &[] };
        let mut sim = small("exosuit_moonlit", room, Fx::ZERO);
        let agent = sim.world().settlement().unwrap().player().unwrap();
        command(&mut sim, CommandKind::Jump { agent });
        let mut peak = Fx::ZERO;
        for _ in 0..content::millis_to_ticks(1500) {
            sim.step(&[]);
            peak = peak.max(feet(&sim));
        }
        assert_eq!(feet(&sim), Fx::ZERO, "landed");
        peaks.push(peak);
    }
    assert_eq!(peaks[0], Fx::from_ratio(5, 2) - BODY_HEIGHT, "the head stops at the ceiling");
    assert!(peaks[1] > peaks[0], "{peaks:?}");
    assert_eq!(BODY_HEIGHT, height(HEADROOM), "a body is as tall as a floor's headroom");
}

/// The jetpack in the tunnel rises until the head meets the roof, 3 m up;
/// let go, the avatar sinks back to the floor.
#[test]
fn the_jetpack_is_stopped_by_the_tunnel_roof() {
    let mut sim = tunnel_lab();
    place(&mut sim, 14.0, 0.0, Fx::ZERO);
    let agent = sim.world().settlement().unwrap().player().unwrap();
    command(&mut sim, CommandKind::Jetpack { agent, enabled: true });
    let mut peak = Fx::ZERO;
    for _ in 0..content::millis_to_ticks(2000) {
        sim.step(&[]);
        peak = peak.max(feet(&sim));
    }
    assert_eq!(peak, Fx::from_int(3) - BODY_HEIGHT);
    command(&mut sim, CommandKind::Jetpack { agent, enabled: false });
    for _ in 0..content::millis_to_ticks(2000) {
        sim.step(&[]);
    }
    assert_eq!(feet(&sim), Fx::ZERO);
}

/// The client's predicted avatar walks into the tunnel by the same rules,
/// tick for tick.
#[test]
fn a_replayed_avatar_walks_into_the_tunnel_as_the_simulation_does() {
    let mut sim = tunnel_lab();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    let (settlement, terrain) = (sim.world().settlement().unwrap().clone(), sim.world().terrain().clone());
    let mut replay = settlement.replay_avatar(agent, sim.world().tick(), &terrain).unwrap();
    let go = CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true } };
    for tick in 0..content::millis_to_ticks(4000) {
        let turn = if tick == 0 { vec![go.clone()] } else { Vec::new() };
        replay.step_with(&turn);
        let turn: Vec<_> = turn.into_iter().map(|kind| Command { player: PlayerId(0), kind }).collect();
        sim.step(&turn);
        assert_eq!(replay.state(), me(&sim), "tick {tick}");
    }
    assert!(offset(&sim).x > Fx::from_int(10), "in the tunnel: {:?}", offset(&sim));
}

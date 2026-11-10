//! Digging in 3D (V.4 of the verticality design, `kestrel/VERTICALITY.md`
//! V5): in a volume scene a dig clears the `HEADROOM` cells of the tile in
//! front, from the digger's floor layer (level), a layer under it (down) or
//! a layer over it (up); never under the digger. Repeated down or up digs
//! make a stair that walks by V.3's rules, and ore cells drop onto piles.

use fx::{Fx, FxVec2};

use super::avatar::Dig;
use super::combat::Event;
use super::dig_tests::facing;
use super::tunnel_lab_tests::{command, feet, me, offset, place, small, tile, tunnel_lab, walk, Cells};
use crate::mine::{cell_yield, BLUE};
use crate::terrain::{DigRefusal, Terrain, LAYER};
use crate::volume::{DigDirection, Volume, HEADROOM};
use crate::{Command, CommandKind, PlayerId, Simulation};

const SIDES: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const DIRECTIONS: [DigDirection; 3] = [DigDirection::Level, DigDirection::Down, DigDirection::Up];

fn agent(sim: &Simulation) -> u16 {
    sim.world().settlement().unwrap().player().unwrap()
}

fn volume(sim: &Simulation) -> &Volume {
    sim.world().terrain().volume.as_ref().unwrap()
}

/// Turns the avatar to face along `side`.
fn face(sim: &mut Simulation, side: (i32, i32)) {
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    s.agents[player].body.heading = facing(side.0, side.1);
}

/// What a dig `direction`-wise would do now: its tile and first layer, or
/// why not.
fn would_dig(sim: &Simulation, direction: DigDirection) -> Result<((i32, i32), i32), DigRefusal> {
    let s = sim.world().settlement().unwrap();
    s.dig_volume_of(agent(sim), sim.world().tick(), sim.world().terrain(), direction).unwrap()
}

/// Sends a dig `direction`-wise and waits for it to finish, returning it
/// (none if it didn't start) and the events of every tick meanwhile.
fn dig(sim: &mut Simulation, direction: DigDirection) -> (Option<Dig>, Vec<Event>) {
    let agent = agent(sim);
    command(sim, CommandKind::DigFront { agent, direction });
    let mut events = sim.world().settlement().unwrap().events.clone();
    let started = me(sim).avatar().unwrap().dig.clone();
    while me(sim).avatar().unwrap().dig.is_some() {
        sim.step(&[]);
        events.extend(sim.world().settlement().unwrap().events.iter().cloned());
        sim.world().settlement().unwrap().validate().unwrap();
    }
    (started, events)
}

/// The avatar's floor layer under its feet.
fn floor_layer(sim: &Simulation) -> i32 {
    volume(sim).floor_at(me(sim).body.position, feet(sim)).unwrap().0
}

/// A 1 × 1 room 2 m high at the center tile, its floor at 0 m, in a block
/// of solid stone 6 m high round it (layers 0 to 12, 3 tiles each way).
fn room() -> Simulation {
    let boxes: &[Cells] = &[(true, (-3, -3), (3, 3), 0, 12), (false, (0, 0), (0, 0), 0, HEADROOM)];
    small("exosuit", boxes, Fx::ZERO)
}

/// From the room, each direction on each side clears exactly the four
/// cells of the tile in front from the floor's layer plus the direction's
/// offset: the floor there ends at that layer with 2 m over it.
#[test]
fn each_direction_from_each_side_clears_the_four_cells_in_front() {
    for side in SIDES {
        for direction in DIRECTIONS {
            let mut sim = room();
            face(&mut sim, side);
            let before = volume(&sim).clone();
            let revision = sim.world().terrain().revision;
            let front = tile(sim.world().terrain(), side.0, side.1);
            let bottom = direction.offset();
            assert_eq!(would_dig(&sim, direction), Ok((front, bottom)), "{side:?} {direction:?}");
            let (started, _) = dig(&mut sim, direction);
            assert_eq!(started.map(|d| (d.tile, d.layer)), Some((front, Some(bottom))), "{side:?} {direction:?}");
            let mut expected = before.clone();
            for layer in bottom..bottom + HEADROOM {
                assert!(expected.clear(front, layer), "{side:?} {direction:?}: layer {layer} was solid");
            }
            assert_eq!(volume(&sim), &expected, "{side:?} {direction:?}: only those cells");
            assert_eq!(volume(&sim).floors(front).collect::<Vec<_>>(), vec![bottom, 12], "{side:?} {direction:?}");
            assert_eq!(volume(&sim).headroom(front, bottom), Some(HEADROOM));
            assert!(sim.world().terrain().revision >= revision + HEADROOM as u64, "a bump per cell, for the view to re-mesh");
        }
    }
}

/// The plain dig (F, `DigDown`, M8.8) digs level where there is a wall in
/// front, and a step down where a level dig has nothing to take (flat open
/// ground); Ctrl+F and Alt+F keep their one direction.
#[test]
fn the_plain_dig_goes_level_into_a_wall_and_down_on_flat_ground() {
    let plain = |sim: &Simulation| {
        let s = sim.world().settlement().unwrap();
        s.dig_plain_of(agent(sim), sim.world().tick(), sim.world().terrain()).unwrap()
    };
    let dig_plain = |sim: &mut Simulation| {
        let agent = agent(sim);
        command(sim, CommandKind::DigDown { agent });
        let started = me(sim).avatar().unwrap().dig.clone();
        while me(sim).avatar().unwrap().dig.is_some() {
            sim.step(&[]);
            sim.world().settlement().unwrap().validate().unwrap();
        }
        started.map(|d| (d.tile, d.layer))
    };
    // In the room, a wall on every side: level.
    let mut sim = room();
    face(&mut sim, (1, 0));
    let front = tile(sim.world().terrain(), 1, 0);
    assert_eq!(plain(&sim), Ok((front, 0)));
    assert_eq!(dig_plain(&mut sim), Some((front, Some(0))));
    assert!((0..HEADROOM).all(|layer| !volume(&sim).is_solid(front, layer)));
    assert!(volume(&sim).is_solid(front, -1), "the floor stays");
    // Now that tile is clear level: the next plain dig there goes down.
    assert_eq!(would_dig(&sim, DigDirection::Level), Err(DigRefusal::Clear));
    assert_eq!(plain(&sim), Ok((front, -1)));
    assert_eq!(dig_plain(&mut sim), Some((front, Some(-1))));
    assert!(!volume(&sim).is_solid(front, -1), "a step down");
    // On flat open ground: down, as Ctrl+F; Alt+F is still refused.
    let mut sim = small("exosuit", &[], Fx::ZERO);
    face(&mut sim, (0, 1));
    let front = tile(sim.world().terrain(), 0, 1);
    assert_eq!(plain(&sim), would_dig(&sim, DigDirection::Down));
    assert_eq!(plain(&sim), Ok((front, -1)));
    assert_eq!(would_dig(&sim, DigDirection::Up), Err(DigRefusal::Clear));
    assert_eq!(dig_plain(&mut sim), Some((front, Some(-1))));
}

/// A dig never takes a tile any of the digger's footprint stands in: at a
/// tile's edge, the next tile is its own too.
#[test]
fn a_dig_is_never_under_the_digger() {
    let mut sim = small("exosuit", &[], Fx::ZERO);
    face(&mut sim, (1, 0));
    assert!(would_dig(&sim, DigDirection::Down).is_ok(), "from the tile's middle");
    place(&mut sim, 0.3, 0.0, Fx::ZERO);
    face(&mut sim, (1, 0));
    for direction in DIRECTIONS {
        assert_eq!(would_dig(&sim, direction), Err(DigRefusal::OwnTile), "{direction:?}");
    }
    let before = volume(&sim).clone();
    let (started, _) = dig(&mut sim, DigDirection::Down);
    assert_eq!(started, None);
    assert_eq!(volume(&sim), &before);
}

/// Refused: nothing left to dig, a tile outside the map, a hole through
/// the volume's bottom, and a tile by the collector.
#[test]
fn volume_digs_are_refused_as_the_ground_says() {
    // On open ground a level or up dig has nothing to take.
    let mut sim = small("exosuit", &[(false, (2, 0), (2, 0), -8, -1)], Fx::ZERO);
    face(&mut sim, (1, 0));
    assert_eq!(would_dig(&sim, DigDirection::Level), Err(DigRefusal::Clear));
    assert_eq!(would_dig(&sim, DigDirection::Up), Err(DigRefusal::Clear));
    // A column with only layer -1 left: digging it out goes through.
    place(&mut sim, 1.0, 0.0, Fx::ZERO);
    face(&mut sim, (1, 0));
    assert_eq!(would_dig(&sim, DigDirection::Down), Err(DigRefusal::Bottom));
    // The map's edge.
    place(&mut sim, 16.0, 0.0, Fx::ZERO);
    face(&mut sim, (1, 0));
    assert_eq!(would_dig(&sim, DigDirection::Down), Err(DigRefusal::Undiggable));

    // The Tunnel Lab's collector, at (-3, -4), keeps its ground.
    let mut sim = tunnel_lab();
    place(&mut sim, -3.0, -2.0, Fx::ZERO);
    face(&mut sim, (0, -1));
    // (The collector's obstacle disc reaches the tile.)
    assert_eq!(would_dig(&sim, DigDirection::Down), Err(DigRefusal::Blocked));
    let before = volume(&sim).clone();
    assert_eq!(dig(&mut sim, DigDirection::Down).0, None);
    assert_eq!(volume(&sim), &before);
}

/// Puts the avatar back in the middle of the tile it walked onto, on its
/// floor, facing `side`.
fn recenter(sim: &mut Simulation, side: (i32, i32)) {
    let center = Terrain::pit_center(Terrain::pit_tile(me(sim).body.position));
    let floor = sim.world().terrain().footprint_at(center, feet(sim), me(sim).body.base_radius).unwrap();
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    let a = &mut s.agents[player];
    a.body.position = center;
    a.body.velocity = FxVec2::ZERO;
    let avatar = a.avatar_mut().unwrap();
    avatar.height = floor;
    avatar.flight.support = floor;
    face(sim, side);
}

/// Each step of a walk changes the feet by less than a layer in a tick.
fn smooth(trail: &[(FxVec2, Fx)]) {
    for pair in trail.windows(2) {
        assert!((pair[1].1 - pair[0].1).abs() < LAYER, "{pair:?}");
    }
}

/// Three down digs in a row east, walking onto each step, make a stair
/// down three layers: the avatar walks down it and back up.
#[test]
fn three_down_digs_make_a_stair_to_walk_down_and_back_up() {
    let mut sim = small("exosuit", &[], Fx::ZERO);
    face(&mut sim, (1, 0));
    for step in 1..=3 {
        let (started, _) = dig(&mut sim, DigDirection::Down);
        assert!(started.is_some(), "step {step}");
        let target = Fx::from_int(step);
        walk(&mut sim, (1, 0), 3, |p| p.x >= target);
        assert_eq!(Terrain::pit_tile(me(&sim).body.position), tile(sim.world().terrain(), step, 0), "on step {step}");
        assert_eq!(floor_layer(&sim), -step, "a layer down a step");
        recenter(&mut sim, (1, 0));
    }
    let v = volume(&sim);
    let terrain = sim.world().terrain();
    for step in 1..=3 {
        assert_eq!(v.top(tile(terrain, step, 0)), Some(-step));
    }
    // Back up to the start, and down again: a layer at most a tick.
    let trail = walk(&mut sim, (-1, 0), 6, |p| p.x <= Fx::ZERO);
    smooth(&trail);
    assert!(offset(&sim).x <= Fx::ZERO, "back at the top: {:?}", offset(&sim));
    assert_eq!(floor_layer(&sim), 0);
    let trail = walk(&mut sim, (1, 0), 6, |p| p.x >= Fx::from_int(3));
    smooth(&trail);
    assert_eq!(floor_layer(&sim), -3, "at the bottom");
}

/// Three up digs into a hillside, walking onto each step, make a stair up
/// through it, each step's tunnel exactly 2 m high: the avatar walks up and
/// back down, and so it does with the jetpack held.
#[test]
fn three_up_digs_make_a_stair_up_through_a_hill() {
    let hill: &[Cells] = &[(true, (1, -3), (8, 3), 0, 16)];
    let mut sim = small("exosuit", hill, Fx::ZERO);
    face(&mut sim, (1, 0));
    for step in 1..=3 {
        let (started, _) = dig(&mut sim, DigDirection::Up);
        assert!(started.is_some(), "step {step}");
        let target = Fx::from_int(step);
        walk(&mut sim, (1, 0), 3, |p| p.x >= target);
        assert_eq!(floor_layer(&sim), step, "a layer up a step");
        recenter(&mut sim, (1, 0));
    }
    let terrain = sim.world().terrain();
    for step in 1..=3 {
        assert_eq!(volume(&sim).headroom(tile(terrain, step, 0), step), Some(HEADROOM), "step {step}");
    }
    let trail = walk(&mut sim, (-1, 0), 6, |p| p.x <= Fx::ZERO);
    smooth(&trail);
    assert_eq!(floor_layer(&sim), 0, "back down");
    // The jetpack held up the stair from its first step: capped under the
    // low roofs, it walks (V.3's note on ramps exactly HEADROOM high). (From
    // outside, hovering, its head doesn't fit the 2 m mouth.)
    place(&mut sim, 1.0, 0.0, LAYER);
    let agent = agent(&sim);
    command(&mut sim, CommandKind::Jetpack { agent, enabled: true });
    walk(&mut sim, (1, 0), 6, |p| p.x >= Fx::from_ratio(29, 10));
    assert!(offset(&sim).x >= Fx::from_ratio(29, 10), "the jetpack climbed the stair: {:?}", offset(&sim));
    command(&mut sim, CommandKind::Jetpack { agent, enabled: false });
    for _ in 0..content::millis_to_ticks(2000) {
        sim.step(&[]);
    }
    assert_eq!(floor_layer(&sim), 3);
}

/// A level dig into the Tunnel Lab's blue vein, from the tunnel: its three
/// blue cells each drop 3 to 6 blue crystal onto piles beside the tile,
/// flying there; the stone cell drops nothing (the scene has no stone).
#[test]
fn ore_cells_drop_their_yield_onto_piles() {
    let mut sim = tunnel_lab();
    place(&mut sim, 14.0, -1.0, Fx::ZERO);
    face(&mut sim, (0, -1));
    let front = tile(sim.world().terrain(), 14, -2);
    let ore: Vec<_> = (0..HEADROOM).map(|layer| volume(&sim).ore(front, layer)).collect();
    assert_eq!(ore, vec![None, Some(BLUE), Some(BLUE), Some(BLUE)]);
    let (started, events) = dig(&mut sim, DigDirection::Level);
    assert!(started.is_some());
    let expected: u32 = (1..HEADROOM).map(|layer| cell_yield(front, layer)).sum();
    assert!((9..=18).contains(&expected));
    let s = sim.world().settlement().unwrap();
    let lab = s.piles.as_ref().unwrap();
    let blue = lab.item(BLUE);
    let piled: u32 = lab.piles.values().filter(|p| p.item() == Some(blue)).map(|p| p.units()).sum();
    assert_eq!(piled, expected);
    assert_eq!(s.accounts[blue as usize].generated, expected as u64);
    let tosses = events.iter().filter(|e| matches!(e, Event::PileToss { material: BLUE, .. })).count();
    assert_eq!(tosses as u32, expected, "every unit flies onto its pile");
    for p in lab.piles.values() {
        assert!(super::nav::distance(p.position, Terrain::pit_center(front)) < Fx::from_int(3), "beside the tile: {:?}", p.position);
        // In the tunnel, on its floor (V.7), not on the mountain over it.
        let at = Terrain::pit_tile(p.position);
        assert_eq!(volume(&sim).floor_near(at, 0), Some(0), "on a tile with the tunnel's floor: {at:?}");
        assert_eq!(p.height, Fx::ZERO, "lying on the tunnel's floor");
        assert!(sim.world().terrain().height(p.position) > Fx::from_int(3), "under the mountain");
    }
    assert_eq!((0..HEADROOM).filter_map(|layer| volume(&sim).ore(front, layer)).count(), 0, "the ore is gone");
}

/// The client's predicted avatar starts and ends the dig as the simulation
/// does, tick for tick; the old `DigDown` digs level in a volume scene.
/// When the cells come out the confirmed terrain changes under the replay
/// (a down dig slopes the digger's own tile toward the step), so it starts
/// over from the confirmed state then, as the client's does.
#[test]
fn a_replayed_avatar_digs_as_the_simulation_does() {
    for kind in [
        CommandKind::DigFront { agent: 0, direction: DigDirection::Down },
        CommandKind::DigFront { agent: 0, direction: DigDirection::Up },
        CommandKind::DigDown { agent: 0 },
    ] {
        let mut sim = room();
        face(&mut sim, (0, 1));
        let agent = agent(&sim);
        let mut kind = kind;
        match &mut kind {
            CommandKind::DigFront { agent: a, .. } | CommandKind::DigDown { agent: a } => *a = agent,
            _ => unreachable!(),
        }
        let mut dug = false;
        for round in 0..2 {
            let (settlement, terrain) = (sim.world().settlement().unwrap().clone(), sim.world().terrain().clone());
            let mut replay = settlement.replay_avatar(agent, sim.world().tick(), &terrain).unwrap();
            for tick in 0..content::millis_to_ticks(3000) {
                let turn = if tick == 0 && round == 0 { vec![kind.clone()] } else { Vec::new() };
                replay.step_with(&turn);
                let turn: Vec<_> = turn.into_iter().map(|kind| Command { player: PlayerId(0), kind }).collect();
                let revision = sim.world().terrain().revision;
                sim.step(&turn);
                if sim.world().terrain().revision != revision {
                    break;
                }
                assert_eq!(replay.state(), me(&sim), "{kind:?} round {round} tick {tick}");
                dug |= me(&sim).avatar().unwrap().dig.is_some();
            }
        }
        assert!(dug, "{kind:?} dug");
        let front = tile(sim.world().terrain(), 0, 1);
        let bottom = match kind {
            CommandKind::DigFront { direction, .. } => direction.offset(),
            _ => 0,
        };
        assert!((bottom..bottom + HEADROOM).all(|layer| !volume(&sim).is_solid(front, layer)), "{kind:?}");
        // The avatar never moved for it.
        assert_eq!(offset(&sim), FxVec2::ZERO);
    }
}

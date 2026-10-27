//! Vehicles: the rover's driving rules on made-up ground, and players
//! driving it across the dig lab's mine.

use content::character::{CharacterDef, CharacterIx};
use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::vehicle::{self, Control, VehicleState};
use super::{PileSetup, SceneSetup, VehicleSetup};
use crate::terrain::{Disc, Surface, Terrain};
use crate::{Command, CommandKind, PlayerId, Simulation, World};

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn v(x: f64, y: f64) -> FxVec2 {
    FxVec2::new(fx(x), fx(y))
}

fn seconds(s: f64) -> u64 {
    (s * content::TICKS_PER_SECOND as f64).round() as u64
}

fn rover() -> (CharacterIx, CharacterDef) {
    let content = Content::builtin();
    let ix = content.names.character_index("rover").expect("the rover");
    (ix, content.rules.characters[ix as usize].clone())
}

/// Flat open ground 40 m across, centered on the origin.
fn flat() -> Terrain {
    Terrain::flat(FxVec2::ZERO, fx(20.0))
}

/// A raised box from (x0, y0) to (x1, y1), `height` up.
fn surface(x0: f64, y0: f64, x1: f64, y1: f64, height: f64) -> Surface {
    Surface { min: v(x0, y0), max: v(x1, y1), height: fx(height) }
}

/// The rover parked at `at`, facing +y, then driven with the stick held
/// toward `direction` for `ticks`.
fn drive(terrain: &Terrain, at: FxVec2, direction: FxVec2, ticks: u64) -> VehicleState {
    let (ix, def) = rover();
    let mut rover = VehicleState::new(ix, &def, at, Fx::ZERO, terrain).expect("parked on the ground");
    steer(&mut rover, terrain, &def, direction, ticks);
    rover
}

fn steer(rover: &mut VehicleState, terrain: &Terrain, def: &CharacterDef, direction: FxVec2, ticks: u64) {
    rover.control = Control { direction, boost: false };
    for _ in 0..ticks {
        let before = rover.wheels;
        vehicle::self_update(rover, terrain, def);
        let climb = def.vehicle.unwrap().climb;
        assert!(before.iter().zip(&rover.wheels).all(|(a, b)| *b - *a <= climb), "a wheel jumped up {before:?} -> {:?}", rover.wheels);
    }
}

/// Meters per second.
fn speed(rover: &VehicleState) -> f64 {
    rover.speed.to_f64() * content::TICKS_PER_SECOND as f64
}

#[test]
fn the_rover_speeds_up_turns_and_backs_up() {
    let terrain = flat();
    let (_, def) = rover();
    let top = def.movement.walk.to_f64();
    let rover = drive(&terrain, v(0.0, -15.0), v(0.0, 1.0), seconds(2.0));
    assert!((speed(&rover) - top).abs() < 0.01, "top speed after 2 s: {}", speed(&rover));
    assert!(rover.position.y > fx(-10.0) && rover.position.x.abs() < fx(0.001), "straight ahead: {:?}", rover.position);

    // Hold the stick to the right: it swings round to face +x while it rolls.
    let mut turning = rover.clone();
    steer(&mut turning, &terrain, &def, v(1.0, 0.0), seconds(2.0));
    let east = Fx::FRAC_PI_2;
    assert!((turning.heading - east).abs() < fx(0.01), "faces east: {}", turning.heading.to_f64());

    // Pull the stick straight back: it brakes, then backs up.
    let mut backing = rover.clone();
    steer(&mut backing, &terrain, &def, v(0.0, -1.0), seconds(0.25));
    assert!(speed(&backing) < top && speed(&backing) >= 0.0, "braking: {}", speed(&backing));
    steer(&mut backing, &terrain, &def, v(0.0, -1.0), seconds(2.0));
    let reverse = def.vehicle.unwrap().reverse.to_f64() * content::TICKS_PER_SECOND as f64;
    assert!((speed(&backing) + reverse).abs() < 0.01, "backing up: {}", speed(&backing));
    assert!(backing.heading.abs() < fx(0.001), "still facing ahead");

    // Let go: it rolls to a stop.
    steer(&mut backing, &terrain, &def, FxVec2::ZERO, seconds(1.0));
    assert_eq!(backing.speed, Fx::ZERO);
}

/// The mine's terraces are a quarter meter each: the wheels roll up them,
/// and the chassis rises over a few ticks rather than jumping.
#[test]
fn the_rover_rolls_up_a_terrace_step() {
    let mut terrain = flat();
    terrain.surfaces = vec![surface(-5.0, 0.0, 5.0, 19.0, 0.25)];
    let rover = drive(&terrain, v(0.0, -5.0), v(0.0, 1.0), seconds(3.0));
    assert!(rover.position.y > fx(5.0), "drove on: {:?}", rover.position);
    assert_eq!(rover.height, fx(0.25));
    assert!(rover.wheels.iter().all(|&w| w == fx(0.25)));
}

/// A mesa's meter-high cliff stops it dead, wheels short of the edge.
#[test]
fn a_cliff_stops_the_rover() {
    let mut terrain = flat();
    terrain.surfaces = vec![surface(-5.0, 0.0, 5.0, 19.0, 1.0)];
    let (_, def) = rover();
    let rover = drive(&terrain, v(0.0, -5.0), v(0.0, 1.0), seconds(3.0));
    let front = rover.position.y + def.vehicle.unwrap().wheelbase / Fx::from_int(2);
    assert!(front < Fx::ZERO && front > fx(-0.15), "front wheels at the foot of the cliff: {}", front.to_f64());
    assert_eq!(rover.speed, Fx::ZERO);
    assert_eq!(rover.height, Fx::ZERO);
}

/// Driving off a ledge, it falls under gravity and lands on the ground below.
#[test]
fn the_rover_drops_off_a_ledge() {
    let mut terrain = flat();
    terrain.surfaces = vec![surface(-5.0, -19.0, 5.0, 0.0, 1.5)];
    let (ix, def) = rover();
    let mut rover = VehicleState::new(ix, &def, v(0.0, -3.0), Fx::ZERO, &terrain).unwrap();
    assert_eq!(rover.height, fx(1.5));
    rover.control = Control { direction: v(0.0, 1.0), boost: false };
    let mut falling = 0;
    for _ in 0..seconds(4.0) {
        vehicle::self_update(&mut rover, &terrain, &def);
        if rover.fall > Fx::ZERO {
            falling += 1;
        }
    }
    assert!(falling >= 5, "it fell for {falling} ticks rather than snapping down");
    assert_eq!((rover.height, rover.fall), (Fx::ZERO, Fx::ZERO), "landed");
    assert!(rover.position.y > fx(5.0));
}

/// Rocks and the arena's edge stop it; it never ends up inside one.
#[test]
fn obstacles_stop_the_rover() {
    let mut terrain = flat();
    terrain.obstacles = vec![Disc { center: v(0.0, 0.0), radius: fx(1.0) }];
    let (_, def) = rover();
    let rover = drive(&terrain, v(0.0, -6.0), v(0.0, 1.0), seconds(3.0));
    let clearance = (rover.position - v(0.0, 0.0)).length();
    assert!(clearance >= fx(1.0) + def.body.radius, "inside the rock: {}", clearance.to_f64());
    assert!(rover.position.y > fx(-2.1), "it reached the rock");
    let edge = drive(&terrain, v(5.0, 15.0), v(0.0, 1.0), seconds(3.0));
    assert!(edge.position.y <= fx(20.0));
}

/// A pit one layer deep (half a meter) is a dip it drives through; three
/// layers deep is a hole its front wheels drop into and can't climb out of.
#[test]
fn the_rover_crosses_a_shallow_pit_but_not_a_deep_one() {
    let mut terrain = flat();
    terrain.diggable = true;
    for x in -2..=2 {
        terrain.deepen((x, 0));
    }
    let rover = drive(&terrain, v(0.0, -6.0), v(0.0, 1.0), seconds(3.0));
    assert!(rover.position.y > fx(5.0), "through the pit: {:?}", rover.position);
    for x in -2..=2 {
        terrain.deepen((x, 0));
        terrain.deepen((x, 0));
    }
    let stuck = drive(&terrain, v(0.0, -6.0), v(0.0, 1.0), seconds(3.0));
    assert!(stuck.position.y < fx(1.5), "stuck at the pit: {:?}", stuck.position);
}

/// The dig lab's mine, with a rover parked on the landing beside the spawn.
fn mine() -> SceneSetup {
    SceneSetup {
        player: "exosuit".into(),
        extent: Fx::from_int(128),
        spawn: [Fx::ZERO, Fx::ZERO, fx(2.0)],
        diggable: true,
        mine: Some(7),
        piles: Some(PileSetup { capacity: 12, dispensers: Vec::new(), piles: Vec::new(), collector: Some([fx(3.5), fx(-2.5)]) }),
        vehicles: vec![VehicleSetup { character: "rover".into(), position: [fx(1.2), fx(3.4)], heading: Fx::ZERO }],
        ..SceneSetup::default()
    }
}

fn world() -> World {
    World::new_scene(1, &Content::builtin(), &mine()).unwrap()
}

fn host(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn walk(direction: FxVec2, run: bool) -> CommandKind {
    CommandKind::MoveAvatar { agent: 0, intent: Intent::Walk { direction, run } }
}

/// A scripted drive: climb in, head north up the terraces with boost, swing
/// east, back up a little, and climb out.
fn script(tick: u64) -> Vec<Command> {
    let at = |s: f64| seconds(s);
    match tick {
        t if t == at(0.0) => vec![host(CommandKind::Board { agent: 0 })],
        t if t == at(0.2) => vec![host(walk(v(0.0, 1.0), true))],
        t if t == at(8.0) => vec![host(walk(v(1.0, 0.3).normalize_or_zero(), false))],
        t if t == at(12.0) => vec![host(walk(v(-1.0, 0.0), false))],
        t if t == at(13.0) => vec![host(walk(FxVec2::ZERO, false))],
        t if t == at(15.0) => vec![host(CommandKind::Board { agent: 0 })],
        _ => Vec::new(),
    }
}

#[test]
fn a_player_drives_the_rover_up_the_mines_terraces() {
    let world = world();
    let center = world.terrain().center;
    let mut sim = Simulation::from_world(world, 2);
    let s = |sim: &Simulation| sim.world().settlement().unwrap().clone();
    assert_eq!(s(&sim).vehicles.len(), 1);
    let parked = s(&sim).vehicles[0].clone();
    assert_eq!(parked.position, center + v(1.2, 3.4));

    sim.step(&script(0));
    assert_eq!(s(&sim).driving(0), Some(0), "the player is in reach of the seat");
    let (mut highest, mut climbed) = (Fx::ZERO, false);
    for tick in 1..seconds(15.0) {
        sim.step(&script(tick));
        let settlement = s(&sim);
        let (rover, player) = (&settlement.vehicles[0], &settlement.agents[0]);
        let seat = settlement.characters[rover.character as usize].vehicle.unwrap().seat;
        assert_eq!(settlement.driving(0), Some(0));
        assert_eq!(player.body.position, rover.position, "the driver rides along");
        assert_eq!(player.avatar().unwrap().height, rover.height + seat);
        climbed |= rover.height > highest && highest > Fx::ZERO;
        highest = highest.max(rover.height);
    }
    let settlement = s(&sim);
    let rover = &settlement.vehicles[0];
    let travelled = (rover.position - parked.position).length();
    eprintln!("travelled {} m, highest {} m, now {:?} at {} m", travelled.to_f64(), highest.to_f64(), rover.position - center, rover.height.to_f64());
    assert!(travelled > fx(25.0), "it got somewhere: {} m", travelled.to_f64());
    assert!(highest >= fx(0.5) && climbed, "up at least two terraces: {} m", highest.to_f64());

    sim.step(&script(seconds(15.0)));
    let settlement = s(&sim);
    let (rover, player) = (&settlement.vehicles[0], &settlement.agents[0]);
    assert_eq!(settlement.driving(0), None, "climbed out");
    let beside = (player.body.position - rover.position).length();
    assert!(beside > fx(1.0) && beside < fx(1.6), "beside the rover: {} m", beside.to_f64());
    assert_eq!(Some(player.avatar().unwrap().height), sim.world().terrain().footprint(player.body.position, fx(0.32)));

    // On foot again, the stick walks the player and the rover stays put.
    sim.step(&[host(walk(v(-1.0, 0.0), false))]);
    for _ in 0..seconds(1.0) {
        sim.step(&[]);
    }
    let after = s(&sim);
    assert_eq!(after.vehicles[0].position, rover.position);
    assert!((after.agents[0].body.position - player.body.position).length() > fx(1.0), "walked off");
}

/// Two players reach for the one seat on the same tick: the first in
/// command order gets it.
#[test]
fn the_first_player_to_reach_for_the_seat_gets_it() {
    let mut sim = Simulation::from_world(world(), 1);
    let guest = PlayerId(1);
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let settlement = sim.world().settlement().unwrap();
    let theirs = settlement.avatar_of(guest).unwrap();
    sim.step(&[Command { player: guest, kind: CommandKind::Board { agent: theirs } }, host(CommandKind::Board { agent: 0 })]);
    let settlement = sim.world().settlement().unwrap();
    assert_eq!(settlement.driving(0), Some(0));
    assert_eq!(settlement.driving(theirs as usize), None);
    // The guest steers nothing; the host's stick drives.
    sim.step(&[Command { player: guest, kind: CommandKind::MoveAvatar { agent: theirs, intent: Intent::Walk { direction: v(1.0, 0.0), run: false } } }]);
    assert_eq!(sim.world().settlement().unwrap().vehicles[0].control, Control::default());
}

/// Out of reach, nothing happens; a driver who dies leaves the rover where it is.
#[test]
fn boarding_needs_reach_and_death_frees_the_seat() {
    let mut setup = mine();
    setup.vehicles[0].position = [fx(10.0), fx(3.5)];
    let mut sim = Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1);
    sim.step(&[host(CommandKind::Board { agent: 0 })]);
    assert_eq!(sim.world().settlement().unwrap().driving(0), None, "too far away");

    let mut sim = Simulation::from_world(world(), 1);
    sim.step(&[host(CommandKind::Board { agent: 0 })]);
    let (settlement, terrain) = sim.world_mut().settlement_and_terrain_mut();
    settlement.agents[0].body.max_health = 3;
    settlement.agents[0].body.health = 3;
    settlement.hurt_player(0, 3, 1, terrain);
    assert_eq!(settlement.driving(0), None);
    assert_eq!(settlement.driver_of(0), None, "the seat is free");
}

/// The client's replay of a driving player matches the simulation tick
/// for tick: the vehicle is the driver's own to predict.
#[test]
fn driving_is_predicted_exactly() {
    let mut sim = Simulation::from_world(world(), 1);
    sim.step(&script(0));
    let mut rng = 0x5eed_u64;
    let mut next = |n: u64| {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng >> 33) % n
    };
    let mut commands: Vec<Vec<CommandKind>> = Vec::new();
    for _ in 0..seconds(12.0) {
        let mut turn = Vec::new();
        if next(15) == 0 {
            let direction = FxVec2::new(Fx::from_ratio(next(201) as i64 - 100, 100), Fx::from_ratio(next(201) as i64 - 100, 100));
            turn.push(walk(direction.normalize_or_zero(), next(2) == 0));
        }
        commands.push(turn);
    }
    // Predict 40 ticks ahead from every 30th confirmed tick.
    for (start, _) in commands.iter().enumerate().step_by(30) {
        let confirmed = copy(sim.world());
        let settlement = confirmed.settlement().unwrap();
        let mut replay = settlement.replay_avatar(0, confirmed.tick(), confirmed.terrain()).unwrap();
        let mut ahead = Simulation::from_world(copy(&confirmed), 1);
        for turn in commands.iter().skip(start).take(40) {
            assert!(replay.step_with(turn.iter()));
            ahead.step(&turn.iter().cloned().map(host).collect::<Vec<_>>());
            let real = ahead.world().settlement().unwrap();
            let (ix, predicted) = replay.vehicle().expect("still driving");
            assert_eq!(predicted, &real.vehicles[ix as usize], "the rover, from {start}");
            assert_eq!(replay.state().body.position, real.agents[0].body.position, "the driver, from {start}");
            assert_eq!(replay.state().avatar().unwrap().height, real.agents[0].avatar().unwrap().height);
        }
        for turn in commands.iter().skip(start).take(30) {
            sim.step(&turn.iter().cloned().map(host).collect::<Vec<_>>());
        }
    }
}

/// The scripted drive hashes the same on 1 and 4 threads, and continues the
/// same from a save made halfway.
#[test]
fn a_drive_is_deterministic_across_threads_and_saves() {
    let run = |threads: usize, save_at: Option<u64>| {
        let mut sim = Simulation::from_world(world(), threads);
        let mut hashes = Vec::new();
        for tick in 0..seconds(16.0) {
            if Some(tick) == save_at {
                let world = sim.into_world();
                let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
                let rules = world.rules().clone();
                let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), rules).unwrap();
                sim = Simulation::from_world(loaded, threads);
            }
            hashes.push(sim.step(&script(tick)).total);
        }
        hashes
    };
    let one = run(1, None);
    assert_eq!(one, run(4, None), "thread count changed the drive");
    assert_eq!(one, run(2, Some(seconds(6.0))), "a save mid-drive changed it");
}

/// A copy of a world, through its saved state.
fn copy(world: &World) -> World {
    World::from_state(world.to_state(), world.rules().clone()).unwrap()
}

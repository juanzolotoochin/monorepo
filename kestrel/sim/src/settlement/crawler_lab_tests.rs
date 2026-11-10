//! The Crawler Lab (CR.1 of the crawler, `kestrel/CRAWLER.md` C11): its map
//! is the scene's own `volume:` section of `crawler_lab.ron` and its rules
//! `crawler_lab_supply.ron`, built as `lockstep::scene_setup` builds them,
//! with the crawler parked beside the spawn. And the crawler walking it on
//! its feet (CR.2, C5-C7, `crawler`).

use content::character::{CharacterDef, CharacterIx};
use content::scenario::Scenario;
use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::crawler::{self, Gait, Pose, Standing};
use super::tunnel_lab_tests::{command, me, meters, section, tile};
use super::vehicle::{self, Control, VehicleState};
use super::{SceneSetup, VehicleSetup};
use crate::nav::heading_vector;
use crate::terrain::Terrain;
use crate::volume::VolumeLayout;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SCENE: &str = include_str!("../../../app/assets/levels/crawler_lab.ron");
const SUPPLY: &str = include_str!("../../../app/assets/levels/crawler_lab_supply.ron");

fn layout() -> VolumeLayout {
    ron::from_str(&section(SCENE, "volume")).unwrap()
}

fn seconds(s: f64) -> u64 {
    (s * content::TICKS_PER_SECOND as f64).round() as u64
}

/// The Crawler Lab as the app makes it.
fn crawler_lab() -> Simulation {
    assert!(SCENE.contains(r#"vehicles: [(character: "crawler", position: (2.5, 0.0, 0.0), heading: 0.0)]"#), "the scene's crawler");
    assert!(SCENE.contains(r#"player: "elder""#) && SCENE.contains("spawn: (0.0, 0.0, 0.0)"));
    let layout = layout();
    let setup = SceneSetup {
        scenario: Some(Scenario::parse(SUPPLY).unwrap()),
        player: "elder".into(),
        extent: Fx::from_int(layout.extent),
        sloped: true,
        volume: Some(layout.build()),
        vehicles: vec![VehicleSetup { character: "crawler".into(), position: [Fx::from_ratio(5, 2), Fx::ZERO], heading: Fx::ZERO }],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// Every feature is where the scene's comment says, at its height in
/// layers: the terraces and the 1.5 m step, the cliff, the trench, the pit
/// and the moat round its island.
#[test]
fn the_crawler_lab_has_every_kind_of_ground() {
    layout().validate().unwrap();
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let volume = terrain.volume.as_ref().expect("the crawler lab has a volume");
    let top = |x: i32, z: i32| volume.top(tile(terrain, x, z)).unwrap();
    assert_eq!([top(0, 0), top(9, 0), top(13, 0), top(20, 0)], [0, 1, 2, 3], "terraces of half a meter up to the plateau");
    assert_eq!([top(23, -7), top(23, -6)], [0, 3], "a 1.5 m step onto the plateau");
    assert_eq!([top(0, -17), top(0, -18)], [0, 4], "a 2 m cliff onto the mesa");
    assert_eq!([top(-10, 0), top(-11, 0), top(-12, 0), top(-13, 0)], [0, -4, -4, 0], "a trench 2 m wide and 2 m deep");
    assert_eq!([top(-21, 0), top(-22, 0), top(-24, 0), top(-25, 0)], [0, -6, -6, 0], "a pit 3 m wide and 3 m deep");
    assert_eq!([top(0, 13), top(0, 14), top(0, 15), top(0, 16), top(0, 26), top(0, 27)], [0, -4, -4, 0, 0, -4], "a moat 2 m wide round the island");
    assert_eq!([top(-8, 20), top(-7, 20), top(-6, 20)], [-4, -4, 0]);
    // Ground room for the crawler between the trench and the pit.
    assert!((-21..=-13).all(|x| (-8..=8).all(|z| top(x, z) == 0)));
}

/// The crawler is parked in the vehicles table beside the elder, on the
/// ground, with its definition's legs; the ghouls come.
#[test]
fn the_crawler_is_parked_beside_the_spawn() {
    let mut sim = crawler_lab();
    let center = sim.world().terrain().center;
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.vehicles.len(), 1);
    let crawler = &s.vehicles[0];
    let content = Content::builtin();
    assert_eq!(crawler.character, content.names.character_index("crawler").unwrap());
    let def = &s.characters[crawler.character as usize];
    assert!(def.crawler.is_some(), "a crawler, with legs");
    assert_eq!(crawler.position, center + FxVec2::new(Fx::from_ratio(5, 2), Fx::ZERO));
    assert_eq!((crawler.height, crawler.wheels), (Fx::ZERO, [Fx::ZERO; 4]));
    assert_eq!(me(&sim).body.position, center, "the elder at the spawn");
    assert_eq!(content.names.characters[s.agents[s.player().unwrap() as usize].character as usize], "character/elder");
    for _ in 0..seconds(15.0) {
        sim.step(&[]);
    }
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.threats.len(), 3, "three ghouls out");
    assert!(s.threats.iter().all(|t| s.characters[t.character as usize].chase.is_some()), "chasers");
}

/// G boards the cab: the pilot rides on the deck, 3 m up, and steers it;
/// G again climbs down beside it onto the ground.
#[test]
fn a_pilot_boards_drives_and_climbs_down() {
    let mut sim = crawler_lab();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    command(&mut sim, CommandKind::Board { agent });
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.driving(agent as usize), Some(0), "in reach of the cab");
    let deck = s.characters[s.vehicles[0].character as usize].crawler.unwrap().deck;
    let parked = s.vehicles[0].position;
    command(&mut sim, CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false } });
    for _ in 0..seconds(4.0) {
        sim.step(&[]);
        let s = sim.world().settlement().unwrap();
        let (crawler, pilot) = (&s.vehicles[0], &s.agents[agent as usize]);
        assert_eq!(pilot.body.position, crawler.position, "the pilot rides along");
        assert_eq!(pilot.avatar().unwrap().height, crawler.height + deck, "on the deck");
    }
    command(&mut sim, CommandKind::MoveAvatar { agent, intent: Intent::Stop });
    for _ in 0..seconds(2.0) {
        sim.step(&[]);
    }
    let crawler = sim.world().settlement().unwrap().vehicles[0].clone();
    assert!(crawler.position.y - parked.y > Fx::from_int(3), "it went south: {:?}", crawler.position - parked);
    assert_eq!(crawler.speed, Fx::ZERO);

    command(&mut sim, CommandKind::Board { agent });
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.driving(agent as usize), None, "climbed down");
    let beside = (me(&sim).body.position - crawler.position).length();
    assert!(beside > Fx::from_ratio(3, 2) && beside < Fx::from_ratio(5, 2), "beside it: {} m", beside.to_f64());
    assert_eq!(me(&sim).avatar().unwrap().height, Fx::ZERO, "on the ground");
}

fn crawler_def() -> (CharacterIx, CharacterDef) {
    let content = Content::builtin();
    let ix = content.names.character_index("crawler").unwrap();
    (ix, content.rules.characters[ix as usize].clone())
}

/// A crawler walked tick by tick off the scene, with what its feet did.
struct Walk {
    v: VehicleState,
    def: CharacterDef,
    /// Every lift: the tick and the leg.
    lifts: Vec<(u64, usize)>,
    /// The most feet in the air at once, walking and trotting.
    most_up: [usize; 2],
    /// The pairs of feet seen in the air together.
    pairs: Vec<Vec<usize>>,
    ticks: u64,
}

impl Walk {
    /// The crawler parked at scene meters `at` facing `heading`.
    fn new(sim: &Simulation, at: (f64, f64), heading: Fx) -> Walk {
        let (ix, def) = crawler_def();
        let v = VehicleState::new(ix, &def, meters(sim, at.0, at.1), heading, sim.world().terrain()).expect("parked on the ground");
        Walk { v, def, lifts: Vec::new(), most_up: [0; 2], pairs: Vec::new(), ticks: 0 }
    }

    fn legs(&self) -> &crawler::Legs {
        self.v.legs.as_ref().unwrap()
    }

    /// One tick with the stick at `control`, among `bodies`; every planted
    /// foot checked in reach and on a floor.
    fn step(&mut self, terrain: &Terrain, control: Control, bodies: &[Standing]) {
        self.v.control = control;
        let before: Vec<bool> = self.legs().feet.iter().map(|f| f.swing.is_some()).collect();
        vehicle::self_update_among(&mut self.v, terrain, &self.def, bodies);
        let c = self.def.crawler.unwrap();
        crawler::check_reach(&self.v, &c).unwrap_or_else(|e| panic!("tick {}: {e}", self.ticks));
        crawler::check_floors(&self.v, terrain).unwrap_or_else(|e| panic!("tick {}: {e}", self.ticks));
        let legs = self.v.legs.as_ref().unwrap();
        for (leg, f) in legs.feet.iter().enumerate() {
            if f.swing.is_some() && !before[leg] {
                self.lifts.push((self.ticks, leg));
            }
        }
        let up: Vec<usize> = legs.up().collect();
        let g = (legs.gait == Gait::Trot) as usize;
        self.most_up[g] = self.most_up[g].max(up.len());
        if up.len() == 2 && !self.pairs.contains(&up) {
            self.pairs.push(up);
        }
        self.ticks += 1;
    }

    /// Holds the stick toward `direction` for `seconds_held`.
    fn drive(&mut self, terrain: &Terrain, direction: FxVec2, boost: bool, seconds_held: f64) {
        for _ in 0..seconds(seconds_held) {
            self.step(terrain, Control { direction, boost }, &[]);
        }
    }

    /// Scene meters from the origin.
    fn at(&self, terrain: &Terrain) -> FxVec2 {
        self.v.position - terrain.center
    }

    fn floors(&self) -> [Fx; 4] {
        std::array::from_fn(|i| self.legs().feet[i].floor)
    }

    /// The legs in the order they lifted.
    fn order(&self) -> Vec<usize> {
        self.lifts.iter().map(|&(_, leg)| leg).collect()
    }
}

const SOUTH: Fx = Fx::ZERO;
const WEST: Fx = Fx::from_ratio(-31_415_926, 20_000_000);

/// Whether `order` runs round `wave`, each leg followed by the next.
fn in_wave(order: &[usize], wave: [usize; 4]) -> bool {
    let next = |leg: usize| wave[(wave.iter().position(|&l| l == leg).unwrap() + 1) % 4];
    order.windows(2).all(|w| w[1] == next(w[0]))
}

/// Meters per second.
fn speed(v: &VehicleState) -> f64 {
    v.speed.to_f64() * content::TICKS_PER_SECOND as f64
}

/// The walk lifts one foot at a time, in a wave (rear left, front left,
/// rear right, front right), so three are always down; the trot on boost
/// lifts the diagonal pairs, two down. Both keep up with the body at full
/// speed.
#[test]
fn the_walk_keeps_three_feet_down_and_the_trot_two() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut walk = Walk::new(&sim, (2.5, -10.0), SOUTH);
    walk.drive(terrain, heading_vector(SOUTH), false, 8.0);
    assert_eq!(walk.most_up, [1, 0], "one foot up at a time");
    assert!((speed(&walk.v) - 1.6).abs() < 0.01, "at walking speed: {}", speed(&walk.v));
    assert!(walk.at(terrain).y > Fx::ONE, "walked south: {:?}", walk.at(terrain));
    assert!(walk.lifts.len() >= 12 && in_wave(&walk.order(), [2, 0, 3, 1]), "every foot stepped, in a wave: {:?}", walk.order());

    let mut trot = Walk::new(&sim, (2.5, -12.0), SOUTH);
    trot.drive(terrain, heading_vector(SOUTH), true, 6.0);
    assert_eq!(trot.most_up[1], 2, "two feet up at a trot");
    assert!((speed(&trot.v) - 3.2).abs() < 0.01, "at running speed: {}", speed(&trot.v));
    assert_eq!(trot.pairs, [vec![0, 3], vec![1, 2]], "diagonal pairs");
}

/// It climbs the plateau's 1.5 m north face, front feet first, the body
/// rising with them; and climbs down it, the body sinking so the low feet
/// stay in reach.
#[test]
fn the_crawler_walks_up_and_down_the_step() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let step = Fx::from_ratio(3, 2);
    let mut up = Walk::new(&sim, (23.0, -16.0), SOUTH);
    let mut halfway = false;
    for _ in 0..seconds(12.0) {
        up.step(terrain, Control { direction: heading_vector(SOUTH), boost: false }, &[]);
        let f = up.floors();
        halfway |= f[0] == step && f[1] == step && f[2] == Fx::ZERO && f[3] == Fx::ZERO;
    }
    assert!(halfway, "front feet up, rear feet down");
    assert_eq!(up.floors(), [step; 4], "on the plateau: {:?}", up.at(terrain));
    assert_eq!(up.v.height, step);

    let mut down = Walk::new(&sim, (23.0, 2.0), Fx::PI);
    assert_eq!(down.v.height, step);
    let mut lowest = step;
    for _ in 0..seconds(12.0) {
        down.step(terrain, Control { direction: heading_vector(Fx::PI), boost: false }, &[]);
        let f = down.floors();
        if f[0] == Fx::ZERO && f[1] == Fx::ZERO && f[2] == step && f[3] == step {
            lowest = lowest.min(down.v.height);
        }
    }
    assert!(lowest < step - Fx::HALF, "it sank with its front feet: {}", lowest.to_f64());
    assert_eq!(down.floors(), [Fx::ZERO; 4], "down on the ground: {:?}", down.at(terrain));
    assert_eq!(down.v.height, Fx::ZERO);
}

/// West from the spawn it steps across the 2 m trench, every foot from one
/// side to the other and never into it, then stops at the 3 m pit and
/// stays stopped while the stick is held; backing off, it walks away.
#[test]
fn the_crawler_crosses_the_trench_and_stops_at_the_pit() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut walk = Walk::new(&sim, (-4.0, 0.0), WEST);
    let x = |p: FxVec2| (p - terrain.center).x;
    let past = |walk: &Walk| walk.legs().feet.iter().all(|f| f.swing.is_none() && x(f.spot) < Fx::from_ratio(-25, 2));
    while !past(&walk) && walk.ticks < seconds(20.0) {
        walk.step(terrain, Control { direction: heading_vector(WEST), boost: false }, &[]);
        assert_eq!(walk.floors(), [Fx::ZERO; 4], "never in the trench: {:?}", walk.at(terrain));
    }
    assert!(past(&walk), "every foot past the trench: {:?}", walk.at(terrain));

    walk.drive(terrain, heading_vector(WEST), false, 15.0);
    let stopped = walk.v.position;
    walk.drive(terrain, heading_vector(WEST), false, 5.0);
    assert_eq!(walk.v.position, stopped, "stopped at the pit: {:?}", walk.at(terrain));
    assert!(walk.v.speed.abs() < Fx::from_ratio(1, 1000));
    assert!(walk.legs().feet.iter().all(|f| x(f.spot) > Fx::from_ratio(-43, 2)), "no foot over the pit");
    assert_eq!(walk.floors(), [Fx::ZERO; 4]);

    walk.drive(terrain, heading_vector(WEST + Fx::PI), false, 6.0);
    assert!(walk.v.position.x - stopped.x > Fx::from_int(2), "backed off: {:?}", walk.at(terrain));
}

/// North from the spawn it stops at the mesa's 2 m cliff, its body short of
/// the face and every foot on the ground.
#[test]
fn the_cliff_stops_the_crawler() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut walk = Walk::new(&sim, (0.0, -6.0), Fx::PI);
    walk.drive(terrain, heading_vector(Fx::PI), false, 12.0);
    let stopped = walk.v.position;
    walk.drive(terrain, heading_vector(Fx::PI), false, 3.0);
    assert_eq!(walk.v.position, stopped, "stopped");
    let length = walk.def.crawler.unwrap().length;
    let front = walk.at(terrain).y - length / Fx::from_int(2);
    assert!(front >= Fx::from_ratio(-351, 20) && front < Fx::from_int(-16), "its front at the cliff: {}", front.to_f64());
    assert_eq!(walk.floors(), [Fx::ZERO; 4], "no foot on the mesa");
    assert_eq!(walk.v.height, Fx::ZERO);
}

/// A foot never lands on an elder or a sprite: with bodies standing where
/// its feet landed before, it finds other footholds and walks on.
#[test]
fn a_foot_never_lands_on_a_body() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut free = Walk::new(&sim, (2.5, -10.0), SOUTH);
    let mut landed = Vec::new();
    for _ in 0..seconds(6.0) {
        free.step(terrain, Control { direction: heading_vector(SOUTH), boost: false }, &[]);
        landed.extend(free.legs().feet.iter().filter_map(|f| f.swing.as_ref().filter(|s| s.progress == 0).map(|s| s.to)));
    }
    let bodies: Vec<Standing> = landed.iter().map(|&position| Standing { position, radius: Fx::from_ratio(3, 10), height: Fx::ZERO }).collect();
    assert!(bodies.len() >= 8);
    let mut walk = Walk::new(&sim, (2.5, -10.0), SOUTH);
    let parked: Vec<FxVec2> = walk.legs().feet.iter().map(|f| f.spot).collect();
    for _ in 0..seconds(6.0) {
        walk.step(terrain, Control { direction: heading_vector(SOUTH), boost: false }, &bodies);
        // Feet where it parked were never put down there.
        for f in walk.legs().feet.iter().filter(|f| f.swing.is_some() || !parked.contains(&f.spot)) {
            let spot = f.swing.as_ref().map_or(f.spot, |s| s.to);
            for b in &bodies {
                assert!((spot - b.position).length() >= b.radius + Fx::from_ratio(2, 5), "a foot on a body at {:?}", b.position - terrain.center);
            }
        }
    }
    assert!(walk.at(terrain).y > Fx::from_int(-5), "it walked on: {:?}", walk.at(terrain));
}

/// With the stick held off to its side it turns in place: the body never
/// moves, and the feet step sideways round its middle.
#[test]
fn the_crawler_turns_in_place() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut walk = Walk::new(&sim, (2.5, -2.0), SOUTH);
    let start = walk.v.position;
    for _ in 0..seconds(8.0) {
        let side = heading_vector(walk.v.heading + Fx::FRAC_PI_2);
        walk.step(terrain, Control { direction: side, boost: false }, &[]);
        assert_eq!(walk.v.position, start, "in place");
    }
    let turned = walk.v.heading.to_f64().to_degrees();
    assert!(turned > 150.0, "turned {turned}°");
    let mut stepped = [0; 4];
    for &(_, leg) in &walk.lifts {
        stepped[leg] += 1;
    }
    assert!(stepped.iter().all(|&n| n >= 2), "every foot stepped round: {stepped:?}");
    // Let go: the feet settle near their resting spots at the new heading.
    walk.drive(terrain, FxVec2::ZERO, false, 4.0);
    let c = walk.def.crawler.unwrap();
    for (i, f) in walk.legs().feet.iter().enumerate() {
        assert!((f.spot - Pose::of(&walk.v).point(c.stance, i)).length() <= Fx::HALF, "foot {i} at rest");
    }
}

/// Pulling the stick back it backs up on the gait reversed: the wave runs
/// front right, rear right, front left, rear left.
#[test]
fn the_crawler_backs_up_on_the_gait_reversed() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut walk = Walk::new(&sim, (2.5, 0.0), SOUTH);
    let start = walk.v.position;
    walk.drive(terrain, heading_vector(Fx::PI), false, 8.0);
    assert!(walk.legs().backing);
    assert!(start.y - walk.v.position.y > Fx::from_int(3), "backed north: {:?}", walk.at(terrain));
    assert!(walk.v.speed < Fx::ZERO);
    assert_eq!(walk.most_up, [1, 0]);
    assert!(walk.lifts.len() >= 8 && in_wave(&walk.order(), [1, 3, 0, 2]), "the wave reversed: {:?}", walk.order());
}

/// A pilot's drive: board, then random sticks, boosting now and then.
fn pilot_script(ticks: u64, seed: u64) -> Vec<Vec<CommandKind>> {
    let mut rng = seed;
    let mut next = |n: u64| {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng >> 33) % n
    };
    (0..ticks)
        .map(|tick| {
            if tick == 0 {
                return vec![CommandKind::Board { agent: 0 }];
            }
            if next(40) != 0 {
                return Vec::new();
            }
            let direction = FxVec2::new(Fx::from_ratio(next(201) as i64 - 100, 100), Fx::from_ratio(next(201) as i64 - 100, 100)).normalize_or_zero();
            vec![CommandKind::MoveAvatar { agent: 0, intent: Intent::Walk { direction, run: next(3) == 0 } }]
        })
        .collect()
}

fn host(turn: &[CommandKind]) -> Vec<Command> {
    turn.iter().cloned().map(|kind| Command { player: PlayerId(0), kind }).collect()
}

/// A copy of a world, through its saved state.
fn copy(world: &World) -> World {
    World::from_state(world.to_state(), world.rules().clone()).unwrap()
}

/// The pilot's client predicts the crawler, feet and all, tick for tick.
#[test]
fn walking_is_predicted_exactly() {
    let mut sim = crawler_lab();
    let commands = pilot_script(seconds(20.0), 0xc4a1);
    sim.step(&host(&commands[0]));
    assert_eq!(sim.world().settlement().unwrap().driving(0), Some(0));
    let mut compared = 0;
    for start in (1..commands.len()).step_by(45) {
        let confirmed = copy(sim.world());
        let settlement = confirmed.settlement().unwrap();
        let mut replay = settlement.replay_avatar(0, confirmed.tick(), confirmed.terrain()).unwrap();
        let mut ahead = Simulation::from_world(copy(&confirmed), 1);
        for turn in commands.iter().skip(start).take(60) {
            replay.step_with(turn.iter());
            ahead.step(&host(turn));
            let real = ahead.world().settlement().unwrap();
            let (ix, predicted) = replay.vehicle().expect("still driving");
            assert_eq!(predicted, &real.vehicles[ix as usize], "the crawler, from {start}");
            assert_eq!(replay.state().avatar().unwrap().height, real.agents[0].avatar().unwrap().height);
            compared += 1;
        }
        for turn in commands.iter().skip(start).take(45) {
            sim.step(&host(turn));
        }
    }
    assert!(compared > 1000);
}

/// The same drive hashes the same on 1 and 4 threads, and continues the
/// same from a save made with a foot in the air (the feet and the clock
/// are saved).
#[test]
fn walking_is_deterministic_across_threads_and_saves() {
    let commands = pilot_script(seconds(16.0), 0x5eed);
    let up = |sim: &Simulation| sim.world().settlement().unwrap().vehicles[0].legs.as_ref().unwrap().up().next().is_some();
    let run = |threads: usize, save_at: Option<u64>| {
        let mut sim = Simulation::from_world(crawler_lab().into_world(), threads);
        let mut hashes = Vec::new();
        for (tick, turn) in commands.iter().enumerate() {
            if Some(tick as u64) == save_at {
                assert!(up(&sim), "saved mid-step");
                let world = sim.into_world();
                let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
                let rules = world.rules().clone();
                let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), rules).unwrap();
                assert_eq!(loaded.settlement().unwrap().vehicles, world.settlement().unwrap().vehicles, "the feet round-trip");
                sim = Simulation::from_world(loaded, threads);
            }
            hashes.push(sim.step(&host(turn)).total);
        }
        hashes
    };
    let one = run(1, None);
    assert_eq!(one, run(4, None), "thread count changed the walk");
    let mut probe = Simulation::from_world(crawler_lab().into_world(), 1);
    let mut save_at = None;
    for (tick, turn) in commands.iter().enumerate() {
        if tick > 300 && up(&probe) {
            save_at = Some(tick as u64);
            break;
        }
        probe.step(&host(turn));
    }
    assert!(save_at.is_some(), "a foot was in the air");
    assert_eq!(one, run(2, save_at), "a save mid-step changed it");
}

/// A short soak: a pilot drives round the lab at random for a minute, and
/// every tick the settlement validates, its feet in reach and on floors.
#[test]
fn a_minute_driving_round_the_lab_validates() {
    let mut sim = crawler_lab();
    let commands = pilot_script(seconds(60.0), 0x50a4);
    let mut lifts = 0;
    for (tick, turn) in commands.iter().enumerate() {
        let before = sim.world().settlement().unwrap().vehicles[0].legs.as_ref().unwrap().up().count();
        sim.step(&host(turn));
        let s = sim.world().settlement().unwrap();
        s.validate().unwrap_or_else(|e| panic!("tick {tick}: {e}"));
        s.validate_footholds(sim.world().terrain()).unwrap_or_else(|e| panic!("tick {tick}: {e}"));
        lifts += s.vehicles[0].legs.as_ref().unwrap().up().count().saturating_sub(before);
    }
    assert!(lifts > 40, "it walked: {lifts} lifts");
}

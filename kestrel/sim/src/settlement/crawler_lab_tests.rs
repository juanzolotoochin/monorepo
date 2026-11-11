//! The Crawler Lab (CR.1 of the crawler, `kestrel/CRAWLER.md` C11): its map
//! is the scene's own `volume:` section of `crawler_lab.ron` and its rules
//! `crawler_lab_supply.ron`, built as `lockstep::scene_setup` builds them,
//! with the crawler parked beside the spawn. And the crawler walking it on
//! its feet (CR.2, C5-C7, `crawler`).

use content::character::{CharacterDef, CharacterIx};
use content::scenario::Scenario;
use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::{Intent, Seat};
use super::crawler::{self, Gait, Pose, Standing};
use super::tunnel_lab_tests::{command, me, meters, section, tile};
use super::vehicle::{self, Control, VehicleState};
use super::{SceneSetup, VehicleSetup};
use crate::nav::{angle_between, heading_vector};
use crate::terrain::Terrain;
use crate::volume::VolumeLayout;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SCENE: &str = include_str!("../../../app/assets/levels/crawler_lab.ron");
const SUPPLY: &str = include_str!("../../../app/assets/levels/crawler_lab_supply.ron");

fn layout() -> VolumeLayout {
    ron::from_str(&section(SCENE, "volume")).unwrap()
}

pub(super) fn seconds(s: f64) -> u64 {
    (s * content::TICKS_PER_SECOND as f64).round() as u64
}

/// The Crawler Lab as the app makes it.
pub(super) fn crawler_lab() -> Simulation {
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

/// The lab for the turret (TU.1, T10): waves of ghouls on a timer from
/// two corners whose diagonals cross by the crawler, two sprites, and a
/// pile of stone; the crawler's gun shoots it. The ghouls of both sides
/// come within the gun's 7 m of the parked crawler.
#[test]
fn the_lab_has_waves_from_two_sides_sprites_and_stone() {
    let mut sim = crawler_lab();
    let center = sim.world().terrain().center;
    let s = sim.world().settlement().unwrap();
    let starts: std::collections::BTreeSet<_> = s.waves.iter().map(|w| ((w.position - center).x.to_f64() as i64, (w.position - center).y.to_f64() as i64)).collect();
    assert_eq!(starts.len(), 2, "two sides: {starts:?}");
    assert_eq!(s.waves.len(), 6);
    assert!(s.waves.iter().all(|w| w.nights.is_empty() && w.count >= 3), "timed waves");
    let mut firsts: Vec<u64> = s.waves.iter().map(|w| w.first).collect();
    firsts.sort();
    assert!(firsts.windows(2).all(|p| p[1] - p[0] >= seconds(20.0)), "a wave every 25 s or so: {firsts:?}");
    let total: u32 = s.waves.iter().map(|w| w.count).sum();
    let sprites: Vec<_> = s.agents.iter().filter(|a| a.sprite().is_some()).collect();
    assert_eq!(sprites.len(), 2, "two sprites");
    assert_eq!(s.stock("lab_stone", "stone"), 60, "a stone pile");
    let gun = s.characters[s.vehicles[0].character as usize].crawler.as_ref().unwrap().gun.clone().expect("a gun");
    assert_eq!(s.item_index(&gun.defence.ammo), s.item_index("stone"), "it shoots the pile's stone");
    let parked = s.vehicles[0].position;
    // Every ghoul of both sides passes within the gun's range.
    let mut nearest: std::collections::BTreeMap<u64, Fx> = Default::default();
    let mut sent = 0;
    for _ in 0..seconds(240.0) {
        sim.step(&[]);
        let s = sim.world().settlement().unwrap();
        s.validate().unwrap();
        for t in &s.threats {
            let d = (t.position - parked).length();
            nearest.entry(t.id).and_modify(|n| *n = (*n).min(d)).or_insert(d);
        }
        sent = s.waves.iter().map(|w| w.spawned).sum::<u32>();
    }
    assert_eq!(sent, total, "every wave sent");
    assert_eq!(nearest.len() as u32, total);
    assert!(nearest.values().all(|&d| d < gun.defence.range), "all in range of the gun once: {:?}", nearest.values().map(|d| d.to_f64()).collect::<Vec<_>>());
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
    let deck = s.characters[s.vehicles[0].character as usize].crawler.clone().unwrap().deck;
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
        let c = self.def.crawler.clone().unwrap();
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

pub(super) const SOUTH: Fx = Fx::ZERO;
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

/// Stopped at the pit's edge with its front feet held back, it still turns
/// in place (CR.6), either way: it backs off slowly as it turns, its feet
/// finding footholds this side of the pit nearer the body, then walks away
/// along the edge, never a foot down the pit.
#[test]
fn the_crawler_turns_in_place_beside_the_pit() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    for (towards, name) in [(SOUTH, "south"), (Fx::PI, "north")] {
        let mut walk = Walk::new(&sim, (-4.0, 0.0), WEST);
        walk.drive(terrain, heading_vector(WEST), false, 30.0);
        let stopped = walk.v.position;
        walk.drive(terrain, heading_vector(WEST), false, 2.0);
        assert_eq!(walk.v.position, stopped, "stopped at the pit: {:?}", walk.at(terrain));
        let mut turned = None;
        for tick in 0..seconds(20.0) {
            walk.step(terrain, Control { direction: heading_vector(towards), boost: false }, &[]);
            assert!(walk.legs().feet.iter().all(|f| f.floor == Fx::ZERO && f.swing.as_ref().is_none_or(|s| s.floor == Fx::ZERO)), "a foot down the pit");
            if angle_between(walk.v.heading, towards).abs() < Fx::from_ratio(1, 100) {
                turned = Some(tick);
                break;
            }
        }
        let took = turned.unwrap_or_else(|| panic!("turned {name}: heading {}°, at {:?}", walk.v.heading.to_f64().to_degrees(), walk.at(terrain)));
        assert!(took < seconds(12.0), "turned {name} in {} s", took as f64 / 60.0);
        let back = (walk.v.position - stopped).length();
        assert!(back < Fx::from_int(3), "it backed off only a little: {} m", back.to_f64());
        // Turned, it walks away along the edge.
        let before = walk.v.position;
        walk.drive(terrain, heading_vector(towards), false, 6.0);
        assert!((walk.v.position - before).length() > Fx::from_int(3), "walked {name} along the pit: {:?}", walk.at(terrain));
    }
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
    let length = walk.def.crawler.clone().unwrap().length;
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
    let c = walk.def.crawler.clone().unwrap();
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
pub(super) fn copy(world: &World) -> World {
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

/// The soak (CR.6): a pilot, a gunner and two riders aboard (TU.1 of the
/// turret), a fifth elder on the ground, ghouls coming every few seconds. The pilot walks, trots, turns
/// in place, backs and steers at random for `minutes`; every so often a
/// ghoul is put where a foot is about to land, or the elder on the ground
/// steps there. Every tick the settlement validates, its feet in reach and
/// on floors, the gunner and riders stay in their places, and every kill
/// is a stomp.
fn soak(minutes: f64, seed: u64) {
    let (mut sim, agents) = crawler_lab_with_guests();
    {
        let wave = &mut sim.world_mut().settlement_mut().waves[0];
        wave.count = 10_000;
        wave.every = seconds(4.0);
    }
    board(&mut sim, &agents, &[0, 1, 2, 3]);
    let mut rng = seed;
    let mut next = |n: u64| {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng >> 33) % n
    };
    let mut until = 0;
    let (mut put, mut shoved, mut stomp_kills, mut lifts) = (0u64, 0u64, 0u64, 0usize);
    let mut gaits = [0u64; 2];
    let mut backing = 0u64;
    let mut next_put = 0;
    let ticks = seconds(minutes * 60.0);
    for tick in 0..ticks {
        let (heading, position) = {
            let v = &sim.world().settlement().unwrap().vehicles[0];
            (v.heading, v.position)
        };
        let mut turn = Vec::new();
        if tick >= until {
            until = tick + seconds(1.0 + next(5) as f64);
            // Toward the lab's middle now and then, so it doesn't sit in a corner.
            let home = (sim.world().terrain().center - position).normalize_or_zero();
            let (direction, run) = match next(7) {
                0 => (heading_vector(heading), false),
                1 => (heading_vector(heading), true),
                2 => (heading_vector(heading + Fx::FRAC_PI_2), false),
                3 => (heading_vector(heading - Fx::FRAC_PI_2), next(2) == 0),
                4 => (heading_vector(heading + Fx::PI), false),
                5 => (home, next(2) == 0),
                _ => (FxVec2::new(Fx::from_ratio(next(201) as i64 - 100, 100), Fx::from_ratio(next(201) as i64 - 100, 100)).normalize_or_zero(), next(3) == 0),
            };
            turn.push(Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Walk { direction, run } } });
        }
        // Something under a foot a tick before it lands.
        if tick >= next_put {
            let s = sim.world().settlement().unwrap();
            let v = &s.vehicles[0];
            let legs = v.legs.as_ref().unwrap();
            let swing = legs.gait.def(s.characters[v.character as usize].crawler.as_ref().unwrap()).swing;
            let landing = legs.feet.iter().find_map(|f| f.swing.as_ref().filter(|w| w.progress + 1 == swing).map(|w| (w.to, w.floor)));
            if let Some((to, floor)) = landing {
                let s = sim.world_mut().settlement_mut();
                if put % 3 != 2 && !s.threats.is_empty() {
                    let t = &mut s.threats[0];
                    (t.position, t.height, t.windup) = (to, floor, None);
                    put += 1;
                    next_put = tick + seconds(1.5);
                } else if put % 3 == 2 {
                    let a = &mut s.agents[agents[4] as usize];
                    a.body.position = to + FxVec2::new(Fx::from_ratio(1, 10), Fx::ZERO);
                    a.body.velocity = FxVec2::ZERO;
                    a.avatar_mut().unwrap().height = floor;
                    put += 1;
                    next_put = tick + seconds(1.5);
                }
            }
        }
        let before = sim.world().settlement().unwrap().vehicles[0].legs.as_ref().unwrap().up().count();
        sim.step(&turn);
        let s = sim.world().settlement().unwrap();
        s.validate().unwrap_or_else(|e| panic!("tick {tick}: {e}"));
        s.validate_footholds(sim.world().terrain()).unwrap_or_else(|e| panic!("tick {tick}: {e}"));
        let v = &s.vehicles[0];
        let legs = v.legs.as_ref().unwrap();
        lifts += legs.up().count().saturating_sub(before);
        gaits[(legs.gait == Gait::Trot) as usize] += 1;
        backing += legs.backing as u64;
        assert_eq!(s.seat(agents[0] as usize), Some(Seat::Vehicle(0)), "tick {tick}: the pilot aboard");
        let def = &s.characters[v.character as usize];
        assert_eq!(s.agents[agents[1] as usize].body.position, v.seat_at(def, Some(crawler::GUNNER)).0, "tick {tick}: the gunner in their place");
        for (place, &agent) in agents[2..4].iter().enumerate() {
            assert_eq!(s.agents[agent as usize].body.position, v.seat_at(def, Some(crawler::RIDERS[place])).0, "tick {tick}: rider {place} in their place");
        }
        for e in &s.events {
            match *e {
                super::Event::Hit { impact, killed, .. } => {
                    assert_eq!(impact, super::Impact::Stomp, "tick {tick}: only feet hurt anyone");
                    stomp_kills += killed as u64;
                }
                super::Event::Footfall { shoved: n, .. } => shoved += n as u64,
                _ => {}
            }
        }
        assert_eq!(s.combat.kills, stomp_kills, "tick {tick}: every kill a stomp");
    }
    assert!(lifts as f64 > 60.0 * minutes, "it walked: {lifts} lifts");
    assert!(gaits[1] > ticks / 20 && backing > ticks / 50, "it trotted ({} ticks) and backed ({backing} ticks)", gaits[1]);
    let ghouls = put - put / 3;
    eprintln!("soak {minutes} min: {lifts} lifts, {} ticks trotting, {backing} backing, {put} puts ({ghouls} ghouls), {stomp_kills} stomp kills, {shoved} shoved", gaits[1]);
    assert!(stomp_kills * 10 >= ghouls * 9 && stomp_kills > 0, "{stomp_kills} kills of {ghouls} ghouls put under a foot ({put} puts)");
    assert!(shoved > 0, "the elder on the ground was shoved");
}

/// Four minutes of the soak, every check every tick.
#[test]
fn four_minutes_driving_with_riders_among_ghouls_validates() {
    soak(4.0, 0x50a6);
}

/// The long soak: half an hour. Run by hand (`--test_arg=--ignored`).
#[test]
#[ignore]
fn half_an_hour_driving_with_riders_among_ghouls_validates() {
    soak(30.0, 0x50a7);
}

// Riders (CR.4 of the crawler, C9) and the gunner (TU.1 of the turret,
// T2, T3): a pilot in the cab, a gunner and up to two riders on the deck.

/// The lab with four guests joined beside the host: every elder stands
/// west of the crawler, in reach of its box. Returns the five avatars,
/// the host first.
pub(super) fn crawler_lab_with_guests() -> (Simulation, Vec<u16>) {
    let mut sim = crawler_lab();
    let guests: Vec<_> = (1..=4).map(PlayerId).collect();
    sim.step(&guests.iter().map(|&player| Command { player, kind: CommandKind::Join }).collect::<Vec<_>>());
    let s = sim.world().settlement().unwrap();
    let mut agents = vec![s.player().unwrap()];
    agents.extend(guests.iter().map(|&g| s.avatar_of(g).unwrap()));
    for (i, &agent) in agents.iter().enumerate() {
        let p = meters(&sim, 0.4, i as f64 * 0.9 - 1.8);
        let a = &mut sim.world_mut().settlement_mut().agents[agent as usize];
        a.body.position = p;
        a.body.velocity = FxVec2::ZERO;
    }
    (sim, agents)
}

pub(super) fn owner(i: usize) -> PlayerId {
    PlayerId(i as u32)
}

pub(super) fn board(sim: &mut Simulation, agents: &[u16], who: &[usize]) {
    sim.step(&who.iter().map(|&i| Command { player: owner(i), kind: CommandKind::Board { agent: agents[i] } }).collect::<Vec<_>>());
}

fn seat_of(sim: &Simulation, agent: u16) -> Option<Seat> {
    sim.world().settlement().unwrap().seat(agent as usize)
}

/// Five elders press G on one tick: the first in command order takes the
/// cab, the second the gun (TU.1 of the turret, T3), the next two the
/// riders' places nearest them, and the fifth stays on the ground. The
/// gunner and the riders ride in their places on the deck as it walks, and
/// their sticks steer nothing.
#[test]
fn a_pilot_a_gunner_and_two_riders_board_and_a_fifth_cannot() {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0, 1, 2, 3, 4]);
    assert_eq!(seat_of(&sim, agents[0]), Some(Seat::Vehicle(0)), "the first takes the cab");
    assert_eq!(seat_of(&sim, agents[1]), Some(Seat::Gunner { vehicle: 0 }), "the second the gun");
    assert_eq!(sim.world().settlement().unwrap().gunner_of(0), Some(agents[1] as usize));
    let places: Vec<_> = agents[2..4].iter().map(|&a| seat_of(&sim, a)).collect();
    assert_eq!(places, (0..2).map(|place| Some(Seat::Rider { vehicle: 0, place })).collect::<Vec<_>>(), "riders on the deck, nearest first");
    assert_eq!(seat_of(&sim, agents[4]), None, "no fifth seat");
    board(&mut sim, &agents, &[4]);
    assert_eq!(seat_of(&sim, agents[4]), None, "still none");

    let s = sim.world().settlement().unwrap();
    let def = s.characters[s.vehicles[0].character as usize].clone();
    let walk = |direction: FxVec2| Intent::Walk { direction, run: false };
    // The pilot walks south; the gunner and riders push their sticks every
    // way.
    let mut turn = vec![Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: walk(FxVec2::new(Fx::ZERO, Fx::ONE)) } }];
    for i in 1..4 {
        turn.push(Command { player: owner(i), kind: CommandKind::MoveAvatar { agent: agents[i], intent: walk(FxVec2::new(Fx::from_int(i as i32 - 2), -Fx::ONE).normalize_or_zero()) } });
    }
    sim.step(&turn);
    let parked = sim.world().settlement().unwrap().vehicles[0].position;
    for _ in 0..seconds(5.0) {
        sim.step(&[]);
        let s = sim.world().settlement().unwrap();
        s.validate().unwrap();
        let v = &s.vehicles[0];
        assert_eq!(v.control.direction, FxVec2::new(Fx::ZERO, Fx::ONE), "only the pilot steers");
        for (i, &agent) in agents[1..4].iter().enumerate() {
            let a = &s.agents[agent as usize];
            let (spot, height) = v.seat_at(&def, crawler::deck_spot(a.avatar().unwrap().seat.unwrap()));
            assert_eq!((a.body.position, a.avatar().unwrap().height, a.body.heading), (spot, height, v.heading), "elder {} in their place", i + 1);
        }
        let gunner = &s.agents[agents[1] as usize];
        assert_eq!(gunner.body.position, Pose::of(v).at(crawler::GUNNER), "the gunner behind the crates");
    }
    let moved = sim.world().settlement().unwrap().vehicles[0].position - parked;
    assert!(moved.y > Fx::from_int(3) && moved.x.abs() < Fx::ONE, "it went south, not where the riders pushed: {moved:?}");

    // With the pilot gone, the gunner's and riders' sticks don't move it.
    board(&mut sim, &agents, &[0]);
    assert_eq!(seat_of(&sim, agents[0]), None, "the pilot climbed down");
    for _ in 0..seconds(3.0) {
        sim.step(&[]);
    }
    let stopped = sim.world().settlement().unwrap().vehicles[0].clone();
    sim.step(&(1..4).map(|i| Command { player: owner(i), kind: CommandKind::MoveAvatar { agent: agents[i], intent: walk(FxVec2::new(Fx::ONE, Fx::ZERO)) } }).collect::<Vec<_>>());
    for _ in 0..seconds(3.0) {
        sim.step(&[]);
    }
    let after = &sim.world().settlement().unwrap().vehicles[0];
    assert_eq!((after.position, after.heading, after.control), (stopped.position, stopped.heading, Control::default()), "riders and the gunner can't steer");
    assert_eq!(seat_of(&sim, agents[1]), Some(Seat::Gunner { vehicle: 0 }), "the gunner doesn't move to the cab");
    assert!(agents[2..4].iter().all(|&a| seat_of(&sim, a).and_then(Seat::place).is_some()), "the riders still ride");
}

/// The gun's seat comes after the cab's and before the riders', whichever
/// side the elder boards from; freed, the next to board takes it again.
/// Only a crawler with a gun has one: without it, two riders and no third
/// place (the gunner's spot is no rider's), and `validate` refuses a
/// gunner there, two at one gun and a rider in the old third place.
#[test]
fn the_gun_is_taken_after_the_cab_and_again_when_freed() {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0]);
    board(&mut sim, &agents, &[3]);
    assert_eq!(seat_of(&sim, agents[3]), Some(Seat::Gunner { vehicle: 0 }), "the second aboard takes the gun");
    board(&mut sim, &agents, &[2]);
    assert_eq!(seat_of(&sim, agents[2]), Some(Seat::Rider { vehicle: 0, place: 0 }), "then a rider");
    // The gunner climbs down; the next to board takes the gun, not the
    // free rider's place nearer them.
    board(&mut sim, &agents, &[3]);
    assert_eq!(seat_of(&sim, agents[3]), None);
    board(&mut sim, &agents, &[1]);
    assert_eq!(seat_of(&sim, agents[1]), Some(Seat::Gunner { vehicle: 0 }), "the gun again");
    sim.world().settlement().unwrap().validate().unwrap();
    {
        let s = sim.world_mut().settlement_mut();
        s.agents[agents[4] as usize].avatar_mut().unwrap().seat = Some(Seat::Gunner { vehicle: 0 });
        assert!(s.validate().unwrap_err().contains("shares a seat"), "two at the gun");
        s.agents[agents[4] as usize].avatar_mut().unwrap().seat = Some(Seat::Rider { vehicle: 0, place: 2 });
        assert!(s.validate().unwrap_err().contains("no place 2"), "no third rider's place");
    }

    // A crawler without a gun: a pilot and two riders.
    let (mut sim, agents) = crawler_lab_with_guests();
    let ix = sim.world().settlement().unwrap().vehicles[0].character as usize;
    sim.world_mut().settlement_mut().characters[ix].crawler.as_mut().unwrap().gun = None;
    board(&mut sim, &agents, &[0, 1, 2, 3]);
    let seats: Vec<_> = agents[..4].iter().map(|&a| seat_of(&sim, a)).collect();
    assert_eq!(seats, [Some(Seat::Vehicle(0)), Some(Seat::Rider { vehicle: 0, place: 0 }), Some(Seat::Rider { vehicle: 0, place: 1 }), None], "no gun, no gunner");
    let s = sim.world_mut().settlement_mut();
    s.agents[agents[3] as usize].avatar_mut().unwrap().seat = Some(Seat::Gunner { vehicle: 0 });
    assert!(s.validate().unwrap_err().contains("no gun"));
}

/// Everyone climbs down beside the box: outside it, on the ground, clear
/// of its feet, each rider on their own side first and the gunner behind
/// it, as the back rider did before the turret.
#[test]
fn riders_climb_down_beside_it_on_the_ground() {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0, 1, 2, 3]);
    for _ in 0..seconds(1.0) {
        sim.step(&[]);
    }
    board(&mut sim, &agents, &[0, 1, 2, 3]);
    let s = sim.world().settlement().unwrap();
    let v = &s.vehicles[0];
    let c = s.characters[v.character as usize].crawler.clone().unwrap();
    let legs = v.legs.as_ref().unwrap();
    for (i, &agent) in agents[..4].iter().enumerate() {
        let a = &s.agents[agent as usize];
        assert_eq!(s.seat(agent as usize), None, "elder {i} climbed down");
        let r = a.radius();
        assert!(crawler::box_distance_squared(Pose::of(v), &c, a.body.position) > r * r, "elder {i} not under the body");
        assert_eq!(a.avatar().unwrap().height, Fx::ZERO, "elder {i} on the ground");
        for f in &legs.feet {
            assert!((f.spot - a.body.position).length() > r + Fx::from_ratio(1, 2), "elder {i} clear of the feet");
        }
        // The gunner (the second elder) climbs down behind, the
        // right-hand rider (place 1, the fourth) on the right.
        let local = Pose::of(v).local(a.body.position);
        assert_eq!(local.across > Fx::ZERO, i == 3, "elder {i} on their own side: {}", local.across.to_f64());
        assert_eq!(local.along < -c.length / Fx::from_int(2), i == 1, "elder {i} behind it or not: {}", local.along.to_f64());
    }
    // Climbing down together, each lands on their own spot.
    for (i, &a) in agents[..4].iter().enumerate() {
        for &b in &agents[i + 1..4] {
            let (a, b) = (&s.agents[a as usize], &s.agents[b as usize]);
            assert!((a.body.position - b.body.position).length() > a.radius() + b.radius(), "two elders on one spot");
        }
    }
}

/// Parked with its left side over the trench, the crawler lets its pilot
/// and its left-hand rider down on its right, on the ground, never into
/// the trench; and a spot a foot is about to land on is passed over.
#[test]
fn climbing_down_never_goes_into_a_pit_or_under_a_foot() {
    let mut sim = crawler_lab();
    let (ix, def) = crawler_def();
    // The trench is the tiles x -12 and -11 (-12.5 to -10.5 m): the left
    // spots to climb down to are over it, the left feet down in it (at
    // -9.5 m) or on its far side (at -10 m).
    let c = def.crawler.clone().unwrap();
    let radius = Fx::from_ratio(7, 20);
    for (x, floor) in [(-9.5, -2), (-10.0, 0)] {
        let v = VehicleState::new(ix, &def, meters(&sim, x, 0.0), Fx::ZERO, sim.world().terrain()).expect("parks astride the trench's edge");
        assert_eq!(v.legs.as_ref().unwrap().feet[0].floor, Fx::from_int(floor), "the front left foot at {x}");
        for place in [None, Some(crawler::RIDERS[0])] {
            let (spot, ground) = crawler::exit_spot(&v, &c, place, radius, sim.world().terrain()).expect("a spot on the right");
            assert!(Pose::of(&v).local(spot).across > Fx::ZERO, "{place:?} at {x}: on the right, not in the trench: {:?} at {} m", spot - sim.world().terrain().center, ground.to_f64());
            assert_eq!(ground, Fx::ZERO, "on the ground");
        }
    }
    let v = VehicleState::new(ix, &def, meters(&sim, -10.0, 0.0), Fx::ZERO, sim.world().terrain()).unwrap();
    // In the sim too: the pilot boards there and climbs down.
    sim.world_mut().settlement_mut().vehicles[0] = v.clone();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    let beside = meters(&sim, -7.8, 0.0);
    sim.world_mut().settlement_mut().agents[agent as usize].body.position = beside;
    command(&mut sim, CommandKind::Board { agent });
    assert_eq!(sim.world().settlement().unwrap().driving(agent as usize), Some(0));
    command(&mut sim, CommandKind::Board { agent });
    assert_eq!(sim.world().settlement().unwrap().driving(agent as usize), None);
    assert_eq!(me(&sim).avatar().unwrap().height, Fx::ZERO, "out of the trench");
    assert!((me(&sim).body.position - meters(&sim, -11.5, 0.0)).length() > Fx::from_int(3));

    // On flat ground, a foot swinging to the first spot keeps the elder off it.
    let v = VehicleState::new(ix, &def, meters(&sim, -17.0, 0.0), Fx::ZERO, sim.world().terrain()).unwrap();
    let terrain = sim.world().terrain();
    let (first, _) = crawler::exit_spot(&v, &c, None, radius, terrain).unwrap();
    let mut landing = v.clone();
    landing.legs.as_mut().unwrap().feet[2].swing = Some(crawler::Swing { to: first, floor: Fx::ZERO, progress: 3 });
    let (spot, _) = crawler::exit_spot(&landing, &c, None, radius, terrain).unwrap();
    assert!((spot - first).length() > radius + Fx::from_ratio(1, 2), "not under the landing foot");
    assert!(Pose::of(&v).local(spot).across < Fx::ZERO, "still on the pilot's side");
}

/// A rider's client predicts them riding, tick for tick, while the pilot
/// holds their stick: the replay runs the crawler on its confirmed
/// controls. A gunner's (TU.1 of the turret) too. Their places round-trip
/// a save, which continues alike.
#[test]
fn riding_is_predicted_and_saved() {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0, 1, 2]);
    let (gunner, rider) = (agents[1], agents[2]);
    assert_eq!(seat_of(&sim, gunner), Some(Seat::Gunner { vehicle: 0 }));
    assert_eq!(seat_of(&sim, rider), Some(Seat::Rider { vehicle: 0, place: 0 }));
    let walk = |direction: FxVec2| CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Walk { direction, run: false } };
    sim.step(&[Command { player: owner(0), kind: walk(FxVec2::new(Fx::from_ratio(3, 10), Fx::ONE).normalize_or_zero()) }]);
    let mut compared = 0;
    for _ in 0..5 {
        let confirmed = copy(sim.world());
        let replay = |agent: u16| confirmed.settlement().unwrap().replay_avatar(agent, confirmed.tick(), confirmed.terrain()).unwrap();
        let mut replays = [(gunner, 1), (rider, 2)].map(|(agent, i)| (agent, i, replay(agent)));
        let mut ahead = Simulation::from_world(copy(&confirmed), 1);
        for t in 0..60u64 {
            // Their sticks, which steer nothing, here or there.
            let push = |agent: u16| -> Vec<CommandKind> {
                if t % 20 == 0 {
                    vec![CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true } }]
                } else {
                    Vec::new()
                }
            };
            let mut turn = Vec::new();
            for (agent, i, replay) in &mut replays {
                let mine = push(*agent);
                replay.step_with(mine.iter());
                turn.extend(mine.into_iter().map(|kind| Command { player: owner(*i), kind }));
            }
            ahead.step(&turn);
            let real = ahead.world().settlement().unwrap();
            for (agent, _, replay) in &replays {
                let (ix, predicted) = replay.vehicle().expect("still aboard");
                assert_eq!(predicted, &real.vehicles[ix as usize], "the crawler, for {agent}");
                assert_eq!(replay.state().body.position, real.agents[*agent as usize].body.position, "{agent} in their place");
                assert_eq!(replay.state().avatar().unwrap().height, real.agents[*agent as usize].avatar().unwrap().height);
            }
            compared += 1;
        }
        for _ in 0..45 {
            sim.step(&[]);
        }
    }
    assert_eq!(compared, 300);

    // Saved with a gunner and a rider aboard, it continues the same.
    let world = sim.into_world();
    let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
    let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), world.rules().clone()).unwrap();
    assert_eq!(loaded.settlement().unwrap().seat(rider as usize), Some(Seat::Rider { vehicle: 0, place: 0 }));
    assert_eq!(loaded.settlement().unwrap().seat(gunner as usize), Some(Seat::Gunner { vehicle: 0 }));
    let (mut a, mut b) = (Simulation::from_world(world, 1), Simulation::from_world(loaded, 1));
    for _ in 0..seconds(3.0) {
        assert_eq!(a.step(&[]).total, b.step(&[]).total);
    }
}

/// Turned east over the moat's edge and then north-north-east, a foot came
/// to rest 2 m from its resting spot with the moat behind it (CR.6's demo):
/// footholds round the resting spot, not only past it, let it step and walk
/// on. Before, it stood there for good, whichever way it was steered.
#[test]
fn the_crawler_walks_on_after_turning_by_the_moat() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let mut walk = Walk::new(&sim, (2.5, 0.0), SOUTH);
    let v = |x: f64, z: f64| FxVec2::new(Fx::quantize(x as f32), Fx::quantize(z as f32));
    walk.drive(terrain, v(0.0, 1.0), false, 10.7);
    walk.drive(terrain, v(1.0, 0.0), false, 10.0);
    walk.drive(terrain, v(0.49, -0.87), false, 10.0);
    for direction in [v(-1.0, 0.0), v(0.0, -1.0)] {
        let before = walk.v.position;
        walk.drive(terrain, direction, false, 10.0);
        if (walk.v.position - before).length() <= Fx::from_int(3) {
            let c = walk.def.crawler.clone().unwrap();
            let pose = Pose::of(&walk.v);
            for (i, f) in walk.legs().feet.iter().enumerate() {
                let r = pose.point(c.stance, i) - terrain.center;
                eprintln!("DEBUG foot {i}: spot {:?} floor {} swing {:?} rest ({:.2},{:.2}) off {:.2}", (f.spot - terrain.center), f.floor.to_f64(), f.swing.as_ref().map(|s| (s.to - terrain.center, s.progress)), r.x.to_f64(), r.y.to_f64(), (f.spot - pose.point(c.stance, i)).length().to_f64());
            }
            eprintln!("DEBUG dir {direction:?} heading {:.2} clock {} gait {:?} backing {} speed {} height {}", walk.v.heading.to_f64(), walk.legs().clock, walk.legs().gait, walk.legs().backing, walk.v.speed.to_f64(), walk.v.height.to_f64());
            let last: Vec<_> = walk.lifts.iter().rev().take(8).collect();
            eprintln!("DEBUG last lifts {last:?} ticks {}", walk.ticks);
        }
        assert!((walk.v.position - before).length() > Fx::from_int(3), "walked on: {:?}", walk.at(terrain));
    }
}

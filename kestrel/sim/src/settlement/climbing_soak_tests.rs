//! The Climbing Gym soak (CL.5 of the climbing design, `kestrel/CLIMBING.md`):
//! the elder walks, grabs, climbs, lunges and lets go at random round the
//! mountain and the pillars for minutes, every tick validated and checked
//! against the climb's invariants: on a face the body hangs its gap off a
//! face it holds, clear of rock; off one it stands free or falls clear;
//! the wheel stays in [0, 1] and never refills on a face; an exhausted or
//! released elder never grabs; a death from a fall comes back at home; and
//! nothing drifts off the map or out of the volume's heights.

use content::TICKS_PER_SECOND;
use fx::{Fx, FxVec2};

use super::avatar::{Intent, JumpPhase};
use super::climb::{body_clear, hold, plane, ClimbPhase, Climbing, Face, GAP};
use super::climbing_gym_tests::gym;
use super::soak_tests::{standing_free, Dice};
use super::{AgentIx, Event};
use crate::terrain::Terrain;
use crate::{Command, CommandKind, PlayerId, Simulation};

const HZ: u64 = TICKS_PER_SECOND as u64;

/// What the soak saw.
#[derive(Debug, Default)]
pub(super) struct Climbed {
    /// Grabs by pushing into a face, and jumps at one from the ground.
    pub grabs: u32,
    pub leaps: u32,
    pub pull_overs: u32,
    pub lunges: u32,
    /// Let go by G, and by an empty wheel.
    pub let_gos: u32,
    pub exhausted: u32,
    /// Corners: round an outside one (the same tile, another face) and
    /// into an inside one (another tile and face).
    pub wraps: u32,
    pub turns: u32,
    pub last_corner: u64,
    pub falls_hurt: u32,
    pub deaths: u32,
    pub walks: u32,
    /// The ticks spent on a face, the longest single climb, and the highest
    /// the feet went.
    pub on_face: u64,
    pub longest_climb: u64,
    pub highest: Fx,
    /// Faces held, by side.
    pub faces: [u32; 4],
}

/// Spots by the gym's faces (scene meters) and the way into the face from
/// there, 0.4 m off it (in a jump's reach): the pillars, the west face's two pitches, the north face, the
/// shoulder's south and west faces, the summit's south face, the inside
/// corner, the chimney's three walls, the wing's east face (beside and
/// under the overhang), the low step's faces.
const SPOTS: [(f64, f64, i32, i32); 18] = [
    (24.5, 21.9, 0, -1),
    (30.5, 21.9, 0, -1),
    (36.5, 21.9, 0, -1),
    (37.9, 20.5, -1, 0),
    (-14.9, -10.0, 1, 0),
    (-14.9, -16.0, 1, 0),
    (0.0, -20.9, 0, 1),
    (0.0, 8.9, 0, -1),
    (7.0, 8.9, 0, -1),
    (-4.9, 4.0, 1, 0),
    (-7.0, 0.9, 0, -1),
    (-4.9, 0.9, 1, 0),
    (4.1, 6.0, 1, 0),
    (2.9, 6.0, -1, 0),
    (3.5, 2.9, 0, -1),
    (22.9, 0.0, -1, 0),
    (22.9, -10.0, -1, 0),
    (16.0, 14.9, 0, -1),
];

fn axis(x: i32, z: i32) -> FxVec2 {
    FxVec2::new(Fx::from_int(x), Fx::from_int(z))
}

/// One of sixteen headings, about 22.5 degrees apart.
fn heading(dice: &mut Dice) -> FxVec2 {
    let i = dice.below(16);
    let (x, y) = [(4, 0), (4, 2), (3, 3), (2, 4)][i as usize % 4];
    let (x, y) = match i / 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    };
    axis(x, y).normalize_or_zero()
}

fn face_index(face: Face) -> usize {
    match face {
        Face::East => 0,
        Face::West => 1,
        Face::South => 2,
        Face::North => 3,
    }
}

/// Plays the gym for `ticks` from `seed`. Returns every tick's hash and
/// what it saw.
pub(super) fn gym_soak(seed: u64, ticks: u64) -> (Vec<u64>, Climbed) {
    let mut sim = gym();
    let mut dice = Dice(seed);
    let mut seen = Climbed::default();
    let me = sim.world().settlement().unwrap().player().unwrap();
    let home = sim.world().settlement().unwrap().agents[me as usize].body.home;
    let center = sim.world().terrain().center;
    let mut hashes = Vec::with_capacity(ticks as usize);
    // The next choice's tick; a walk under way (to push into its spot's
    // face on arrival); a respawn awaited; the climb under way's start.
    let (mut next_at, mut walking, mut respawn, mut climb_start) = (0u64, None::<FxVec2>, None::<u64>, None::<u64>);
    let walk = |intent| Command { player: PlayerId(0), kind: CommandKind::MoveAvatar { agent: me, intent } };
    let act = |kind| Command { player: PlayerId(0), kind };
    while sim.world().tick() < ticks {
        let tick = sim.world().tick();
        let s = sim.world().settlement().unwrap();
        let a = &s.agents[me as usize];
        let avatar = a.avatar().unwrap();
        let before = avatar.climbing;
        let (alive, airborne) = (a.body.alive(), avatar.flight.airborne || matches!(avatar.jump, JumpPhase::Airborne { .. }));
        let still = a.body.route.is_empty() && a.body.velocity == FxVec2::ZERO;
        let mut turn = Vec::new();
        let mut let_go = false;
        if alive && (tick >= next_at || walking.is_some() && still && tick >= next_at.saturating_sub(18 * HZ)) {
            if let (Some(into), true) = (walking.take(), still) {
                // Arrived at a spot: push into its face a while, or jump at
                // it (a grab 1.5 m up).
                turn.push(walk(Intent::Walk { direction: into, run: false }));
                if dice.below(4) == 0 {
                    turn.push(act(CommandKind::Jump { agent: me }));
                }
                next_at = tick + HZ / 2 + dice.below(3 * HZ);
            } else if let Some(climb) = before.climb {
                let (n, t) = (climb.face.normal(), climb.face.tangent());
                let side = if dice.below(2) == 0 { t } else { -t };
                // Mostly up, and then for longer (a pitch takes 13 s).
                let mut hold_for = HZ / 3 + dice.below(2 * HZ);
                let stick = match dice.below(100) {
                    0..=44 => {
                        hold_for = 2 * HZ + dice.below(8 * HZ);
                        -n
                    }
                    45..=59 => side,
                    60..=69 => (side - n).normalize_or_zero(),
                    70..=79 => n,
                    80..=84 => FxVec2::ZERO,
                    85..=95 => {
                        turn.push(act(CommandKind::Jump { agent: me }));
                        [-n, side, (side - n).normalize_or_zero(), n][dice.below(4) as usize]
                    }
                    _ => {
                        turn.push(act(CommandKind::Board { agent: me }));
                        let_go = true;
                        -n
                    }
                };
                turn.insert(0, walk(if stick == FxVec2::ZERO { Intent::Stop } else { Intent::Walk { direction: stick, run: false } }));
                next_at = tick + hold_for;
            } else if airborne {
                // Falling: now and then reach for a face.
                if dice.below(2) == 0 {
                    let (x, z) = [(1, 0), (-1, 0), (0, 1), (0, -1)][dice.below(4) as usize];
                    turn.push(walk(Intent::Walk { direction: axis(x, z), run: false }));
                }
                next_at = tick + HZ / 4;
            } else {
                match dice.below(100) {
                    // To a spot by a face, by a route.
                    0..=34 => {
                        let (x, z, dx, dz) = SPOTS[dice.below(SPOTS.len() as u64) as usize];
                        let m = |v: f64| Fx::from_ratio((v * 1000.0).round() as i64, 1000);
                        turn.push(walk(Intent::GoTo { target: center + FxVec2::new(m(x), m(z)), run: dice.below(2) == 0 }));
                        seen.walks += 1;
                        walking = Some(axis(dx, dz));
                        next_at = tick + 20 * HZ;
                    }
                    // Push a random way, often straight along an axis.
                    35..=69 => {
                        let direction = if dice.below(2) == 0 {
                            let (x, z) = [(1, 0), (-1, 0), (0, 1), (0, -1)][dice.below(4) as usize];
                            axis(x, z)
                        } else {
                            heading(&mut dice)
                        };
                        turn.push(walk(Intent::Walk { direction, run: dice.below(4) == 0 }));
                        next_at = tick + HZ / 3 + dice.below(2 * HZ);
                    }
                    // Jump, the stick a random way (at a face, a grab up it).
                    70..=84 => {
                        let (x, z) = [(1, 0), (-1, 0), (0, 1), (0, -1)][dice.below(4) as usize];
                        turn.push(walk(Intent::Walk { direction: axis(x, z), run: false }));
                        turn.push(act(CommandKind::Jump { agent: me }));
                        next_at = tick + HZ / 2 + dice.below(HZ);
                    }
                    // Stand still a moment, or let go of nothing.
                    85..=94 => {
                        turn.push(walk(Intent::Stop));
                        next_at = tick + HZ / 2 + dice.below(HZ);
                    }
                    _ => {
                        turn.push(act(CommandKind::Board { agent: me }));
                        next_at = tick + HZ / 4;
                    }
                }
            }
        }
        let hash = sim.step(&turn).total;
        hashes.push(hash);
        check(&sim, me, home, before, let_go, &mut seen, &mut respawn, &mut climb_start);
    }
    (hashes, seen)
}

/// The checks after each tick, and the counts.
#[allow(clippy::too_many_arguments)]
fn check(sim: &Simulation, me: AgentIx, home: FxVec2, before: Climbing, let_go: bool, seen: &mut Climbed, respawn: &mut Option<u64>, climb_start: &mut Option<u64>) {
    let (world, tick) = (sim.world(), sim.world().tick());
    let terrain = world.terrain();
    let volume = terrain.volume.as_ref().unwrap();
    let s = world.settlement().unwrap();
    s.validate().unwrap_or_else(|e| panic!("tick {tick}: {e}"));
    let a = &s.agents[me as usize];
    let avatar = a.avatar().unwrap();
    let now = avatar.climbing;
    let (p, feet, r) = (a.body.position, avatar.height, a.radius());
    let at = p - terrain.center;
    // No drift: on the map, within the volume's heights, the wheel in range.
    assert!(terrain.inside(p), "off the map at tick {tick}: {at:?}");
    assert!(feet >= Fx::from_int(-16) && feet <= Fx::from_int(64), "feet {feet:?} at tick {tick}");
    assert!(now.spent >= Fx::ZERO && now.spent <= Fx::ONE, "the wheel {:?} at tick {tick}", now.spent);
    assert!(a.body.health <= a.body.max_health, "health {} at tick {tick}", a.body.health);
    for event in &s.events {
        if let Event::Fell { damage, killed, .. } = *event {
            assert!(damage > 0);
            seen.falls_hurt += 1;
            if killed {
                seen.deaths += 1;
                *respawn = Some(tick + 3 * HZ);
            }
        }
    }
    if let Some(by) = *respawn {
        if a.body.alive() {
            assert_eq!((a.body.position, a.body.health), (home, a.body.max_health), "back at home, whole, at tick {tick}");
            *respawn = None;
        } else {
            assert!(tick < by, "no respawn by tick {tick}");
        }
    }
    if !a.body.alive() {
        return;
    }
    seen.highest = seen.highest.max(feet);
    match (before.climb, now.climb) {
        (None, Some(c)) => {
            // A grab: never while exhausted, never in the air after a let-go.
            assert!(!before.exhausted || now.spent == Fx::ZERO, "grabbed exhausted at tick {tick}");
            assert!(!now.exhausted, "grabbed exhausted at tick {tick}");
            if before.released {
                let floor = terrain.footprint_at(p, feet, r);
                assert!(floor.is_some_and(|f| (f - feet).abs() <= Fx::from_ratio(1, 50)), "grabbed in the air after letting go at tick {tick}: {at:?}, feet {feet:?}");
            }
            match c.phase {
                ClimbPhase::Lunge { .. } => seen.leaps += 1,
                _ => seen.grabs += 1,
            }
            seen.faces[face_index(c.face)] += 1;
            *climb_start = Some(tick);
        }
        (Some(b), Some(c)) => {
            if c.face != b.face {
                if c.tile == b.tile {
                    seen.wraps += 1;
                } else {
                    seen.turns += 1;
                    assert!(tick > seen.last_corner + 10, "ping-pong at tick {tick}: {at:?}, feet {feet:?}, {b:?} -> {c:?}, stick {:?}", avatar.intent);
                }
                seen.last_corner = tick;
                seen.faces[face_index(c.face)] += 1;
            }
            if matches!(c.phase, ClimbPhase::Mantle { .. }) && !matches!(b.phase, ClimbPhase::Mantle { .. }) {
                seen.pull_overs += 1;
            }
            if matches!(c.phase, ClimbPhase::Lunge { .. }) && b.phase == ClimbPhase::Hold {
                seen.lunges += 1;
            }
            // No refill on a face.
            assert!(now.spent >= before.spent, "the wheel refilled on a face at tick {tick}");
        }
        (Some(_), None) => {
            if now.exhausted && !let_go {
                seen.exhausted += 1;
            } else if let_go {
                seen.let_gos += 1;
            }
            let span = tick - climb_start.take().unwrap_or(tick);
            seen.longest_climb = seen.longest_climb.max(span);
        }
        (None, None) => {}
    }
    match now.climb {
        Some(c) => {
            seen.on_face += 1;
            // A climb lasts no longer than a full wheel hanging still.
            let span = tick - climb_start.unwrap_or(tick);
            assert!(span <= 92 * HZ, "on a face {span} ticks at tick {tick}");
            assert!(body_clear(volume, p, feet, r), "the climber is in rock at tick {tick}: {at:?}, feet {feet:?}, {c:?}");
            if !matches!(c.phase, ClimbPhase::Mantle { .. }) {
                assert!(hold(volume, c.tile, feet).is_some(), "nothing to hold at tick {tick}: {at:?}, feet {feet:?}, {c:?}");
                let n = c.face.normal();
                let off = p.dot(n) - plane(c.tile, c.face);
                assert_eq!(off, r + GAP, "off the face at tick {tick}: {at:?}, {c:?}");
                let (dx, dy) = c.face.step();
                assert_eq!(Terrain::pit_tile(p), (c.tile.0 + dx, c.tile.1 + dy), "in front of its tile at tick {tick}: {at:?}, {c:?}");
            }
        }
        None if avatar.flight.airborne || matches!(avatar.jump, JumpPhase::Airborne { .. }) => {
            assert!(body_clear(volume, p, feet, r), "falling through rock at tick {tick}: {at:?}, feet {feet:?}");
        }
        None => standing_free(terrain, "the elder", p, feet, r, tick),
    }
}

fn report(what: &str, seen: &Climbed) {
    eprintln!("{what}: {seen:#?}");
}

/// Ten minutes of random climbing in the gym (well under a second to run):
/// every invariant every tick; it climbed every way there is (grabs, leaps,
/// pull-overs, lunges, let-gos, corners both ways), ran out, fell hard and
/// died; and two runs alike tick for tick for the first minute.
#[test]
fn the_climbing_gym_soaks_climbing_at_random() {
    let (hashes, seen) = gym_soak(0xc11b_0005, 600 * HZ);
    report("gym soak, 10 min", &seen);
    assert!(seen.grabs >= 15 && seen.leaps >= 2, "{seen:?}");
    assert!(seen.pull_overs >= 3 && seen.lunges >= 5 && seen.let_gos >= 1 && seen.exhausted >= 5, "{seen:?}");
    assert!(seen.wraps >= 3 && seen.turns >= 1, "{seen:?}");
    assert!(seen.falls_hurt >= 5 && seen.deaths >= 1, "{seen:?}");
    assert!(seen.on_face >= 120 * HZ && seen.highest >= Fx::from_int(15), "two minutes or more on faces, 15 m up: {seen:?}");
    assert_eq!(hashes[..60 * HZ as usize], gym_soak(0xc11b_0005, 60 * HZ).0[..], "deterministic");
}

/// Half an hour each from four seeds, run once by hand:
/// `bazel test --config=remote //kestrel/sim:sim_test --test_arg=gym_soaks_for --test_arg=--ignored --test_output=all`.
#[test]
#[ignore = "two hours of play: run by hand"]
fn the_climbing_gym_soaks_for_half_an_hour() {
    for seed in [0x30_c11b, 0x31_c11b, 0x32_c11b, 0x33_c11b] {
        let (_, seen) = gym_soak(seed, 1800 * HZ);
        report(&format!("gym soak, 30 min, seed {seed:#x}"), &seen);
        assert!(seen.grabs >= 40 && seen.pull_overs >= 5 && seen.deaths >= 1, "{seen:?}");
    }
}

//! Climbing and stamina (CL.2 of the climbing design, `kestrel/CLIMBING.md`
//! K2–K6, K11), in the Climbing Gym (`climbing_gym_tests::gym`, the scene's
//! own map) and in small volumes: grabbing, moving on a face, corners, the
//! top, overhangs, the wheel, the lunge, letting go; determinism, and the
//! replay predicting a climb.

use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::climb::{ClimbPhase, Climbing, Face};
use super::climbing_gym_tests::gym;
use super::tunnel_lab_tests::{feet, me, meters, offset, small, tile};
use crate::{Command, CommandKind, PlayerId, Simulation};

fn agent(sim: &Simulation) -> u16 {
    sim.world().settlement().unwrap().player().unwrap()
}

fn ticks(seconds: f64) -> u64 {
    content::millis_to_ticks((seconds * 1000.0).round() as u64)
}

fn command(sim: &mut Simulation, kind: CommandKind) {
    sim.step(&[Command { player: PlayerId(0), kind }]);
}

/// Holds the stick toward `(x, z)` (scene axes; normalized), for a tick.
fn stick(sim: &mut Simulation, x: f64, z: f64) {
    let length = (x * x + z * z).sqrt();
    let m = |v: f64| Fx::from_ratio((v / length * 1_000_000.0).round() as i64, 1_000_000);
    let direction = if length == 0.0 { FxVec2::ZERO } else { FxVec2::new(m(x), m(z)) };
    let agent = agent(sim);
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction, run: false } });
}

fn stop(sim: &mut Simulation) {
    let agent = agent(sim);
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::Stop });
}

fn jump(sim: &mut Simulation) {
    let agent = agent(sim);
    command(sim, CommandKind::Jump { agent });
}

fn let_go(sim: &mut Simulation) {
    let agent = agent(sim);
    command(sim, CommandKind::Board { agent });
}

fn run(sim: &mut Simulation, seconds: f64) {
    for _ in 0..ticks(seconds) {
        sim.step(&[]);
    }
}

/// Steps until `done` or `seconds` pass; whether it was done.
fn until(sim: &mut Simulation, seconds: f64, done: impl Fn(&Simulation) -> bool) -> bool {
    for _ in 0..ticks(seconds) {
        if done(sim) {
            return true;
        }
        sim.step(&[]);
    }
    done(sim)
}

fn climbing(sim: &Simulation) -> Climbing {
    me(sim).avatar().unwrap().climbing
}

/// The face held: its tile from the scene's origin and its side.
fn face(sim: &Simulation) -> Option<((i32, i32), Face)> {
    let origin = tile(sim.world().terrain(), 0, 0);
    climbing(sim).climb.map(|c| ((c.tile.0 - origin.0, c.tile.1 - origin.1), c.face))
}

fn on_face(sim: &Simulation) -> bool {
    climbing(sim).climb.is_some()
}

fn standing(sim: &Simulation) -> bool {
    let a = me(sim).avatar().unwrap();
    a.climbing.climb.is_none() && !a.flight.airborne
}

fn near(a: Fx, b: f64, within: f64) -> bool {
    (a.to_f64() - b).abs() <= within
}

/// Puts the elder at `x`, `z` on the ground (feet at `height`), facing
/// `(dx, dz)`, still, with a full wheel.
fn place(sim: &mut Simulation, x: f64, z: f64, height: f64, dx: i32, dz: i32) {
    let p = meters(sim, x, z);
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    let a = &mut s.agents[player];
    a.body.position = p;
    a.body.velocity = FxVec2::ZERO;
    a.body.heading = crate::nav::bearing(FxVec2::ZERO, FxVec2::new(Fx::from_int(dx), Fx::from_int(dz)));
    let avatar = a.avatar_mut().unwrap();
    let h = Fx::from_ratio((height * 1000.0).round() as i64, 1000);
    avatar.height = h;
    avatar.flight.support = h;
    avatar.climbing = Climbing::default();
}

fn set_spent(sim: &mut Simulation, spent: Fx) {
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    s.agents[player].avatar_mut().unwrap().climbing.spent = spent;
}

/// The elder on the 15 m pillar's south face (tile x 36, z 21), its feet
/// `up` meters up, the stick let go.
fn on_the_tall_pillar(up: f64) -> Simulation {
    let mut sim = gym();
    place(&mut sim, 36.3, 21.9, 0.0, 0, -1);
    stick(&mut sim, 0.0, -1.0);
    assert!(until(&mut sim, 1.0, on_face), "grabbed the pillar");
    assert!(until(&mut sim, up + 1.0, |s| feet(s).to_f64() >= up), "climbed {up} m");
    stop(&mut sim);
    sim
}

// Grabbing.

/// Walking into the 5 m pillar's south face grabs on after 0.15 s of
/// pushing (9 ticks), not before; the body hangs 2 cm off the face.
#[test]
fn pushing_into_a_face_grabs_it_after_a_short_push() {
    let mut sim = gym();
    place(&mut sim, 24.3, 21.86, 0.0, 0, -1);
    stick(&mut sim, 0.0, -1.0);
    let grab = ticks(0.15);
    for _ in 1..grab - 1 {
        sim.step(&[]);
    }
    assert!(!on_face(&sim), "not before the push time");
    assert!(until(&mut sim, 0.1, on_face));
    assert_eq!(face(&sim), Some(((24, 21), Face::South)));
    assert!(near(offset(&sim).y, 21.5 + 0.32 + 0.02, 1e-6), "off the face: {:?}", offset(&sim));
    assert_eq!(feet(&sim), Fx::ZERO);
    assert_eq!(climbing(&sim).climb.unwrap().phase, ClimbPhase::Hold);
}

/// Walking along the pillar's face, straight or a little into it, never
/// grabs it.
#[test]
fn brushing_along_a_face_does_not_grab() {
    for (x, z) in [(-1.0, 0.0), (-1.0, -0.3)] {
        let mut sim = gym();
        place(&mut sim, 27.0, 21.9, 0.0, -1, 0);
        stick(&mut sim, x, z);
        assert!(!until(&mut sim, 2.0, on_face), "brushing ({x}, {z}) grabbed");
    }
}

/// A one-layer step is walked up, never grabbed; a two-layer (1 m) wall is
/// grabbed, and pulled over at once.
#[test]
fn a_one_layer_step_is_walked_up_and_a_1_m_wall_grabbed() {
    let mut sim = small("elder", &[(true, (2, -4), (12, 4), 0, 1)], Fx::ZERO);
    stick(&mut sim, 1.0, 0.0);
    assert!(!until(&mut sim, 1.5, on_face));
    assert_eq!(feet(&sim), Fx::HALF, "walked up the step");
    let mut sim = small("elder", &[(true, (2, -4), (5, 4), 0, 2)], Fx::ZERO);
    stick(&mut sim, 1.0, 0.0);
    assert!(until(&mut sim, 1.5, on_face), "grabbed the 1 m wall");
    sim.step(&[]);
    assert!(matches!(climbing(&sim).climb.unwrap().phase, ClimbPhase::Mantle { .. }), "and pulls over at once");
    stop(&mut sim);
    assert!(until(&mut sim, 1.0, standing));
    assert_eq!(feet(&sim), Fx::ONE, "on top");
    assert!(offset(&sim).x > Fx::from_ratio(3, 2), "over the edge: {:?}", offset(&sim));
}

/// Space at a face from the ground grabs 1.5 m up it, with no stick (the
/// heading picks the face); without a face in reach, the elder (no jump of
/// its own) does nothing.
#[test]
fn a_jump_at_a_face_grabs_higher() {
    let mut sim = gym();
    place(&mut sim, 30.3, 21.9, 0.0, 0, -1);
    jump(&mut sim);
    assert_eq!(face(&sim), Some(((30, 21), Face::South)));
    assert!(until(&mut sim, 1.0, |s| climbing(s).climb.is_some_and(|c| c.phase == ClimbPhase::Hold)));
    assert!(near(feet(&sim), 1.5, 0.001), "feet {}", feet(&sim).to_f64());
    assert_eq!(climbing(&sim).spent, Fx::ZERO, "the grab is free");
    let mut sim = gym();
    place(&mut sim, 30.3, 23.0, 0.0, 0, -1);
    jump(&mut sim);
    run(&mut sim, 0.5);
    assert!(!on_face(&sim) && feet(&sim) == Fx::ZERO);
}

// On the face.

/// The stick into the face climbs at 1 m/s; out of it climbs down at
/// 1.5 m/s; along it moves sideways at 1.5 m/s. Down to the ground, the
/// elder stands.
#[test]
fn climbing_up_down_and_sideways() {
    let mut sim = on_the_tall_pillar(0.0);
    stick(&mut sim, 0.0, -1.0);
    run(&mut sim, 3.0);
    assert!(near(feet(&sim), 3.0, 0.05), "up 3 m: {}", feet(&sim).to_f64());
    stick(&mut sim, 0.0, 1.0);
    run(&mut sim, 1.0);
    assert!(near(feet(&sim), 1.5, 0.05), "down 1.5 m: {}", feet(&sim).to_f64());
    let (x, h) = (offset(&sim).x, feet(&sim));
    // On the south face, east is along it.
    stick(&mut sim, 1.0, 0.0);
    run(&mut sim, 0.5);
    assert!(near(offset(&sim).x - x, 0.75, 0.05), "sideways 0.75 m: {}", (offset(&sim).x - x).to_f64());
    assert_eq!(feet(&sim), h, "level");
    assert_eq!(face(&sim).unwrap().1, Face::South);
    stick(&mut sim, 0.0, 1.0);
    assert!(until(&mut sim, 2.0, standing), "down to the ground");
    assert_eq!(feet(&sim), Fx::ZERO);
}

/// Along the 15 m pillar's south face past its east edge, the elder wraps
/// round the outside corner onto its east face.
#[test]
fn an_outside_corner_wraps() {
    let mut sim = on_the_tall_pillar(2.0);
    stick(&mut sim, 1.0, 0.0);
    assert!(until(&mut sim, 3.0, |s| face(s).is_some_and(|f| f.1 != Face::South)));
    assert_eq!(face(&sim), Some(((37, 21), Face::East)));
    let at = offset(&sim);
    assert!(near(at.x, 37.5 + 0.34, 1e-6) && at.y < Fx::from_ratio(43, 2), "beside the east face: {at:?}");
    // On the east face, the stick north is along it, round to the north face.
    stick(&mut sim, 0.0, -1.0);
    assert!(until(&mut sim, 3.0, |s| face(s).is_some_and(|f| f.1 == Face::North)));
    assert_eq!(face(&sim), Some(((37, 20), Face::North)));
}

/// Along the shoulder's west face north into the inside corner, the elder
/// turns onto the summit's south face there.
#[test]
fn an_inside_corner_turns() {
    let mut sim = gym();
    place(&mut sim, -4.86, 3.0, 0.0, 1, 0);
    stick(&mut sim, 1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face));
    assert_eq!(face(&sim), Some(((-4, 3), Face::West)));
    stick(&mut sim, 1.0, 0.0);
    run(&mut sim, 1.0);
    // North along the west face.
    stick(&mut sim, 0.0, -1.0);
    assert!(until(&mut sim, 3.0, |s| face(s).is_some_and(|f| f.1 != Face::West)));
    assert_eq!(face(&sim), Some(((-5, 0), Face::South)));
    let at = offset(&sim);
    assert!(near(at.y, 0.5 + 0.34, 1e-6) && near(at.x, -4.5 - 0.34, 1e-6), "in the corner: {at:?}");
    // The stick north now climbs the summit's face.
    let h = feet(&sim);
    run(&mut sim, 1.0);
    assert!(feet(&sim) > h + Fx::HALF);
}

/// At the top of the 5 m pillar and the 10 m wing the elder pulls over and
/// stands on top.
#[test]
fn pulling_over_the_top() {
    let mut sim = gym();
    place(&mut sim, 24.3, 21.86, 0.0, 0, -1);
    stick(&mut sim, 0.0, -1.0);
    assert!(until(&mut sim, 1.0, on_face));
    assert!(until(&mut sim, 6.0, |s| climbing(s).climb.is_some_and(|c| matches!(c.phase, ClimbPhase::Mantle { .. }))));
    assert!(near(feet(&sim), 4.0, 0.05), "pulls over from a meter under the top: {}", feet(&sim).to_f64());
    stop(&mut sim);
    assert!(until(&mut sim, 1.0, standing));
    assert_eq!(feet(&sim), Fx::from_int(5));
    assert!(offset(&sim).y < Fx::from_ratio(43, 2) && offset(&sim).y > Fx::from_ratio(39, 2), "on the pillar: {:?}", offset(&sim));
    // The east wing's east face, 10 m.
    let mut sim = gym();
    place(&mut sim, 22.86, 0.0, 0.0, -1, 0);
    stick(&mut sim, -1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face));
    assert!(until(&mut sim, 12.0, |s| standing(s) && feet(s) > Fx::from_int(9)), "over the wing's top");
    assert_eq!(feet(&sim), Fx::from_int(10));
}

/// Under the overhang (a roof from 7 m, 4 m out from the wing's east face)
/// the climb stops with the head at the roof.
#[test]
fn an_overhang_stops_the_climb() {
    let mut sim = gym();
    place(&mut sim, 22.86, -10.0, 0.0, -1, 0);
    stick(&mut sim, -1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face));
    run(&mut sim, 8.0);
    assert!(on_face(&sim), "still on the face");
    assert!(feet(&sim) <= Fx::from_int(5) && feet(&sim) > Fx::from_ratio(49, 10), "stopped under the roof: {}", feet(&sim).to_f64());
    assert!(climbing(&sim).spent < Fx::from_ratio(1, 2), "hanging there drains slowly");
}

// Stamina.

/// From full the wheel climbs about 15 m straight up the summit's 40 m
/// north face, then lets go: the elder falls to the ground, exhausted
/// (and hurt, `fall_tests`).
#[test]
fn a_full_wheel_climbs_about_15_m() {
    let mut sim = gym();
    place(&mut sim, 0.0, -20.86, 0.0, 0, 1);
    stick(&mut sim, 0.0, 1.0);
    assert!(until(&mut sim, 1.0, on_face));
    let mut highest = Fx::ZERO;
    for _ in 0..ticks(20.0) {
        sim.step(&[]);
        highest = highest.max(feet(&sim));
        if !on_face(&sim) {
            break;
        }
    }
    assert!(!on_face(&sim), "let go");
    assert!(near(highest, 15.0, 0.2), "climbed {} m", highest.to_f64());
    let c = climbing(&sim);
    assert!(c.exhausted && c.released && c.spent == Fx::ONE);
    stop(&mut sim);
    assert!(until(&mut sim, 3.0, standing), "fell to the ground");
    assert_eq!(feet(&sim), Fx::ZERO);
}

/// Hanging still drains the wheel slowly: 10 s takes about a ninth.
#[test]
fn hanging_still_drains_slowly() {
    let mut sim = on_the_tall_pillar(1.0);
    let before = climbing(&sim).spent;
    run(&mut sim, 10.0);
    let spent = climbing(&sim).spent - before;
    assert!(near(spent, 10.0 / 90.0, 0.005), "{}", spent.to_f64());
    assert!(on_face(&sim));
}

/// On the ground an empty wheel refills in 2 s, and once full the
/// exhaustion ends.
#[test]
fn the_wheel_refills_on_the_ground() {
    let mut sim = gym();
    set_spent(&mut sim, Fx::from_ratio(9, 10));
    run(&mut sim, 1.0);
    assert!(near(climbing(&sim).spent, 0.4, 0.01), "{}", climbing(&sim).spent.to_f64());
    run(&mut sim, 1.0);
    assert_eq!(climbing(&sim).spent, Fx::ZERO);
}

/// Emptying on a face lets go; the wheel never refills on the face, and
/// exhausted the elder can't grab again until it is full on the ground.
#[test]
fn an_empty_wheel_lets_go_and_refills_only_on_the_ground() {
    let mut sim = on_the_tall_pillar(3.0);
    set_spent(&mut sim, Fx::ONE - Fx::from_ratio(1, 200));
    let mut last = climbing(&sim).spent;
    while on_face(&sim) {
        sim.step(&[]);
        assert!(climbing(&sim).spent >= last, "no refill on the face");
        last = climbing(&sim).spent;
    }
    assert!(climbing(&sim).exhausted);
    // Pushing into the face all the while.
    stick(&mut sim, 0.0, -1.0);
    assert!(until(&mut sim, 2.0, standing));
    assert_eq!(feet(&sim), Fx::ZERO);
    run(&mut sim, 1.0);
    assert!(!on_face(&sim) && climbing(&sim).exhausted, "no grab while refilling");
    assert!(until(&mut sim, 1.5, on_face), "grabs again once full");
    assert!(!climbing(&sim).exhausted);
}

/// The 40 m west face in three pitches: each climbs to its ledge, where the
/// wheel refills, and on to the summit. In one go it can't (above).
#[test]
fn the_40_m_face_with_two_rests_on_the_ledges() {
    let mut sim = gym();
    place(&mut sim, -14.86, -10.0, 0.0, 1, 0);
    for (top, name) in [(13.5, "the first ledge"), (27.0, "the second ledge"), (40.0, "the summit")] {
        stick(&mut sim, 1.0, 0.0);
        assert!(until(&mut sim, 2.0, on_face), "grabbed under {name}");
        assert!(until(&mut sim, 16.0, |s| standing(s) && feet(s) > Fx::ONE), "climbed to {name}");
        assert!(near(feet(&sim), top, 1e-6), "on {name}: {}", feet(&sim).to_f64());
        assert!(climbing(&sim).spent > Fx::from_ratio(7, 10), "the pitch took most of the wheel");
        stop(&mut sim);
        // The rest.
        assert!(until(&mut sim, 2.5, |s| climbing(s).spent == Fx::ZERO), "refilled on {name}");
    }
}

/// A lunge on the face goes 2 m (up, with no stick) for a fifth of the
/// wheel, then holds; sideways, with the stick along the face.
#[test]
fn a_lunge_goes_2_m_for_a_fifth_of_the_wheel() {
    let mut sim = on_the_tall_pillar(1.0);
    let (h, spent) = (feet(&sim), climbing(&sim).spent);
    jump(&mut sim);
    assert!(matches!(climbing(&sim).climb.unwrap().phase, ClimbPhase::Lunge { .. }));
    assert!(near(climbing(&sim).spent - spent, 0.2, 1e-6));
    assert!(until(&mut sim, 1.0, |s| climbing(s).climb.is_some_and(|c| c.phase == ClimbPhase::Hold)));
    assert!(near(feet(&sim) - h, 2.0, 0.01), "{}", (feet(&sim) - h).to_f64());
    // Sideways: east along the summit's north face.
    let mut sim = gym();
    place(&mut sim, 0.0, -20.86, 0.0, 0, 1);
    stick(&mut sim, 0.0, 1.0);
    assert!(until(&mut sim, 1.0, on_face));
    run(&mut sim, 1.0);
    stick(&mut sim, 1.0, 0.0);
    let (x, h) = (offset(&sim).x, feet(&sim));
    jump(&mut sim);
    stop(&mut sim);
    assert!(until(&mut sim, 1.0, |s| climbing(s).climb.is_some_and(|c| c.phase == ClimbPhase::Hold)));
    assert!(near(offset(&sim).x - x, 2.0, 0.01), "{}", (offset(&sim).x - x).to_f64());
    assert_eq!(feet(&sim), h);
    assert_eq!(face(&sim), Some(((2, -20), Face::North)));
}

/// G (Board) lets go: the elder falls, doesn't grab again on the way down
/// though the stick still pushes into the face, and grabs once on the
/// ground.
#[test]
fn letting_go_falls() {
    let mut sim = on_the_tall_pillar(3.0);
    stick(&mut sim, 0.0, -1.0);
    let_go(&mut sim);
    assert!(!on_face(&sim) && climbing(&sim).released);
    assert!(me(&sim).avatar().unwrap().flight.airborne, "falling");
    let mut lowest = feet(&sim);
    while !standing(&sim) {
        assert!(!on_face(&sim), "no grab falling");
        sim.step(&[]);
        assert!(feet(&sim) <= lowest);
        lowest = feet(&sim);
    }
    assert_eq!(feet(&sim), Fx::ZERO);
    assert!(until(&mut sim, 1.0, on_face), "grabs again from the ground");
}

/// Only climbers climb: the exosuit walks into a wall and stays there.
#[test]
fn characters_without_a_climb_section_do_not_climb() {
    let mut sim = small("exosuit_moonlit", &[(true, (2, -4), (5, 4), 0, 20)], Fx::ZERO);
    stick(&mut sim, 1.0, 0.0);
    run(&mut sim, 2.0);
    assert!(!on_face(&sim) && feet(&sim) == Fx::ZERO);
    assert_eq!(climbing(&sim), Climbing::default());
}

// Determinism and prediction.

/// A climb's commands: up the 10 m pillar, a lunge, along, letting go.
fn script(agent: u16) -> Vec<(u64, CommandKind)> {
    let walk = |x: i32, z: i32| CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::from_int(x), Fx::from_int(z)), run: false } };
    vec![
        (0, walk(0, -1)),
        (ticks(3.0), CommandKind::Jump { agent }),
        (ticks(4.0), walk(1, 0)),
        (ticks(4.5), CommandKind::Jump { agent }),
        (ticks(5.5), walk(0, -1)),
        (ticks(7.0), CommandKind::Board { agent }),
        (ticks(9.0), CommandKind::MoveAvatar { agent, intent: Intent::Stop }),
    ]
}

fn turn(script: &[(u64, CommandKind)], tick: u64) -> Vec<Command> {
    script.iter().filter(|(at, _)| *at == tick).map(|(_, kind)| Command { player: PlayerId(0), kind: kind.clone() }).collect()
}

fn pillar_climber(threads: usize) -> Simulation {
    let mut sim = Simulation::from_world(gym().into_world(), threads);
    place(&mut sim, 30.3, 22.4, 0.0, 0, -1);
    sim
}

/// The scripted climb hashes alike on 1 and 4 threads, tick by tick.
#[test]
fn a_climb_is_deterministic_across_thread_counts() {
    let (mut one, mut four) = (pillar_climber(1), pillar_climber(4));
    let script = script(agent(&one));
    let mut climbed = 0;
    for tick in 0..ticks(10.0) {
        let commands = turn(&script, tick);
        assert_eq!(one.step(&commands), four.step(&commands), "tick {tick}");
        climbed += usize::from(on_face(&one));
    }
    assert!(climbed > ticks(4.0) as usize, "it climbed: {climbed} ticks");
}

/// The replay predicts the scripted climb exactly as the simulation runs it.
#[test]
fn a_replayed_avatar_climbs_as_the_simulation_does() {
    let mut sim = pillar_climber(1);
    let agent = agent(&sim);
    let script = script(agent);
    let (settlement, terrain) = (sim.world().settlement().unwrap().clone(), sim.world().terrain().clone());
    let mut replay = settlement.replay_avatar(agent, sim.world().tick(), &terrain).unwrap();
    let (mut lunged, mut wrapped) = (false, false);
    for tick in 0..ticks(10.0) {
        let commands = turn(&script, tick);
        replay.step_with(commands.iter().map(|c| &c.kind));
        sim.step(&commands);
        assert_eq!(replay.state(), me(&sim), "tick {tick}");
        let phase = climbing(&sim).climb.map(|c| c.phase);
        lunged |= matches!(phase, Some(ClimbPhase::Lunge { .. }));
        wrapped |= face(&sim).is_some_and(|f| f.1 == Face::East);
    }
    assert!(lunged && wrapped, "the replay covered a lunge and a corner");
    assert!(!on_face(&sim), "and letting go");
}

// The cases CL.5 looked at: the chimney, the overhang from below, a pit.

/// Whether the elder's body is clear of rock, as the climb keeps it.
fn clear_of_rock(sim: &Simulation) -> bool {
    let a = me(sim);
    let volume = sim.world().terrain().volume.as_ref().unwrap();
    super::climb::body_clear(volume, a.body.position, a.avatar().unwrap().height, a.radius())
}

/// Steps until `done` or `seconds` pass, the body clear of rock every
/// tick; whether it was done.
fn until_clear(sim: &mut Simulation, seconds: f64, done: impl Fn(&Simulation) -> bool) -> bool {
    for _ in 0..ticks(seconds) {
        assert!(clear_of_rock(sim), "in rock: {:?}, feet {}, {:?}", offset(sim), feet(sim).to_f64(), climbing(sim).climb);
        if done(sim) {
            return true;
        }
        sim.step(&[]);
    }
    done(sim)
}

/// Round the chimney's walls (a slot 2 m wide, x 3..4, z 3..8, in the
/// shoulder): up its east wall, north into the back wall (an inside
/// corner), west across the back onto the west wall (another), south along
/// it out of the mouth round the outside corner onto the shoulder's south
/// face. Nothing special bridges the two walls: on a wall the stick out of
/// it climbs down, and a let-go grabs nothing until a floor.
#[test]
fn round_the_chimney_s_three_walls_and_out() {
    let mut sim = gym();
    place(&mut sim, 4.1, 6.0, 0.0, 1, 0);
    stick(&mut sim, 1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face));
    assert_eq!(face(&sim), Some(((5, 6), Face::West)), "the east wall");
    run(&mut sim, 2.0);
    let legs: [((f64, f64), ((i32, i32), Face)); 3] = [((0.0, -1.0), ((4, 2), Face::South)), ((-1.0, 0.0), ((2, 3), Face::East)), ((0.0, 1.0), ((2, 8), Face::South))];
    for ((x, z), want) in legs {
        stick(&mut sim, x, z);
        assert!(until_clear(&mut sim, 6.0, |s| face(s) == Some(want)), "toward {want:?}: at {:?} on {:?}", offset(&sim), face(&sim));
        assert_eq!(face(&sim), Some(want), "at {:?}", offset(&sim));
    }
    assert!(feet(&sim) > Fx::ONE, "still up: {}", feet(&sim).to_f64());
    // Out of the wall climbs down, it doesn't cross: let go, and the
    // stick at the other wall grabs nothing on the way down.
    let mut sim = gym();
    place(&mut sim, 4.1, 6.0, 0.0, 1, 0);
    stick(&mut sim, 1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face));
    run(&mut sim, 3.0);
    stick(&mut sim, -1.0, 0.0);
    let_go(&mut sim);
    assert!(until_clear(&mut sim, 3.0, standing), "fell");
    assert_eq!(feet(&sim), Fx::ZERO);
    assert!(until(&mut sim, 2.0, on_face), "grabs the west wall once down: {:?}", offset(&sim));
    assert_eq!(face(&sim), Some(((2, 6), Face::East)));
}

/// The overhang from below (a roof from 7 m to 10 m, 4 m out from the
/// wing's east face, z -14..-6): from the ground under it nothing grabs it;
/// up the wing's face under it the climb stops at the roof, a lunge up
/// there stays under it, and along the face out from under it the elder
/// climbs on to the wing's top, walks out over the roof and off its edge:
/// 10 m, 3 health.
#[test]
fn the_overhang_from_below() {
    let mut sim = gym();
    place(&mut sim, 24.5, -10.0, 0.0, 0, -1);
    jump(&mut sim);
    run(&mut sim, 0.5);
    assert!(!on_face(&sim), "nothing to grab under the roof");
    place(&mut sim, 22.86, -7.0, 0.0, -1, 0);
    stick(&mut sim, -1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face));
    assert!(!until_clear(&mut sim, 6.0, |s| feet(s) > Fx::from_int(5)), "stopped under the roof");
    jump(&mut sim);
    assert!(!until_clear(&mut sim, 1.0, |s| feet(s) > Fx::from_int(5)), "a lunge up stays under it");
    assert!(on_face(&sim));
    set_spent(&mut sim, Fx::ZERO);
    stick(&mut sim, 0.0, 1.0);
    // The roof ends at z -5.5; the body (0.32 m round) clears it past -5.18.
    assert!(until_clear(&mut sim, 3.0, |s| offset(s).y > Fx::from_ratio(-51, 10)), "out from under the roof: {:?}", offset(&sim));
    stick(&mut sim, -1.0, 0.0);
    assert!(until_clear(&mut sim, 8.0, |s| standing(s) && feet(s) > Fx::from_int(9)), "on the wing's top");
    assert_eq!(feet(&sim), Fx::from_int(10));
    // Over the roof and off its east edge.
    place(&mut sim, 24.5, -10.0, 10.0, 1, 0);
    sim.step(&[]);
    assert_eq!(feet(&sim), Fx::from_int(10), "on the roof");
    stick(&mut sim, 1.0, 0.0);
    assert!(until_clear(&mut sim, 5.0, |s| standing(s) && feet(s) == Fx::ZERO), "off the edge");
    assert_eq!(me(&sim).body.health, 3, "a 10 m fall");
}

/// A pit 3 m deep (dug, as in the town): pushing into its wall grabs it,
/// and the elder climbs out and stands on the ground.
#[test]
fn climbing_out_of_a_pit() {
    let mut sim = small("elder", &[(false, (-1, -1), (1, 1), -6, 0)], Fx::from_int(-3));
    assert_eq!(feet(&sim), Fx::from_int(-3), "in the pit");
    stick(&mut sim, 1.0, 0.0);
    assert!(until(&mut sim, 1.0, on_face), "grabbed the pit's wall");
    assert_eq!(face(&sim).unwrap().1, Face::West);
    assert!(until_clear(&mut sim, 5.0, |s| standing(s) && feet(s) == Fx::ZERO), "out of the pit: {:?}, {}", offset(&sim), feet(&sim).to_f64());
    assert!(offset(&sim).x > Fx::from_ratio(3, 2), "on the ground beside it: {:?}", offset(&sim));
}

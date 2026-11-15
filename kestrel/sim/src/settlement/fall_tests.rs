//! Falls hurt (CL.3 of the climbing design, `kestrel/CLIMBING.md` K7): a
//! climber's fall, from its highest point to the floor it lands on, takes
//! nothing under the climb's `safe_fall` (5 m for the elder), then 1
//! health and 1 more per `fall_step` (2.5 m), so about 20 m kills a
//! 6-health elder, who comes back at home as after any death. In the
//! Climbing Gym (6 health) and small volumes; determinism, the replay
//! predicting a fall, and where players have no health.

use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::{Intent, JumpPhase};
use super::climb::{fall_damage, Climbing};
use super::climbing_gym_tests::gym;
use super::tunnel_lab_tests::{feet, me, meters, small, Cells};
use super::{Event, SceneSetup};
use crate::volume::Volume;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

fn agent(sim: &Simulation) -> u16 {
    sim.world().settlement().unwrap().player().unwrap()
}

fn ticks(seconds: f64) -> u64 {
    content::millis_to_ticks((seconds * 1000.0).round() as u64)
}

fn m(v: f64) -> Fx {
    Fx::from_ratio((v * 1000.0).round() as i64, 1000)
}

fn command(sim: &mut Simulation, kind: CommandKind) {
    sim.step(&[Command { player: PlayerId(0), kind }]);
}

/// Walks the stick toward `(x, z)` (scene axes, an axis's unit) for a tick.
fn walk(sim: &mut Simulation, x: i32, z: i32) {
    let agent = agent(sim);
    let direction = FxVec2::new(Fx::from_int(x), Fx::from_int(z));
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction, run: false } });
}

fn stop(sim: &mut Simulation) {
    let agent = agent(sim);
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::Stop });
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

fn health(sim: &Simulation) -> u32 {
    me(sim).body.health
}

fn on_face(sim: &Simulation) -> bool {
    me(sim).avatar().unwrap().climbing.climb.is_some()
}

/// On a floor: off any face, not in the air.
fn standing(sim: &Simulation) -> bool {
    let a = me(sim).avatar().unwrap();
    a.climbing.climb.is_none() && !a.flight.airborne && !matches!(a.jump, JumpPhase::Airborne { .. })
}

/// The falls that hurt on the last tick: (height, damage, killed).
fn fell(sim: &Simulation) -> Vec<(Fx, u32, bool)> {
    let events = &sim.world().settlement().unwrap().events;
    events.iter().filter_map(|e| if let Event::Fell { height, damage, killed, .. } = *e { Some((height, damage, killed)) } else { None }).collect()
}

/// Steps until the elder stands on a floor (at most 5 s), collecting the
/// falls that hurt on the way.
fn land(sim: &mut Simulation) -> Vec<(Fx, u32, bool)> {
    let mut falls = Vec::new();
    for _ in 0..ticks(5.0) {
        sim.step(&[]);
        falls.extend(fell(sim));
        if standing(sim) {
            return falls;
        }
    }
    panic!("never landed: feet {}", feet(sim).to_f64());
}

/// Puts the elder at `x`, `z` (scene meters) with its feet at `height`,
/// still, off any face, with a full wheel. Over lower ground it falls from
/// there, as off a ledge.
fn place(sim: &mut Simulation, x: f64, z: f64, height: f64) {
    let p = meters(sim, x, z);
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    let a = &mut s.agents[player];
    a.body.position = p;
    a.body.velocity = FxVec2::ZERO;
    let avatar = a.avatar_mut().unwrap();
    avatar.height = m(height);
    avatar.flight.support = m(height);
    avatar.climbing = Climbing::default();
}

/// The elder on the 15 m pillar's south face (tile x 36, z 21), its feet
/// about `up` meters up, the stick let go.
fn on_the_tall_pillar(up: f64) -> Simulation {
    let mut sim = gym();
    place(&mut sim, 36.3, 21.9, 0.0);
    walk(&mut sim, 0, -1);
    assert!(until(&mut sim, 1.0, on_face), "grabbed the pillar");
    assert!(until(&mut sim, up + 1.0, |s| feet(s).to_f64() >= up), "climbed {up} m");
    stop(&mut sim);
    sim
}

fn elder_climb() -> content::character::ClimbDef {
    let content = Content::builtin();
    content.rules.characters[content.names.character_index("elder").unwrap() as usize].climb.unwrap()
}

/// The steps themselves: nothing under 5 m, 1 from 5 m, 1 more per 2.5 m.
#[test]
fn the_damage_steps_every_2_5_m_from_5_m() {
    let climb = elder_climb();
    for (height, damage) in [(0.0, 0), (4.99, 0), (5.0, 1), (7.49, 1), (7.5, 2), (10.0, 3), (12.5, 4), (15.0, 5), (17.5, 6), (20.0, 7), (40.0, 15)] {
        assert_eq!(fall_damage(m(height), &climb), damage, "{height} m");
    }
}

/// Drops onto the gym's flat from 4.5, 5, 7.5, 10 and 15 m take 0, 1, 2,
/// 3 and 5 of the elder's 6 health on landing, each shown by one `Fell`.
#[test]
fn falls_hurt_by_their_height() {
    for (height, damage) in [(4.5, 0), (5.0, 1), (7.5, 2), (10.0, 3), (15.0, 5)] {
        let mut sim = gym();
        assert_eq!(health(&sim), 6, "the gym's elder has 6 health");
        place(&mut sim, 44.0, 30.0, height);
        let falls = land(&mut sim);
        assert_eq!(feet(&sim), Fx::ZERO, "on the flat");
        assert_eq!(health(&sim), 6 - damage, "a {height} m fall");
        let expected = if damage == 0 { vec![] } else { vec![(m(height), damage, false)] };
        assert_eq!(falls, expected, "a {height} m fall");
        // Kept on the landing tick, for housekeeping; then gone.
        assert_eq!(me(&sim).avatar().unwrap().fall_top, Some(m(height)));
        sim.step(&[]);
        assert_eq!(me(&sim).avatar().unwrap().fall_top, None, "the fall is over");
    }
}

/// Walking off a pillar's top is a fall like any: off the 10 m one, 3.
#[test]
fn walking_off_a_pillar_hurts() {
    let mut sim = gym();
    place(&mut sim, 30.5, 20.5, 10.0);
    sim.step(&[]);
    assert_eq!(feet(&sim), Fx::from_int(10), "on the pillar's top");
    walk(&mut sim, 0, 1);
    assert!(until(&mut sim, 2.0, |s| feet(s) < Fx::from_int(10)), "walked off");
    stop(&mut sim);
    assert_eq!(land(&mut sim), vec![(Fx::from_int(10), 3, false)]);
    assert_eq!(health(&sim), 3);
}

/// Letting go 4 m up a face doesn't hurt.
#[test]
fn a_let_go_at_4_m_does_not_hurt() {
    let mut sim = on_the_tall_pillar(4.0);
    let up = feet(&sim);
    assert!(up < Fx::from_int(5), "{}", up.to_f64());
    let agent = agent(&sim);
    command(&mut sim, CommandKind::Board { agent });
    assert!(!on_face(&sim));
    assert_eq!(land(&mut sim), vec![]);
    assert_eq!((feet(&sim), health(&sim)), (Fx::ZERO, 6));
}

/// A small volume with player health: 33 m of ground at 0 m and `boxes`
/// over it, the elder at the center.
fn hurtable(boxes: &[Cells]) -> Simulation {
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
        player: "elder".into(),
        extent: Fx::from_int(16),
        diggable: true,
        sloped: true,
        volume: Some(volume),
        player_health: 6,
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// Down a 1.5 m terrace (three layers) the elder falls, unhurt.
#[test]
fn going_down_a_terrace_does_not_hurt() {
    let mut sim = hurtable(&[(true, (-16, -16), (2, 16), 0, 3)]);
    place(&mut sim, 1.0, 0.0, 1.5);
    sim.step(&[]);
    assert_eq!(feet(&sim), Fx::from_ratio(3, 2), "on the terrace");
    walk(&mut sim, 1, 0);
    let mut fell_off = false;
    for _ in 0..ticks(2.0) {
        sim.step(&[]);
        assert!(fell(&sim).is_empty());
        fell_off |= matches!(me(&sim).avatar().unwrap().jump, JumpPhase::Airborne { .. });
    }
    assert!(fell_off, "it fell off the terrace");
    assert_eq!((feet(&sim), health(&sim)), (Fx::ZERO, 6));
}

/// Walking off the 20 m shoulder kills; two seconds later the elder is
/// back at the spawn with its 6 health, as after any death.
#[test]
fn a_20_m_fall_kills_and_the_elder_comes_back() {
    let mut sim = gym();
    let home = me(&sim).body.home;
    place(&mut sim, 7.0, 7.0, 20.0);
    sim.step(&[]);
    assert_eq!(feet(&sim), Fx::from_int(20), "on the shoulder");
    walk(&mut sim, 0, 1);
    assert!(until(&mut sim, 2.0, |s| feet(s) < Fx::from_int(20)), "walked off");
    let falls = land(&mut sim);
    assert_eq!(falls, vec![(Fx::from_int(20), 7, true)]);
    assert_eq!(health(&sim), 0);
    assert!(!me(&sim).body.alive(), "dead");
    assert!(until(&mut sim, 2.5, |s| me(s).body.alive()), "comes back");
    let a = me(&sim);
    assert_eq!((a.body.position, a.body.health), (home, 6));
    assert_eq!(feet(&sim), Fx::ZERO);
    assert_eq!(a.avatar().unwrap().fall_top, None);
}

/// Running out of stamina about 15 m up the summit's north face lets go,
/// and the fall hurts as its height says: 4 or 5 of 6.
#[test]
fn running_out_of_stamina_at_15_m_hurts_as_computed() {
    let mut sim = gym();
    place(&mut sim, 0.0, -20.86, 0.0);
    walk(&mut sim, 0, 1);
    assert!(until(&mut sim, 1.0, on_face));
    let mut highest = Fx::ZERO;
    for _ in 0..ticks(20.0) {
        sim.step(&[]);
        highest = highest.max(feet(&sim));
        if !on_face(&sim) {
            break;
        }
    }
    assert!(me(&sim).avatar().unwrap().climbing.exhausted, "ran out");
    stop(&mut sim);
    let falls = land(&mut sim);
    let damage = fall_damage(highest, &elder_climb());
    assert!((4..=5).contains(&damage), "{} m", highest.to_f64());
    assert_eq!(falls, vec![(highest, damage, false)]);
    assert_eq!(health(&sim), 6 - damage);
}

/// A fall counts from its top: a lunge from 4 m up to about 6 m, then
/// letting go, takes 1 (a let-go at 4 m takes none, above); and a body
/// thrown up from 4 m to about 6 m before falling is hurt from 6 m.
#[test]
fn a_fall_counts_from_its_highest_point() {
    let mut sim = on_the_tall_pillar(4.0);
    let agent = agent(&sim);
    command(&mut sim, CommandKind::Jump { agent });
    assert!(until(&mut sim, 1.0, |s| me(s).avatar().unwrap().climbing.climb.is_some_and(|c| c.phase == super::climb::ClimbPhase::Hold)));
    let top = feet(&sim);
    assert!(top > Fx::from_int(5), "lunged to {}", top.to_f64());
    command(&mut sim, CommandKind::Board { agent });
    assert_eq!(land(&mut sim), vec![(top, 1, false)]);
    // Rising from 4 m first: the top is where it turned.
    let mut sim = gym();
    place(&mut sim, 44.0, 30.0, 4.0);
    {
        let s = sim.world_mut().settlement_mut();
        let player = s.player().unwrap() as usize;
        s.agents[player].avatar_mut().unwrap().jump = JumpPhase::Airborne { started: 0, velocity: Fx::from_ratio(3, 20) };
        s.agents[player].avatar_mut().unwrap().flight.airborne = true;
    }
    let mut highest = Fx::ZERO;
    let mut falls = Vec::new();
    for _ in 0..ticks(3.0) {
        sim.step(&[]);
        highest = highest.max(feet(&sim));
        falls.extend(fell(&sim));
        if standing(&sim) {
            break;
        }
    }
    assert!(highest > Fx::from_ratio(59, 10) && highest < Fx::from_ratio(15, 2), "rose to {}", highest.to_f64());
    assert_eq!(falls, vec![(highest, 1, false)]);
}

/// Where players have no health (a scene without it, as the Tunnel Lab),
/// a fall hurts nothing.
#[test]
fn without_player_health_falls_do_not_hurt() {
    let mut sim = small("elder", &[], Fx::ZERO);
    assert_eq!(me(&sim).body.max_health, 0);
    place(&mut sim, 0.0, 0.0, 12.0);
    assert_eq!(land(&mut sim), vec![]);
    assert!(me(&sim).body.alive());
    assert_eq!(feet(&sim), Fx::ZERO);
}

// Determinism and prediction.

/// Off the 10 m pillar, walking south from its top.
fn pillar_jumper(threads: usize) -> Simulation {
    let mut sim = Simulation::from_world(gym().into_world(), threads);
    place(&mut sim, 30.5, 20.5, 10.0);
    sim
}

fn south(agent: u16) -> CommandKind {
    CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false } }
}

/// A hurting fall hashes alike on 1 and 4 threads, tick by tick.
#[test]
fn a_hurting_fall_is_deterministic_across_thread_counts() {
    let (mut one, mut four) = (pillar_jumper(1), pillar_jumper(4));
    let walk = [Command { player: PlayerId(0), kind: south(agent(&one)) }];
    for tick in 0..ticks(4.0) {
        let commands: &[Command] = if tick == 0 { &walk } else { &[] };
        assert_eq!(one.step(commands), four.step(commands), "tick {tick}");
    }
    assert_eq!((health(&one), health(&four)), (3, 3));
}

/// The replay predicts the fall as the simulation runs it, the top of the
/// fall among it; only the damage stays the confirmed world's.
#[test]
fn a_replayed_avatar_falls_as_the_simulation_does() {
    let mut sim = pillar_jumper(1);
    let agent = agent(&sim);
    let (settlement, terrain) = (sim.world().settlement().unwrap().clone(), sim.world().terrain().clone());
    let mut replay = settlement.replay_avatar(agent, sim.world().tick(), &terrain).unwrap();
    let mut tops = 0;
    for tick in 0..ticks(4.0) {
        let commands = if tick == 0 { vec![Command { player: PlayerId(0), kind: south(agent) }] } else { vec![] };
        replay.step_with(commands.iter().map(|c| &c.kind));
        sim.step(&commands);
        let mut predicted = replay.state().clone();
        predicted.body.health = me(&sim).body.health;
        assert_eq!(&predicted, me(&sim), "tick {tick}");
        tops += usize::from(me(&sim).avatar().unwrap().fall_top.is_some());
    }
    assert!(tops > 10, "it fell for {tops} ticks");
    assert_eq!(health(&sim), 3, "hurt in the confirmed world");
    assert_eq!(replay.state().body.health, 6, "the replay leaves damage to it");
}


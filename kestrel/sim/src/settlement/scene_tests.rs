//! Scenes without a supply chain: jetpack flight, the memory trial and the
//! pile lab, following the original game's demo scripts (`kestrel/app/demos`).

use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::avatar::{JumpPhase, SwordStroke};
use super::piles::PileAction;
use super::{PileSetup, SceneSetup, Settlement};
use crate::terrain::{Disc, Surface};
use crate::{Command, CommandKind, PlayerId, Simulation, World};

/// Ticks in `n` milliseconds.
fn ms(n: u64) -> u64 {
    content::millis_to_ticks(n)
}

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn v(x: f64, y: f64) -> FxVec2 {
    FxVec2::new(fx(x), fx(y))
}

struct Scene {
    sim: Simulation,
    center: FxVec2,
}

impl Scene {
    fn new(setup: SceneSetup) -> Scene {
        let world = World::new_scene(1, &Content::builtin(), &setup).unwrap();
        let center = world.terrain().center;
        Scene { sim: Simulation::from_world(world, 2), center }
    }

    fn s(&self) -> &Settlement {
        self.sim.world().settlement().unwrap()
    }

    fn at(&self, x: f64, y: f64) -> FxVec2 {
        self.center + v(x, y)
    }

    fn agent(&self) -> u16 {
        self.s().player().unwrap()
    }

    fn command(&mut self, kind: CommandKind) {
        self.sim.step(&[Command { player: PlayerId(0), kind }]);
    }

    fn wait(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.sim.step(&[]);
        }
    }

    /// Like the scripts' `walk`: go there and wait until the route is done.
    fn walk(&mut self, x: f64, y: f64) {
        let (agent, target) = (self.agent(), self.at(x, y));
        self.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: false } });
        assert!(!self.avatar().route.is_empty(), "no route to ({x}, {y})");
        for _ in 0..ms(30_000) {
            if self.avatar().route.is_empty() {
                return;
            }
            self.sim.step(&[]);
        }
        panic!("walk to ({x}, {y}) never arrived");
    }

    fn avatar(&self) -> &super::AgentState {
        &self.s().agents[self.agent() as usize]
    }

    fn lab(&self) -> &super::piles::PileLab {
        self.s().piles.as_ref().unwrap()
    }

    /// The player's pile-lab hauler, once it has hauled.
    fn hauler(&self) -> super::piles::Hauler {
        self.s().piles.as_ref().unwrap().hauler(self.agent()).cloned().expect("the player has hauled")
    }

    fn altitude(&self) -> Fx {
        self.avatar().avatar.height
    }
}

/// What every test scene shares: the builtin exosuit as the player.
fn base() -> SceneSetup {
    SceneSetup { player: "exosuit".into(), ..SceneSetup::default() }
}

/// The jetpack lab's platforms; everything else is a drop.
fn jetpack_lab() -> SceneSetup {
    let platform = |min: (f64, f64), max: (f64, f64), height: f64| Surface { min: v(min.0, min.1), max: v(max.0, max.1), height: fx(height) };
    SceneSetup {
        extent: Fx::from_int(8),
        surfaces: vec![
            platform((-7.0, -1.0), (-2.0, 6.0), 0.0),
            platform((-2.0, -1.0), (1.0, 4.0), 2.0),
            platform((3.0, -3.0), (7.0, 4.0), 3.5),
            platform((-4.0, -6.0), (1.0, -3.0), 1.0),
        ],
        platforms_only: true,
        spawn: [fx(-5.0), Fx::ZERO, fx(3.0)],
        ..base()
    }
}

#[test]
fn jump_has_anticipation_arc_landing_and_no_air_restart() {
    let mut scene = Scene::new(SceneSetup { jump_enabled: true, ..field() });
    let agent = scene.agent();
    let ground = scene.altitude();
    scene.command(CommandKind::Jump { agent });
    assert!(matches!(scene.avatar().avatar.jump, JumpPhase::Preparing { .. }));
    scene.wait(ms(220));
    assert!(matches!(scene.avatar().avatar.jump, JumpPhase::Airborne { .. }));
    assert!(scene.altitude() > ground + fx(0.2));
    scene.wait(ms(130));
    assert!(scene.altitude() > ground + fx(0.4));
    scene.command(CommandKind::Jump { agent });
    assert!(matches!(scene.avatar().avatar.jump, JumpPhase::Airborne { .. }));
    scene.wait(ms(1300));
    assert_eq!(scene.avatar().avatar.jump, JumpPhase::Grounded);
    assert_eq!(scene.altitude(), ground);
    assert!(!scene.avatar().avatar.flight.airborne);
}

#[test]
fn repeated_sword_attacks_cycle_then_reset() {
    let mut scene = Scene::new(SceneSetup { swift_sword: true, ..field() });
    let agent = scene.agent();
    let attack = |scene: &mut Scene| {
        scene.command(CommandKind::Attack {
            agent,
            weapon: content::scenario::Weapon::Sword,
            aim: v(1.0, 0.0),
            muzzle: [Fx::ZERO; 3],
        });
        scene.avatar().avatar.swing.as_ref().unwrap().stroke
    };
    assert_eq!(attack(&mut scene), SwordStroke::Forehand);
    let swing = scene.avatar().avatar.swing.as_ref().unwrap();
    assert_eq!(swing.resolve_at - swing.started, content::scenario::ticks_from_60hz(7), "the exosuit's swift sword windup");
    scene.wait(ms(600));
    assert_eq!(attack(&mut scene), SwordStroke::Backhand);
    scene.wait(ms(600));
    assert_eq!(attack(&mut scene), SwordStroke::Overhead);
    scene.wait(ms(1500));
    assert_eq!(attack(&mut scene), SwordStroke::Forehand);
}

#[test]
fn ground_routes_cannot_cross_walls_or_drops() {
    let mut scene = Scene::new(jetpack_lab());
    let agent = scene.agent();
    let go = |scene: &mut Scene, x: f64, y: f64| {
        let target = scene.at(x, y);
        scene.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: false } });
        !scene.avatar().route.is_empty()
    };
    assert!(go(&mut scene, -4.0, 0.0));
    assert!(!go(&mut scene, 0.0, 2.0), "a 2 m wall");
    assert!(!go(&mut scene, 2.0, 2.0), "a drop");
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    assert!(go(&mut scene, 5.0, 2.0), "flying, anything goes");
}

#[test]
fn feet_rise_when_body_reaches_a_low_step() {
    let mut scene = Scene::new(SceneSetup {
        extent: Fx::from_int(8),
        surfaces: vec![Surface { min: v(0.0, -1.0), max: v(1.0, 1.0), height: fx(0.24) }],
        spawn: [fx(-1.0), Fx::ZERO, Fx::ZERO],
        ..base()
    });
    let agent = scene.agent();
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: v(1.0, 0.0), run: false } });
    let mut stepped_at_edge = false;
    for _ in 0..ms(2500) {
        scene.sim.step(&[]);
        let x = scene.avatar().position.x - scene.center.x;
        if x > fx(-0.3) && x < Fx::ZERO {
            stepped_at_edge = true;
            assert_eq!(scene.altitude(), fx(0.24), "feet must rise when the body first overlaps the slab");
            break;
        }
    }
    assert!(stepped_at_edge, "avatar never reached the slab edge: x={} height={}", (scene.avatar().position.x - scene.center.x).to_f32(), scene.altitude().to_f32());
}

/// `demos/jetpack.json`: climb onto the 2 m platform, cross the drop, hover
/// over it with the jetpack released, and land on the 3.5 m platform.
#[test]
fn jetpack_climbs_crosses_a_drop_hovers_and_lands() {
    let mut scene = Scene::new(jetpack_lab());
    let agent = scene.agent();
    let close = |a: Fx, b: f64| (a - fx(b)).abs() <= fx(0.05);
    scene.wait(ms(1000));
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.wait(ms(1000));
    scene.walk(0.0, 2.0);
    scene.wait(ms(1000));
    assert!(scene.avatar().avatar.flight.airborne && close(scene.altitude(), 3.8), "hovering at {}", scene.altitude());
    scene.walk(2.0, 2.0);
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.wait(ms(1000));
    assert!(scene.avatar().avatar.flight.airborne && close(scene.altitude(), 3.8), "kept aloft over the drop at {}", scene.altitude());
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.walk(5.0, 1.0);
    scene.wait(ms(1000));
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.wait(ms(1333));
    assert!(!scene.avatar().avatar.flight.airborne && close(scene.altitude(), 3.5), "landed at {}", scene.altitude());
}

/// The surprise lab: a wall at x = 2 with a door in the middle, two pads and
/// the core behind the door.
fn surprise_lab() -> SceneSetup {
    let (mut walls, mut doors) = (Vec::new(), Vec::new());
    for n in -18..=18 {
        let z = n as f64 * 0.45;
        let disc = Disc { center: v(2.0, z), radius: fx(0.24) };
        if z.abs() < 1.15 {
            doors.push(disc);
        } else {
            walls.push(disc);
        }
    }
    SceneSetup {
        extent: Fx::from_int(8),
        obstacles: walls,
        doors,
        spawn: [fx(-5.0), Fx::ZERO, fx(3.0)],
        memory_trial: true,
        ..base()
    }
}

/// `demos/surprise.json`: record a walk to the first pad, let the ghost
/// replay it while the player stands on the second, and collect the core.
#[test]
fn memory_trial_needs_a_ghost_to_open_the_door() {
    let mut scene = Scene::new(surprise_lab());
    let memory = |scene: &Scene| scene.s().memory.clone().unwrap();
    scene.wait(ms(1333));
    assert!(!memory(&scene).unlocked);
    scene.command(CommandKind::MemoryRecord);
    scene.walk(-4.0, -2.0);
    scene.wait(ms(667));
    scene.command(CommandKind::MemoryDeploy);
    assert_eq!(scene.avatar().position, scene.at(-5.0, 3.0), "deploying returns the player to the start");
    scene.wait(ms(667));
    scene.walk(-1.0, 3.0);
    scene.wait(ms(3333));
    let m = memory(&scene);
    assert!(m.unlocked && !m.collected, "{:?}", (m.pressed, m.charge));
    scene.walk(5.0, 0.0);
    scene.wait(ms(1333));
    assert!(memory(&scene).collected);
}

#[test]
fn the_trial_cannot_be_completed_alone() {
    let mut scene = Scene::new(surprise_lab());
    scene.walk(-4.0, -2.0);
    scene.walk(-1.0, 3.0);
    scene.wait(ms(2000));
    assert!(!scene.s().memory.as_ref().unwrap().unlocked);
    // The door blocks the way to the core.
    let (agent, target) = (scene.agent(), scene.at(5.0, 0.0));
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: false } });
    let reached = scene.avatar().route.last() == Some(&target);
    assert!(!reached || scene.avatar().route.len() > 1, "only around the wall, if at all");
}

#[test]
fn recordings_stop_at_eight_seconds_and_deploy_themselves() {
    let mut scene = Scene::new(surprise_lab());
    scene.command(CommandKind::MemoryRecord);
    scene.wait(ms(8667));
    let m = scene.s().memory.clone().unwrap();
    assert!(!m.recording && m.ghost.is_some());
    assert_eq!(m.elapsed, super::memory::LIMIT_TICKS);
    scene.command(CommandKind::MemoryClear);
    assert!(scene.s().memory.as_ref().unwrap().ghost.is_none());
}

/// The trial records the first avatar, so only its owner drives the trial.
#[test]
fn only_the_trial_avatars_owner_records_and_deploys() {
    let mut scene = Scene::new(surprise_lab());
    let guest = PlayerId(1);
    scene.sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    scene.sim.step(&[Command { player: guest, kind: CommandKind::MemoryRecord }]);
    assert!(!scene.s().memory.as_ref().unwrap().recording, "the guest cannot start a recording");
    scene.command(CommandKind::MemoryRecord);
    scene.walk(-4.0, -2.0);
    scene.wait(ms(667));
    scene.sim.step(&[Command { player: guest, kind: CommandKind::MemoryDeploy }]);
    assert!(scene.s().memory.as_ref().unwrap().recording, "the guest cannot deploy the host's recording");
    assert_ne!(scene.avatar().position, scene.at(-5.0, 3.0), "the host's avatar was not sent back");
    scene.sim.step(&[Command { player: guest, kind: CommandKind::MemoryClear }]);
    assert!(!scene.s().memory.as_ref().unwrap().samples.is_empty(), "the guest cannot clear it");
}

fn pile_lab() -> SceneSetup {
    let p = |x: f64, z: f64| [fx(x), fx(z)];
    SceneSetup {
        extent: Fx::from_int(8),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::ONE],
        piles: Some(PileSetup {
            capacity: 12,
            dispensers: vec![(0, p(-3.1, -2.0)), (1, p(2.2, -3.1))],
            piles: vec![(0, p(-4.0, 3.4), 8), (1, p(0.2, 4.3), 24), (0, p(4.6, 1.0), 64)],
            collector: None,
        }),
        ..base()
    }
}

/// `demos/haul_four.json`: walk to a dispenser's pile, take exactly four
/// units one at a time, carry them across, and start a new pile.
#[test]
fn pile_lab_hauls_exactly_four_units() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    let finish = |scene: &mut Scene| {
        for _ in 0..ms(30_000) {
            if scene.hauler().job.is_none() {
                return;
            }
            scene.sim.step(&[]);
        }
        panic!("the haul never finished");
    };
    scene.command(CommandKind::Dispense { dispenser: 0, units: 8 });
    scene.wait(ms(1000));
    scene.walk(-1.7, -3.4);
    let approach = Some(scene.at(-2.3, -2.8));
    scene.command(CommandKind::Pile { agent, action: PileAction::PickupAmount(0, 4), pace: fx(0.7), approach });
    let start = scene.avatar().position;
    finish(&mut scene);
    let l = scene.hauler();
    assert_eq!(l.last_error, None);
    assert_eq!(l.cargo.units, 4);
    assert_eq!(scene.lab().total(), 104);
    assert_eq!(scene.lab().piles[&0].units, 4);
    assert_ne!(scene.avatar().position, start, "walked before transferring");
    scene.wait(ms(1000));
    scene.walk(2.0, 1.0);
    let (drop, approach) = (scene.at(4.0, 4.5), Some(scene.at(2.9, 5.0)));
    scene.command(CommandKind::Pile { agent, action: PileAction::Drop(drop), pace: fx(0.7), approach });
    finish(&mut scene);
    let l = scene.hauler();
    assert_eq!(l.last_error, None);
    assert_eq!(l.cargo.units, 0);
    assert_eq!(scene.lab().total(), 104);
    assert_eq!(scene.lab().piles[&5].units, 4);
    assert!(scene.lab().conserved());
}

#[test]
fn moving_cancels_a_haul_and_keeps_the_cargo() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None });
    assert!(scene.hauler().job.is_some());
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: false } });
    let l = scene.hauler();
    assert!(l.job.is_none());
    assert_eq!(l.last_error.as_deref(), Some("Haul canceled by player input"));
    assert!(scene.lab().conserved());
}

#[test]
fn blocked_drops_are_rejected_without_losing_cargo() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None });
    for _ in 0..ms(20_000) {
        if scene.hauler().job.is_none() {
            break;
        }
        scene.sim.step(&[]);
    }
    let carried = scene.hauler().cargo.units;
    assert_eq!(carried, 8);
    // Off the deck.
    let edge = scene.at(7.5, 7.5);
    scene.command(CommandKind::Pile { agent, action: PileAction::Drop(edge), pace: Fx::ZERO, approach: None });
    let l = scene.hauler();
    assert!(l.job.is_none() && l.last_error.is_some());
    assert_eq!(l.cargo.units, carried);
    assert!(scene.lab().conserved());
}

/// Scenes save and continue like any world: piles, ghosts and flight included.
#[test]
fn scenes_are_deterministic_across_thread_counts() {
    let run = |threads| {
        let world = World::new_scene(4, &Content::builtin(), &surprise_lab()).unwrap();
        let mut sim = Simulation::from_world(world, threads);
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let mut hashes = Vec::new();
        hashes.push(sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MemoryRecord }]).total);
        let target = sim.world().terrain().center + v(-4.0, -2.0);
        hashes.push(sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: true } } }]).total);
        hashes.extend((0..ms(4000)).map(|_| sim.step(&[]).total));
        hashes.push(sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MemoryDeploy }]).total);
        hashes.extend((0..ms(4000)).map(|_| sim.step(&[]).total));
        hashes
    };
    assert_eq!(run(1), run(8));
}

/// A flat field with nothing on it, like the app's `field` scene.
fn field() -> SceneSetup {
    SceneSetup { extent: Fx::from_int(8), spawn: [Fx::ZERO, Fx::ZERO, fx(2.0)], ..base() }
}

#[test]
fn authored_locomotion_speeds_control_avatar_movement() {
    let travel = |speed: f64| {
        let mut scene = Scene::new(SceneSetup {
            locomotion: Some(crate::terrain::Locomotion { walk: fx(speed), run: fx(speed + 0.5) }),
            ..field()
        });
        let agent = scene.agent();
        scene.command(CommandKind::MoveAvatar {
            agent,
            intent: Intent::Walk { direction: v(1.0, 0.0), run: false },
        });
        scene.wait(ms(600));
        scene.avatar().position.x - scene.center.x
    };
    assert!(travel(2.0) > travel(1.0) + fx(0.4));
}

/// A player who joins gets an avatar of their own, drives only that one,
/// and joining twice doesn't add a second.
#[test]
fn joining_players_get_their_own_avatar() {
    let mut scene = Scene::new(field());
    let guest = PlayerId(1);
    let host_avatar = scene.agent();
    assert_eq!(scene.s().avatar_of(PlayerId(0)), Some(host_avatar));
    assert_eq!(scene.s().avatar_of(guest), None);
    scene.sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    scene.sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let mine = scene.s().avatar_of(guest).expect("the guest has an avatar");
    assert_ne!(mine, host_avatar);
    assert_eq!(scene.s().agents.iter().filter(|a| a.controlled).count(), 2);

    let start = scene.s().agents[mine as usize].position;
    let host_start = scene.avatar().position;
    let walk = |agent| CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: v(1.0, 0.0), run: false } };
    // The guest's command for the host's avatar is ignored.
    scene.sim.step(&[Command { player: guest, kind: walk(mine) }, Command { player: guest, kind: walk(host_avatar) }]);
    scene.wait(ms(1000));
    assert!(scene.s().agents[mine as usize].position.x > start.x + Fx::ONE, "the guest walked");
    assert_eq!(scene.avatar().position, host_start, "the host's avatar stayed put");
}

/// Scenes without weapons only offer the sword.
#[test]
fn unavailable_weapons_cannot_be_equipped() {
    let mut scene = Scene::new(field());
    let agent = scene.agent();
    scene.command(CommandKind::Equip { agent, weapon: content::scenario::Weapon::Spear });
    assert_eq!(scene.avatar().avatar.weapon, content::scenario::Weapon::Sword);
}

/// Players name their own avatar; names are trimmed, lose control
/// characters, stop at `NAME_CHARS`, and survive a respawn.
#[test]
fn players_name_their_own_avatar() {
    let mut scene = Scene::new(field());
    let guest = PlayerId(1);
    scene.sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let name = |name: &str| CommandKind::Name { name: name.into() };
    scene.sim.step(&[
        Command { player: PlayerId(0), kind: name("  Ada\n ") },
        Command { player: guest, kind: name("a very long name that keeps going") },
    ]);
    let s = scene.s();
    assert_eq!(s.agents[s.avatar_of(PlayerId(0)).unwrap() as usize].name, "Ada");
    let guest_name = &s.agents[s.avatar_of(guest).unwrap() as usize].name;
    assert_eq!(guest_name.chars().count(), super::avatar::NAME_CHARS);
    let agent = scene.agent();
    scene.command(CommandKind::Respawn { agent });
    assert_eq!(scene.avatar().name, "Ada");
}

/// The field with diggable ground.
fn dig_lab() -> SceneSetup {
    SceneSetup { diggable: true, ..field() }
}

/// Digging takes the avatar's tile down a layer after two seconds standing
/// still, three layers at most; walking off stops it; on foot there's no
/// way out of a pit, and the jetpack lifts the avatar out.
#[test]
fn avatars_dig_pits_and_fly_out() {
    use super::avatar::DIG_TICKS;
    use crate::terrain::{Terrain, LAYER};
    let mut scene = Scene::new(field());
    let agent = scene.agent();
    scene.command(CommandKind::DigDown { agent });
    assert_eq!(scene.avatar().avatar.dig, None, "the field isn't diggable");

    let mut scene = Scene::new(dig_lab());
    scene.walk(0.3, 1.8);
    let tile = Terrain::pit_tile(scene.avatar().position);
    let pit = |scene: &Scene| scene.sim.world().terrain().pit(tile);
    let walk = |direction| CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction, run: false } };

    // Walking off before it's done leaves the ground as it was.
    scene.command(CommandKind::DigDown { agent });
    scene.wait(ms(667));
    scene.command(walk(v(0.0, 1.0)));
    scene.command(walk(FxVec2::ZERO));
    assert_eq!(scene.avatar().avatar.dig, None);
    scene.wait(DIG_TICKS);
    assert_eq!(pit(&scene), 0);
    scene.walk(0.3, 1.8);

    for depth in 1..=3 {
        scene.command(CommandKind::DigDown { agent });
        // Standing still: a stop or an attack doesn't interrupt.
        scene.command(walk(FxVec2::ZERO));
        let weapon = content::scenario::Weapon::Sword;
        scene.command(CommandKind::Attack { agent, weapon, aim: v(1.0, 0.0), muzzle: [Fx::ZERO; 3] });
        scene.wait(DIG_TICKS - 4);
        assert_eq!(pit(&scene), depth - 1, "still digging");
        scene.wait(ms(33));
        assert_eq!(pit(&scene), depth);
        assert_eq!(scene.avatar().position, Terrain::pit_center(tile), "dug in the middle of the tile");
        scene.wait(ms(1000));
        assert_eq!(scene.altitude(), -LAYER.mul_int(depth as i64), "dropped into the pit");
        assert!(!scene.avatar().avatar.flight.airborne);
    }
    scene.command(CommandKind::DigDown { agent });
    assert_eq!(scene.avatar().avatar.dig, None, "three layers is as deep as it goes");

    // On foot, the walls are too high to climb.
    scene.command(walk(v(1.0, 0.0)));
    scene.wait(ms(1000));
    assert_eq!(Terrain::pit_tile(scene.avatar().position), tile, "still in the pit");
    // The jetpack lifts the avatar out, and it lands on the ground beside.
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.wait(ms(1500));
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.command(walk(FxVec2::ZERO));
    scene.wait(ms(1500));
    assert_ne!(Terrain::pit_tile(scene.avatar().position), tile, "out of the pit");
    assert_eq!(scene.altitude(), Fx::ZERO);
    assert!(!scene.avatar().avatar.flight.airborne);
}

/// The dig lab's mine: 256 x 256 m of generated ground with a collector by
/// the spawn.
fn mine() -> SceneSetup {
    let collector = Some([fx(3.5), fx(-2.5)]);
    SceneSetup {
        extent: Fx::from_int(128),
        diggable: true,
        mine: Some(7),
        piles: Some(PileSetup { capacity: 12, dispensers: Vec::new(), piles: Vec::new(), collector }),
        ..field()
    }
}

/// Digging an ore layer throws its ore onto a pile beside the pit, within
/// reach; the avatar collects it, flies out, and delivers it to the
/// collector, and every unit is accounted for.
#[test]
fn dug_ore_piles_up_and_goes_to_the_collector() {
    use super::avatar::DIG_TICKS;
    use crate::terrain::Terrain;
    let mut scene = Scene::new(mine());
    let agent = scene.agent();
    let terrain = scene.sim.world().terrain();
    let mine = terrain.mine().expect("a mine");
    assert_eq!(terrain.height(scene.at(0.0, 2.0)), Fx::ZERO, "the spawn is on the landing");
    // The nearest ore at the surface of the landing, clear of the collector.
    let lab = scene.s().piles.as_ref().unwrap();
    let tile = (-6..=6)
        .flat_map(|y| (-6..=6).map(move |x| (x, y)))
        .map(|(x, y)| Terrain::pit_tile(scene.at(x as f64, y as f64)))
        .filter(|&t| mine.ore(t, 0).is_some() && !lab.blocks_dig(t))
        .min_by_key(|&t| {
            let d = Terrain::pit_center(t) - scene.at(0.0, 2.0);
            (d.length_squared(), t)
        })
        .expect("ore near the spawn");
    let (material, units) = (mine.ore(tile, 0).unwrap(), mine.yield_of(tile, 0));
    let spot = Terrain::pit_center(tile) - scene.center;
    scene.walk(spot.x.to_f64(), spot.y.to_f64());
    scene.command(CommandKind::DigDown { agent });
    scene.wait(DIG_TICKS);
    let lab = scene.s().piles.as_ref().unwrap();
    let (&pile, p) = lab.piles.iter().next().expect("the ore landed in a pile");
    assert_eq!((p.material, p.units), (material, units));
    assert!(crate::nav::distance(p.position, Terrain::pit_center(tile)) < fx(3.0), "beside the pit");
    assert!(lab.conserved());

    let pile_action = |action| CommandKind::Pile { agent, action, pace: Fx::ZERO, approach: None };
    let haul = |scene: &mut Scene, action| {
        scene.command(pile_action(action));
        for _ in 0..ms(30_000) {
            if scene.hauler().job.is_none() {
                return;
            }
            scene.sim.step(&[]);
        }
        panic!("{action:?} never finished");
    };
    haul(&mut scene, PileAction::Pickup(pile));
    assert_eq!(scene.hauler().last_error, None, "the pile is in reach from the pit");
    assert_eq!(scene.hauler().cargo.units, units);
    // There's no walking to the collector from the pit: fly out first.
    scene.command(pile_action(PileAction::Deliver));
    assert!(scene.hauler().last_error.is_some(), "no route out of the pit on foot");
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: v(0.0, 1.0), run: false } });
    scene.wait(ms(667));
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::ZERO, run: false } });
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.wait(ms(1333));
    assert!(!scene.avatar().avatar.flight.airborne);
    haul(&mut scene, PileAction::Deliver);
    let lab = scene.s().piles.as_ref().unwrap();
    assert_eq!(scene.hauler().last_error, None);
    assert_eq!(scene.hauler().cargo.units, 0);
    assert_eq!(lab.collector.as_ref().unwrap().stored[material as usize], units);
    assert!(lab.conserved());
}

/// Routes across the big mine stay quick to plan: a long walk over the
/// terraces arrives.
#[test]
fn routes_cross_the_mine() {
    let mut scene = Scene::new(mine());
    let started = std::time::Instant::now();
    scene.walk(30.0, 25.0);
    assert!(crate::nav::distance(scene.avatar().position, scene.at(30.0, 25.0)) < fx(0.1));
    assert!(started.elapsed().as_secs() < 20, "took {:?}", started.elapsed());
}

/// Two players haul at once in the mine: each collects a different ore into
/// their own carrier and delivers it, and the collector counts both.
#[test]
fn two_players_haul_their_own_loads() {
    let p = |x: f64, y: f64| [fx(x), fx(y)];
    let mut setup = mine();
    setup.piles.as_mut().unwrap().piles = vec![(0, p(-3.0, 4.0), 10), (1, p(3.0, 5.0), 10)];
    let mut scene = Scene::new(setup);
    let (host, guest) = (PlayerId(0), PlayerId(1));
    scene.sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let (a, b) = (scene.s().avatar_of(host).unwrap(), scene.s().avatar_of(guest).unwrap());
    let haul = |agent, action| CommandKind::Pile { agent, action, pace: fx(0.2), approach: None };
    let idle = |scene: &Scene, agent| scene.lab().hauler(agent).is_some_and(|h| h.job.is_none());
    let both = |scene: &mut Scene, first: CommandKind, second: CommandKind| {
        scene.sim.step(&[Command { player: host, kind: first }, Command { player: guest, kind: second }]);
        for _ in 0..ms(30_000) {
            if idle(scene, a) && idle(scene, b) {
                return;
            }
            scene.sim.step(&[]);
        }
        panic!("the hauls never finished");
    };
    both(&mut scene, haul(a, PileAction::Pickup(0)), haul(b, PileAction::Pickup(1)));
    let lab = scene.lab();
    assert_eq!((lab.cargo(a).material, lab.cargo(a).units), (Some(0), 10));
    assert_eq!((lab.cargo(b).material, lab.cargo(b).units), (Some(1), 10));
    assert!(lab.piles.is_empty());
    both(&mut scene, haul(a, PileAction::Deliver), haul(b, PileAction::Deliver));
    let lab = scene.lab();
    assert_eq!(lab.hauler(a).unwrap().last_error, None);
    assert_eq!(lab.hauler(b).unwrap().last_error, None);
    assert_eq!(lab.collector.as_ref().unwrap().stored, vec![10, 10, 0]);
    assert!(lab.conserved());
}

// Releasing direct control (pause, focus loss, a controller disconnecting).

fn release(scene: &mut Scene) {
    let agent = scene.agent();
    scene.command(CommandKind::ReleaseDirectControl { agent });
}

#[test]
fn releasing_stops_walking_and_switches_the_jetpack_off() {
    let mut scene = Scene::new(jetpack_lab());
    let agent = scene.agent();
    scene.wait(ms(500));
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, -Fx::ONE), run: true } });
    scene.wait(ms(300));
    assert!(scene.avatar().avatar.flight.requested);
    release(&mut scene);
    assert!(!scene.avatar().avatar.flight.requested, "thrust is off");
    assert_eq!(scene.avatar().avatar.intent, Some(Intent::Stop));
    scene.wait(ms(1000));
    let stopped = scene.avatar().position;
    scene.wait(ms(500));
    assert_eq!(scene.avatar().position, stopped, "no drift after release");
}

#[test]
fn releasing_stops_a_click_route() {
    let mut scene = Scene::new(pile_lab());
    let (agent, target) = (scene.agent(), scene.at(3.0, -1.0));
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: false } });
    scene.wait(ms(200));
    assert!(!scene.avatar().route.is_empty());
    release(&mut scene);
    assert!(scene.avatar().route.is_empty());
    assert_eq!(scene.avatar().avatar.intent, Some(Intent::Stop));
}

#[test]
fn releasing_keeps_an_accepted_haul_and_its_route() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None });
    scene.wait(ms(200));
    let route = scene.avatar().route.clone();
    assert!(scene.hauler().job.is_some() && !route.is_empty());
    release(&mut scene);
    assert!(scene.hauler().job.is_some(), "the haul continues");
    assert_eq!(scene.hauler().last_error, None);
    assert_eq!(scene.avatar().route, route, "its route is untouched");
    for _ in 0..ms(30_000) {
        if scene.hauler().job.is_none() {
            break;
        }
        scene.sim.step(&[]);
    }
    assert!(scene.hauler().job.is_none() && scene.hauler().last_error.is_none(), "the haul finished normally");
    assert!(scene.hauler().cargo.units > 0);
    assert!(scene.lab().conserved());
}

/// A haul and a release in one turn: the release sees the haul it follows.
#[test]
fn release_judges_a_haul_accepted_earlier_in_the_same_turn() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    let haul = CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None };
    let commands = [haul, CommandKind::ReleaseDirectControl { agent }].map(|kind| Command { player: PlayerId(0), kind });
    scene.sim.step(&commands);
    assert!(scene.hauler().job.is_some());
    assert!(!scene.avatar().route.is_empty(), "the accepted haul owns the route");
}

/// A haul the simulation rejected owns nothing: the release stops the walk.
#[test]
fn release_after_a_rejected_haul_stops_manual_travel() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: false } });
    // A drop with nothing carried is refused.
    let refused = CommandKind::Pile { agent, action: PileAction::Deliver, pace: Fx::ZERO, approach: None };
    let commands = [refused, CommandKind::ReleaseDirectControl { agent }].map(|kind| Command { player: PlayerId(0), kind });
    scene.sim.step(&commands);
    assert!(scene.hauler().job.is_none() && scene.hauler().last_error.is_some());
    assert_eq!(scene.avatar().avatar.intent, Some(Intent::Stop));
}

/// Units already tossed land under the usual rules while the haul goes on.
#[test]
fn releasing_mid_transfer_conserves_airborne_units() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: fx(0.2), approach: None });
    let mut tossing = false;
    for _ in 0..ms(30_000) {
        scene.sim.step(&[]);
        if !scene.hauler().in_flight.is_empty() {
            tossing = true;
            break;
        }
    }
    assert!(tossing, "the haul never started tossing");
    release(&mut scene);
    assert!(scene.hauler().job.is_some());
    assert!(scene.lab().conserved());
    scene.wait(ms(10_000));
    assert!(scene.hauler().job.is_none() && scene.hauler().in_flight.is_empty());
    assert!(scene.lab().conserved());
}

/// Explicit cancellation stays a separate, deliberate action.
#[test]
fn deliberate_movement_after_release_still_cancels_the_haul() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None });
    release(&mut scene);
    assert!(scene.hauler().job.is_some());
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: false } });
    assert!(scene.hauler().job.is_none());
    assert_eq!(scene.hauler().last_error.as_deref(), Some("Haul canceled by player input"));
}

/// Only the avatar's owner may release it.
#[test]
fn release_is_refused_for_another_players_avatar() {
    let mut scene = Scene::new(jetpack_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.sim.step(&[Command { player: PlayerId(7), kind: CommandKind::ReleaseDirectControl { agent } }]);
    assert!(scene.avatar().avatar.flight.requested);
}

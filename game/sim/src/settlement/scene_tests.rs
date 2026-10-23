//! Scenes without a supply chain: jetpack flight, the memory trial and the
//! pile lab, following the original game's demo scripts (`game/app/demos`).

use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::piles::PileAction;
use super::{PileSetup, SceneSetup, Settlement};
use crate::terrain::{Disc, Surface};
use crate::{Command, CommandKind, PlayerId, Simulation, World};

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
        let world = World::new_scene(1, Content::builtin().rules, &setup).unwrap();
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
        for _ in 0..900 {
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

    fn altitude(&self) -> Fx {
        self.avatar().avatar.height
    }
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
        ..SceneSetup::default()
    }
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

/// `demos/jetpack.json`: climb onto the 2 m platform, cross the drop, hover
/// over it with the jetpack released, and land on the 3.5 m platform.
#[test]
fn jetpack_climbs_crosses_a_drop_hovers_and_lands() {
    let mut scene = Scene::new(jetpack_lab());
    let agent = scene.agent();
    let close = |a: Fx, b: f64| (a - fx(b)).abs() <= fx(0.05);
    scene.wait(30);
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.wait(30);
    scene.walk(0.0, 2.0);
    scene.wait(30);
    assert!(scene.avatar().avatar.flight.airborne && close(scene.altitude(), 3.8), "hovering at {}", scene.altitude());
    scene.walk(2.0, 2.0);
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.wait(30);
    assert!(scene.avatar().avatar.flight.airborne && close(scene.altitude(), 3.8), "kept aloft over the drop at {}", scene.altitude());
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.walk(5.0, 1.0);
    scene.wait(30);
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.wait(40);
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
        ..SceneSetup::default()
    }
}

/// `demos/surprise.json`: record a walk to the first pad, let the ghost
/// replay it while the player stands on the second, and collect the core.
#[test]
fn memory_trial_needs_a_ghost_to_open_the_door() {
    let mut scene = Scene::new(surprise_lab());
    let memory = |scene: &Scene| scene.s().memory.clone().unwrap();
    scene.wait(40);
    assert!(!memory(&scene).unlocked);
    scene.command(CommandKind::MemoryRecord);
    scene.walk(-4.0, -2.0);
    scene.wait(20);
    scene.command(CommandKind::MemoryDeploy);
    assert_eq!(scene.avatar().position, scene.at(-5.0, 3.0), "deploying returns the player to the start");
    scene.wait(20);
    scene.walk(-1.0, 3.0);
    scene.wait(100);
    let m = memory(&scene);
    assert!(m.unlocked && !m.collected, "{:?}", (m.pressed, m.charge));
    scene.walk(5.0, 0.0);
    scene.wait(40);
    assert!(memory(&scene).collected);
}

#[test]
fn the_trial_cannot_be_completed_alone() {
    let mut scene = Scene::new(surprise_lab());
    scene.walk(-4.0, -2.0);
    scene.walk(-1.0, 3.0);
    scene.wait(60);
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
    scene.wait(260);
    let m = scene.s().memory.clone().unwrap();
    assert!(!m.recording && m.ghost.is_some());
    assert_eq!(m.elapsed, super::memory::LIMIT_TICKS);
    scene.command(CommandKind::MemoryClear);
    assert!(scene.s().memory.as_ref().unwrap().ghost.is_none());
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
        }),
        ..SceneSetup::default()
    }
}

/// `demos/haul_four.json`: walk to a dispenser's pile, take exactly four
/// units one at a time, carry them across, and start a new pile.
#[test]
fn pile_lab_hauls_exactly_four_units() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    let lab = |scene: &Scene| scene.s().piles.clone().unwrap();
    let finish = |scene: &mut Scene| {
        for _ in 0..900 {
            if lab(scene).job.is_none() {
                return;
            }
            scene.sim.step(&[]);
        }
        panic!("the haul never finished");
    };
    scene.command(CommandKind::Dispense { dispenser: 0, units: 8 });
    scene.wait(30);
    scene.walk(-1.7, -3.4);
    let approach = Some(scene.at(-2.3, -2.8));
    scene.command(CommandKind::Pile { agent, action: PileAction::PickupAmount(0, 4), pace: fx(0.7), approach });
    let start = scene.avatar().position;
    finish(&mut scene);
    let l = lab(&scene);
    assert_eq!(l.last_error, None);
    assert_eq!(l.cargo.units, 4);
    assert_eq!(l.total(), 104);
    assert_eq!(l.piles[&0].units, 4);
    assert_ne!(scene.avatar().position, start, "walked before transferring");
    scene.wait(30);
    scene.walk(2.0, 1.0);
    let (drop, approach) = (scene.at(4.0, 4.5), Some(scene.at(2.9, 5.0)));
    scene.command(CommandKind::Pile { agent, action: PileAction::Drop(drop), pace: fx(0.7), approach });
    finish(&mut scene);
    let l = lab(&scene);
    assert_eq!(l.last_error, None);
    assert_eq!(l.cargo.units, 0);
    assert_eq!(l.total(), 104);
    assert_eq!(l.piles[&5].units, 4);
    assert!(l.conserved());
}

#[test]
fn moving_cancels_a_haul_and_keeps_the_cargo() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None });
    assert!(scene.s().piles.as_ref().unwrap().job.is_some());
    scene.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: false } });
    let l = scene.s().piles.clone().unwrap();
    assert!(l.job.is_none());
    assert_eq!(l.last_error.as_deref(), Some("Haul canceled by player input"));
    assert!(l.conserved());
}

#[test]
fn blocked_drops_are_rejected_without_losing_cargo() {
    let mut scene = Scene::new(pile_lab());
    let agent = scene.agent();
    scene.command(CommandKind::Pile { agent, action: PileAction::Pickup(2), pace: Fx::ZERO, approach: None });
    for _ in 0..600 {
        if scene.s().piles.as_ref().unwrap().job.is_none() {
            break;
        }
        scene.sim.step(&[]);
    }
    let carried = scene.s().piles.as_ref().unwrap().cargo.units;
    assert_eq!(carried, 8);
    // Off the deck.
    let edge = scene.at(7.5, 7.5);
    scene.command(CommandKind::Pile { agent, action: PileAction::Drop(edge), pace: Fx::ZERO, approach: None });
    let l = scene.s().piles.clone().unwrap();
    assert!(l.job.is_none() && l.last_error.is_some());
    assert_eq!(l.cargo.units, carried);
    assert!(l.conserved());
}

/// Scenes save and continue like any world: piles, ghosts and flight included.
#[test]
fn scenes_are_deterministic_across_thread_counts() {
    let run = |threads| {
        let world = World::new_scene(4, Content::builtin().rules, &surprise_lab()).unwrap();
        let mut sim = Simulation::from_world(world, threads);
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let mut hashes = Vec::new();
        hashes.push(sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MemoryRecord }]).total);
        let target = sim.world().terrain().center + v(-4.0, -2.0);
        hashes.push(sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: true } } }]).total);
        hashes.extend((0..120).map(|_| sim.step(&[]).total));
        hashes.push(sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MemoryDeploy }]).total);
        hashes.extend((0..120).map(|_| sim.step(&[]).total));
        hashes
    };
    assert_eq!(run(1), run(8));
}

/// A flat field with nothing on it, like the app's `field` scene.
fn field() -> SceneSetup {
    SceneSetup { extent: Fx::from_int(8), spawn: [Fx::ZERO, Fx::ZERO, fx(2.0)], ..SceneSetup::default() }
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
    scene.wait(30);
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
    scene.wait(20);
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
        scene.wait(1);
        assert_eq!(pit(&scene), depth);
        assert_eq!(scene.avatar().position, Terrain::pit_center(tile), "dug in the middle of the tile");
        scene.wait(30);
        assert_eq!(scene.altitude(), -LAYER.mul_int(depth as i64), "dropped into the pit");
        assert!(!scene.avatar().avatar.flight.airborne);
    }
    scene.command(CommandKind::DigDown { agent });
    assert_eq!(scene.avatar().avatar.dig, None, "three layers is as deep as it goes");

    // On foot, the walls are too high to climb.
    scene.command(walk(v(1.0, 0.0)));
    scene.wait(30);
    assert_eq!(Terrain::pit_tile(scene.avatar().position), tile, "still in the pit");
    // The jetpack lifts the avatar out, and it lands on the ground beside.
    scene.command(CommandKind::Jetpack { agent, enabled: true });
    scene.wait(45);
    scene.command(CommandKind::Jetpack { agent, enabled: false });
    scene.command(walk(FxVec2::ZERO));
    scene.wait(45);
    assert_ne!(Terrain::pit_tile(scene.avatar().position), tile, "out of the pit");
    assert_eq!(scene.altitude(), Fx::ZERO);
    assert!(!scene.avatar().avatar.flight.airborne);
}

//! The crawler's gun's ammo (TU.3 of `kestrel/CRAWLER_TURRET.md`, T6): an
//! elder carrying stone interacts beside the stopped crawler and delivers
//! as much as fits in the gun's store (20), the rest staying in the
//! bundle; a walking crawler takes none; nothing comes back out; the
//! balances hold through deliveries and shots; saves, hashes and
//! prediction hold with a delivery under way. And the gun, 4.8 m up,
//! shoots threats on any floor in sight below it, not only its own.

use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::crawler::{self, Pose};
use super::crawler_lab_tests::{board, copy, crawler_lab_with_guests, owner, seconds, SOUTH};
use super::gun;
use super::interact::{InteractTarget, Interaction};
use super::vehicle::VehicleState;
use super::{AgentIx, Direction, Event, InventoryRef, ItemIx, Settlement, Threat};
use crate::nav::heading_vector;
use crate::{Command, CommandKind, Simulation, World};

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn stone_ix(s: &Settlement) -> ItemIx {
    s.item_index("stone").unwrap()
}

/// The stone in the gun's store.
fn store(s: &Settlement) -> u32 {
    s.vehicles[0].gun.as_ref().unwrap().store.count(stone_ix(s))
}

/// The stone in `agent`'s bundle.
fn carried(s: &Settlement, agent: AgentIx) -> u32 {
    s.agents[agent as usize].carry.inventory.count(stone_ix(s))
}

/// The lab with the pilot (`agents[0]`) in the cab, its waves and ghouls
/// gone; the deliverer (`agents[1]`) stands on the ground west of the box.
fn lab() -> (Simulation, Vec<u16>) {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0]);
    let s = sim.world_mut().settlement_mut();
    s.waves.iter_mut().for_each(|w| w.count = w.spawned);
    s.threats.clear();
    (sim, agents)
}

/// Takes `n` stones out of the gun's store, as spent elsewhere.
fn empty(sim: &mut Simulation, n: u32) {
    let s = sim.world_mut().settlement_mut();
    let stone = stone_ix(s);
    s.vehicles[0].gun.as_mut().unwrap().store.remove(stone, n);
    s.accounts[stone as usize].consumed += n as u64;
    s.validate().unwrap();
}

/// Moves `n` stones from the lab's stone pile into `agent`'s bundle, as a
/// withdrawal would: the balances still hold.
fn hand_stone(sim: &mut Simulation, agent: AgentIx, n: u32) {
    let s = sim.world_mut().settlement_mut();
    let stone = stone_ix(s);
    let pile = s.depots.iter().position(|d| d.id == "lab_stone").unwrap();
    s.depots[pile].inventory.remove(stone, n);
    assert!(s.agents[agent as usize].carry.inventory.add(stone, n), "the bundle holds {n}");
    s.validate().unwrap();
}

/// Puts `agent` `gap` meters west of the crawler's box, level with its
/// middle, standing.
fn beside(sim: &mut Simulation, agent: AgentIx, gap: f64) {
    let s = sim.world_mut().settlement_mut();
    let v = &s.vehicles[0];
    let c = s.characters[v.character as usize].crawler.clone().unwrap();
    let pose = Pose::of(v);
    let edge = crawler::box_nearest(pose, &c, v.position - FxVec2::new(Fx::from_int(10), Fx::ZERO));
    let a = &mut s.agents[agent as usize];
    a.body.position = edge - FxVec2::new(fx(gap), Fx::ZERO);
    a.body.velocity = FxVec2::ZERO;
    a.body.route.clear();
}

fn fx(v: f64) -> Fx {
    Fx::from_ratio((v * 1000.0).round() as i64, 1000)
}

/// `agent`'s interact target now, as the app reads it.
fn target(sim: &Simulation, agent: AgentIx) -> Option<(InteractTarget, Interaction)> {
    let world = sim.world();
    settlement(sim).interact_target(agent, world.tick(), world.terrain())
}

fn interact(agents: &[u16], i: usize, target: InteractTarget) -> Command {
    Command { player: owner(i), kind: CommandKind::Interact { agent: agents[i], target } }
}

fn south(agents: &[u16]) -> Command {
    Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Walk { direction: heading_vector(SOUTH), run: false } } }
}

fn refusals(s: &Settlement, agent: AgentIx) -> Vec<BundleRefusal> {
    s.events
        .iter()
        .filter_map(|e| match *e {
            Event::BundleRefused { agent: a, reason } if a == agent => Some(reason),
            _ => None,
        })
        .collect()
}

/// Steps until `agent`'s job is done and nothing flies, `validate` every
/// tick; returns the ticks it took.
fn settle(sim: &mut Simulation, agent: AgentIx, most: u64) -> u64 {
    for t in 0..most {
        let s = settlement(sim);
        if s.agents[agent as usize].carry.job.is_none() && s.transfers.is_empty() {
            return t;
        }
        sim.step(&[]);
        settlement(sim).validate().unwrap();
    }
    panic!("the delivery didn't finish in {most} ticks");
}

/// A stopped crawler takes as much stone as fits, the rest staying in the
/// bundle: an elder a step off the box (in interact reach, out of the
/// boarding reach) walks in and delivers 3 of its 5 to a store of 17; a full
/// store is then no target, and nothing comes back out, not even to an
/// empty hand.
#[test]
fn a_stopped_crawler_takes_stone_up_to_its_cap() {
    let (mut sim, agents) = lab();
    let elder = agents[1];
    empty(&mut sim, 3);
    hand_stone(&mut sim, elder, 5);
    beside(&mut sim, elder, 1.3);
    let (t, use_) = target(&sim, elder).expect("the gun is in reach");
    assert_eq!(t, InteractTarget::Gun(0));
    let Interaction::Bundle(job) = use_ else { panic!("not a delivery: {use_:?}") };
    assert_eq!((job.other, job.direction, job.item, job.remaining), (InventoryRef::Gun(0), Direction::Deposit, stone_ix(settlement(&sim)), 3));
    let start = settlement(&sim).agents[elder as usize].body.position;
    sim.step(&[interact(&agents, 1, t)]);
    assert!(refusals(settlement(&sim), elder).is_empty());
    settle(&mut sim, elder, seconds(10.0));
    let s = settlement(&sim);
    assert_eq!((store(s), carried(s, elder)), (20, 2), "3 in, 2 left in the bundle");
    let moved = (s.agents[elder as usize].body.position - start).length();
    assert!(moved > fx(0.2), "it stepped in to the box ({moved:?})");
    let v = &s.vehicles[0];
    let c = s.characters[v.character as usize].crawler.as_ref().unwrap();
    let reach = s.agents[elder as usize].radius() + fx(0.8);
    assert!(crawler::box_distance_squared(Pose::of(v), c, s.agents[elder as usize].body.position) <= reach * reach, "and delivered from beside it");

    // Full: no target with stone in hand, nor empty-handed; a bundle action
    // can't take any out.
    assert!(target(&sim, elder).is_none_or(|(t, _)| t != InteractTarget::Gun(0)));
    let world = sim.world();
    assert_eq!(settlement(&sim).bundle_job(elder, InventoryRef::Gun(0), Direction::Deposit, world.tick(), world.terrain()), Err(BundleRefusal::PileFull));
    let other = agents[2];
    beside(&mut sim, other, 0.3);
    assert!(target(&sim, other).is_none_or(|(t, _)| t != InteractTarget::Gun(0)), "empty-handed, nothing to take out");
    let world = sim.world();
    assert_eq!(settlement(&sim).bundle_job(other, InventoryRef::Gun(0), Direction::Pickup, world.tick(), world.terrain()), Err(BundleRefusal::DepositOnly));
    sim.step(&[Command { player: owner(2), kind: CommandKind::Bundle { agent: other, other: InventoryRef::Gun(0), direction: Direction::Pickup } }]);
    assert_eq!(refusals(settlement(&sim), other), [BundleRefusal::DepositOnly]);
    assert_eq!(store(settlement(&sim)), 20);

    // An interact names the gun from an elder seated aboard: nothing.
    board(&mut sim, &agents, &[3]);
    assert!(settlement(&sim).seat(agents[3] as usize).is_some());
    assert!(target(&sim, agents[3]).is_none(), "aboard, interact offers nothing");
}

/// A walking crawler takes none: the target says to wait and the action
/// is refused as `Walking`. A delivery started while it stood still stops
/// moving units once it walks, and goes on when it stops beside the elder
/// again; nothing is lost on the way.
#[test]
fn a_walking_crawler_takes_none() {
    let (mut sim, agents) = lab();
    let elder = agents[1];
    empty(&mut sim, 10);
    hand_stone(&mut sim, elder, 5);
    beside(&mut sim, elder, 0.3);
    sim.step(&[south(&agents)]);
    sim.step(&[south(&agents)]);
    assert!(crawler::walking(&settlement(&sim).vehicles[0]));
    assert_eq!(target(&sim, elder), Some((InteractTarget::Gun(0), Interaction::Walking { vehicle: 0 })));
    sim.step(&[south(&agents), interact(&agents, 1, InteractTarget::Gun(0))]);
    let s = settlement(&sim);
    assert_eq!(refusals(s, elder), [BundleRefusal::Walking]);
    assert!(s.agents[elder as usize].carry.job.is_none());
    let world = sim.world();
    assert_eq!(settlement(&sim).bundle_job(elder, InventoryRef::Gun(0), Direction::Deposit, world.tick(), world.terrain()), Err(BundleRefusal::Walking));
    assert_eq!((store(settlement(&sim)), carried(settlement(&sim), elder)), (10, 5));

    // Stopped: the pilot lets go and the feet settle.
    let stop = Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Stop } };
    sim.step(&[stop]);
    for _ in 0..seconds(5.0) {
        if !crawler::walking(&settlement(&sim).vehicles[0]) {
            break;
        }
        sim.step(&[]);
    }
    assert!(!crawler::walking(&settlement(&sim).vehicles[0]), "it stopped");
    beside(&mut sim, elder, 0.3);
    let (t, use_) = target(&sim, elder).unwrap();
    assert!(matches!((t, &use_), (InteractTarget::Gun(0), Interaction::Bundle(job)) if job.remaining == 5), "{use_:?}");
    sim.step(&[interact(&agents, 1, t)]);
    // Two units in, then it walks off: no more go while it walks.
    let arrived = |s: &Settlement| store(s) + s.transfers.values().filter(|t| t.to == InventoryRef::Gun(0)).count() as u32;
    for _ in 0..seconds(5.0) {
        if arrived(settlement(&sim)) >= 12 {
            break;
        }
        sim.step(&[]);
    }
    assert_eq!(arrived(settlement(&sim)), 12);
    sim.step(&[south(&agents)]);
    let mut walked = 0;
    for _ in 0..seconds(3.0) {
        sim.step(&[south(&agents)]);
        let s = settlement(&sim);
        s.validate().unwrap();
        if crawler::walking(&s.vehicles[0]) {
            walked += 1;
            assert_eq!(arrived(s), 12, "a walking crawler takes none");
        }
    }
    assert!(walked > seconds(2.0), "it walked");
    // Stopped again, the elder beside it: the rest goes in.
    sim.step(&[Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Stop } }]);
    for _ in 0..seconds(5.0) {
        if !crawler::walking(&settlement(&sim).vehicles[0]) {
            break;
        }
        sim.step(&[]);
    }
    beside(&mut sim, elder, 0.3);
    settle(&mut sim, elder, seconds(10.0));
    let s = settlement(&sim);
    assert_eq!((store(s), carried(s, elder)), (15, 0));
}

/// The balances hold through deliveries and shots: a sprite at the gun
/// shoots ghouls, an elder restocks it, and every stone is where the
/// accounts say, `validate` every tick; each bolt spent one.
#[test]
fn conservation_holds_through_deliveries_and_shots() {
    let (mut sim, agents) = lab();
    let elder = agents[1];
    let sprite = settlement(&sim).agents.iter().position(|a| a.id == "sprite_ash").unwrap() as AgentIx;
    assert!(sim.world_mut().settlement_mut().man_gun(sprite, 0));
    for (i, x) in [-4.0, 4.0, 0.0].into_iter().enumerate() {
        ghoul(&mut sim, 9001 + i as u64, (x, 6.0), Fx::ZERO, 3);
    }
    let spent0 = settlement(&sim).accounts[stone_ix(settlement(&sim)) as usize].spent;
    let mut shots = 0;
    for _ in 0..seconds(4.0) {
        sim.step(&[]);
        let s = settlement(&sim);
        s.validate().unwrap();
        shots += s.events.iter().filter(|e| matches!(e, Event::GunShot { vehicle: 0 })).count() as u64;
    }
    assert!(shots >= 3, "it shot ({shots})");
    let s = settlement(&sim);
    assert_eq!(s.accounts[stone_ix(s) as usize].spent - spent0, shots, "a stone a bolt");
    assert_eq!(store(s) as u64, 20 - shots);
    // The ghouls gone, the gun stops spending while the elder restocks it.
    sim.world_mut().settlement_mut().threats.clear();
    let s = settlement(&sim);
    let room = 20 - store(s);
    hand_stone(&mut sim, elder, 5);
    beside(&mut sim, elder, 0.3);
    sim.step(&[interact(&agents, 1, InteractTarget::Gun(0))]);
    settle(&mut sim, elder, seconds(15.0));
    let s = settlement(&sim);
    let delivered = room.min(5);
    assert_eq!((store(s), carried(s, elder)), (20 - room + delivered, 5 - delivered));
    let b = s.balances().into_iter().find(|(item, _)| item == "stone").unwrap().1;
    assert!(b.conserved, "{b:?}");
}

/// A save mid-delivery loads and runs on alike, on one thread or four;
/// and the delivering elder's client predicts its walk in to the box.
#[test]
fn a_delivery_is_saved_hashed_and_predicted() {
    let (mut sim, agents) = lab();
    let elder = agents[1];
    empty(&mut sim, 12);
    hand_stone(&mut sim, elder, 5);
    beside(&mut sim, elder, 1.3);

    // The elder's replay walks in as the world does.
    let confirmed = copy(sim.world());
    let mut replay = confirmed.settlement().unwrap().replay_avatar(elder, confirmed.tick(), confirmed.terrain()).unwrap();
    let mut ahead = Simulation::from_world(copy(&confirmed), 1);
    for t in 0..seconds(1.5) {
        let mine: Vec<CommandKind> = if t == 0 { vec![CommandKind::Interact { agent: elder, target: InteractTarget::Gun(0) }] } else { Vec::new() };
        replay.step_with(mine.iter());
        ahead.step(&mine.into_iter().map(|kind| Command { player: owner(1), kind }).collect::<Vec<_>>());
        let (mine, theirs) = (replay.state(), &settlement(&ahead).agents[elder as usize]);
        assert_eq!(mine.body.position, theirs.body.position, "tick {t}");
    }

    // Saved with units in the air.
    sim.step(&[interact(&agents, 1, InteractTarget::Gun(0))]);
    for _ in 0..seconds(5.0) {
        if !settlement(&sim).transfers.is_empty() {
            break;
        }
        sim.step(&[]);
    }
    assert!(!settlement(&sim).transfers.is_empty(), "stone in the air");
    let world = sim.into_world();
    let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
    let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), world.rules().clone()).unwrap();
    assert_eq!(loaded.settlement().unwrap().vehicles, world.settlement().unwrap().vehicles, "the store and its reservation");
    let threaded = copy(&world);
    let (mut a, mut b, mut c) = (Simulation::from_world(world, 1), Simulation::from_world(loaded, 1), Simulation::from_world(threaded, 4));
    for _ in 0..seconds(6.0) {
        let h = a.step(&[]).total;
        assert_eq!(h, b.step(&[]).total, "the save changed it");
        assert_eq!(h, c.step(&[]).total, "four threads changed it");
        settlement(&b).validate().unwrap();
    }
    for s in [settlement(&a), settlement(&b)] {
        assert_eq!((store(s), carried(s, elder)), (13, 0));
    }
}

/// A ghoul of the lab's waves with `hp`, at `offset` from the crawler's
/// middle, its feet at `height`, standing still.
fn ghoul(sim: &mut Simulation, id: u64, offset: (f64, f64), height: Fx, hp: u32) {
    let s = sim.world_mut().settlement_mut();
    let position = s.vehicles[0].position + FxVec2::new(fx(offset.0), fx(offset.1));
    let character = s.waves[0].character;
    s.threats.push(Threat {
        id,
        wave: 0,
        character,
        position,
        heading: Fx::ZERO,
        travelled: Fx::ZERO,
        hp,
        armor: 0,
        pierced: false,
        shocked_until: 0,
        route: vec![position],
        stagger_until: u64::MAX,
        knockback: FxVec2::ZERO,
        windup: None,
        ready_at: 0,
        burn: None,
        goal: Default::default(),
        stranded: false,
        height,
        chase: Vec::new(),
        backoff: Default::default(),
    });
    s.threats.sort_by_key(|t| t.id);
    s.next_id = s.next_id.max(id + 1);
}

/// Parks the crawler afresh at scene meters `(x, z)`, facing south,
/// keeping its gun's state.
fn park(sim: &mut Simulation, x: f64, z: f64) {
    let world = sim.world_mut();
    let terrain = world.terrain().clone();
    let s = world.settlement_mut();
    let v = &s.vehicles[0];
    let def = s.characters[v.character as usize].clone();
    let mut parked = VehicleState::new(v.character, &def, terrain.center + FxVec2::new(fx(x), fx(z)), Fx::ZERO, &terrain).expect("room to park");
    parked.gun = v.gun.clone();
    s.vehicles[0] = parked;
}

/// The gun, 4.8 m up, shoots ghouls below it on other floors: from the
/// ground into the trench (2 m down, beyond the floor reach a ballista
/// keeps to) and from the plateau (1.5 m up) onto the ground below its
/// north face. A sprite mans it; each ghoul is hit.
#[test]
fn the_gun_shoots_ghouls_on_floors_below_it() {
    for (x, z, ghoul_at, floor) in [
        // West of the spawn by the trench (x -12.5..-10.5): the ghoul in it.
        (-7.5, 0.0, (-4.0, 2.0), -2.0),
        // On the plateau by its north face (z -6.5): the ghoul on the
        // ground below it.
        (22.0, -2.0, (0.0, -5.0), 0.0),
    ] {
        let (mut sim, _) = lab();
        let sprite = settlement(&sim).agents.iter().position(|a| a.id == "sprite_ash").unwrap() as AgentIx;
        // The pilot stays in the cab, holding no stick.
        park(&mut sim, x, z);
        let terrain = sim.world().terrain().clone();
        let s = settlement(&sim);
        let v = &s.vehicles[0];
        let (c, g) = gun::gun_of(&s.characters[v.character as usize]).unwrap();
        let (mount, top) = gun::mount(v, c, g);
        let target = v.position + FxVec2::new(fx(ghoul_at.0), fx(ghoul_at.1));
        assert_eq!(terrain.height(target), fx(floor), "the ghoul's floor");
        assert!(top - terrain.height(mount) > fx(3.0), "the gun stands high over its own floor");
        assert!((mount - target).length() <= g.defence.range, "in range on the ground");
        assert!(sim.world_mut().settlement_mut().man_gun(sprite, 0));
        ghoul(&mut sim, 9001, ghoul_at, fx(floor), 6);
        let mut hits = 0;
        for _ in 0..seconds(3.0) {
            sim.step(&[]);
            let s = settlement(&sim);
            s.validate().unwrap();
            hits += s.events.iter().filter(|e| matches!(e, Event::Hit { target: 9001, projectile: Some(_), .. })).count();
        }
        assert!(hits > 0, "the gun at ({x}, {z}) hit the ghoul {floor} m down there");
    }
}

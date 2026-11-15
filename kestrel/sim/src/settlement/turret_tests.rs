//! The crawler's turret shoots (TU.2 of `kestrel/CRAWLER_TURRET.md`, T5,
//! T8, T9): in the Crawler Lab, a sprite at the gun shoots the nearest
//! threat in range from the moving mount; an elder there fires at the
//! reticle, cut to 7 m, once a period, and throws the fireball on its
//! cooldown for no stone; nothing fires empty; the pilot and riders can't
//! fire; nobody aboard is hit; prediction, saves and hashes hold.

use content::scenario::Weapon;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::buildings::{BurstRefusal, FireRefusal};
use super::crawler::{self, Pose};
use super::crawler_lab_tests::{board, copy, crawler_lab_with_guests, owner, seconds, SOUTH};
use super::gun;
use super::{AgentIx, Event, Mount, Seat, Settlement, Threat};
use crate::nav::heading_vector;
use crate::{Command, CommandKind, Simulation, World};

pub(super) fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// The lab with the pilot (`agents[0]`) in the cab, its waves and ghouls
/// gone, so the tests place their own.
pub(super) fn lab() -> (Simulation, Vec<u16>) {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0]);
    let s = sim.world_mut().settlement_mut();
    s.waves.iter_mut().for_each(|w| w.count = w.spawned);
    s.threats.clear();
    (sim, agents)
}

/// The lab's first sprite (`sprite_ash`).
pub(super) fn ash(sim: &Simulation) -> AgentIx {
    settlement(sim).agents.iter().position(|a| a.id == "sprite_ash").unwrap() as AgentIx
}

/// A ghoul of the lab's waves with `hp`, at `offset` from the parked
/// crawler's middle, standing still (staggered for good) unless `walks`.
pub(super) fn ghoul(sim: &mut Simulation, id: u64, offset: (f64, f64), hp: u32, walks: bool) {
    let s = sim.world_mut().settlement_mut();
    let m = |v: f64| Fx::from_ratio((v * 1000.0).round() as i64, 1000);
    let position = s.vehicles[0].position + FxVec2::new(m(offset.0), m(offset.1));
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
        stagger_until: if walks { 0 } else { u64::MAX },
        knockback: FxVec2::ZERO,
        windup: None,
        ready_at: 0,
        burn: None,
        goal: Default::default(),
        stranded: false,
        height: Fx::ZERO,
        chase: Vec::new(),
        backoff: Default::default(),
    });
    s.threats.sort_by_key(|t| t.id);
    s.next_id = s.next_id.max(id + 1);
}

/// Where the crawler's gun is now, and how high.
fn mount(s: &Settlement) -> (FxVec2, Fx) {
    let v = &s.vehicles[0];
    let (c, g) = gun::gun_of(&s.characters[v.character as usize]).unwrap();
    gun::mount(v, c, g)
}

/// The gun's range, period and the elder's fireball cooldown, in ticks.
fn numbers(s: &Settlement) -> (Fx, u64, u64) {
    let v = &s.vehicles[0];
    let (_, g) = gun::gun_of(&s.characters[v.character as usize]).unwrap();
    let ticks = content::scenario::ticks;
    (g.defence.range, ticks(g.defence.period), ticks(g.defence.fireball.as_ref().unwrap().cooldown))
}

fn stone(s: &Settlement) -> u32 {
    s.vehicles[0].gun.as_ref().unwrap().store.count(s.item_index("stone").unwrap())
}

fn spent(s: &Settlement) -> u64 {
    s.accounts[s.item_index("stone").unwrap() as usize].spent
}

pub(super) fn shots(s: &Settlement) -> usize {
    s.events.iter().filter(|e| matches!(e, Event::GunShot { vehicle: 0 })).count()
}

/// The pilot holds the stick south.
pub(super) fn south(agents: &[u16]) -> Command {
    Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Walk { direction: heading_vector(SOUTH), run: false } } }
}

fn aim(agents: &[u16], i: usize, offset: FxVec2) -> Command {
    Command { player: owner(i), kind: CommandKind::AimAvatar { agent: agents[i], aim: offset } }
}

/// The fire key: an `Attack` at `offset` (from the gun's mount, for a
/// gunner).
fn fire(agents: &[u16], i: usize, offset: FxVec2) -> Command {
    Command { player: owner(i), kind: CommandKind::Attack { agent: agents[i], weapon: Weapon::Sword, aim: offset, muzzle: [Fx::ZERO; 3] } }
}

fn burst(agents: &[u16], i: usize) -> Command {
    Command { player: owner(i), kind: CommandKind::Burst { agent: agents[i] } }
}

pub(super) fn fx(v: f64) -> Fx {
    Fx::from_ratio((v * 1000.0).round() as i64, 1000)
}

fn close(a: FxVec2, b: FxVec2) -> bool {
    (a - b).length() < Fx::from_ratio(1, 100)
}

/// A sprite at the gun shoots the nearest threat in range of the mount
/// that the bolts in flight won't kill, on its own, once a period, while
/// the crawler walks south: every bolt leaves from where the mount is on
/// the tick it goes, one stone each. A ghoul out of range when it sets off
/// is shot once the crawler brings it in range; all three die.
#[test]
fn a_sprite_gunner_hits_the_nearest_threat_in_range_while_walking() {
    let (mut sim, agents) = lab();
    let sprite = ash(&sim);
    assert!(sim.world_mut().settlement_mut().man_gun(sprite, 0), "the gun is free");
    assert!(!sim.world_mut().settlement_mut().man_gun(sprite, 0), "and then taken");
    // Ahead (south) and to the sides; the last 13 m away at the start.
    // Wide of its feet, so none is stomped.
    ghoul(&mut sim, 9001, (-4.0, 3.0), 2, false);
    ghoul(&mut sim, 9002, (5.0, 4.5), 2, false);
    ghoul(&mut sim, 9003, (-4.0, 13.0), 2, false);
    let start = settlement(&sim).vehicles[0].position;
    let (range, period, _) = numbers(settlement(&sim));
    let far = settlement(&sim).threat(9003).unwrap().position;
    assert!((far - mount(settlement(&sim)).0).length() > range, "the third starts out of range");
    let full = stone(settlement(&sim));
    assert_eq!(full, 20, "the lab's gun starts full");
    let mut fired = Vec::new();
    for tick in 0..seconds(10.0) {
        let before = settlement(&sim).clone();
        sim.step(&if tick == 0 { vec![south(&agents)] } else { vec![] });
        let s = settlement(&sim);
        s.validate().unwrap();
        let sprite_at = &s.agents[sprite as usize];
        let v = &s.vehicles[0];
        let (spot, height) = v.seat_at(&s.characters[v.character as usize], Some(crawler::GUNNER));
        assert_eq!((sprite_at.body.position, sprite_at.sprite().unwrap().height), (spot, height), "the sprite at the gunner's place");
        if shots(s) == 0 {
            continue;
        }
        let p = s.projectiles.iter().max_by_key(|p| p.id).unwrap();
        let (from, _) = mount(s);
        assert_eq!(p.from, from, "from the mount, where it is now");
        // The nearest in range of the mount that the bolts already flying
        // won't kill, as stage 3 saw them.
        let incoming = |id: u64| before.projectiles.iter().filter(|q| q.target == Some(id)).map(|q| q.damage).sum::<u32>();
        let expected = before
            .threats
            .iter()
            .filter(|t| (t.position - from).length() <= range && incoming(t.id) < t.hp)
            .min_by_key(|t| ((t.position - from).length(), t.id))
            .map(|t| t.id);
        assert_eq!(p.target, expected, "tick {tick}");
        fired.push((sim.world().tick(), p.target.unwrap(), from));
    }
    let s = settlement(&sim);
    assert!(s.threats.is_empty(), "all three shot dead: {:?}", s.threats.iter().map(|t| (t.id, t.hp)).collect::<Vec<_>>());
    assert_eq!(fired.len(), 6, "two bolts each: {fired:?}");
    assert!(fired.windows(2).all(|w| w[1].0 - w[0].0 >= period), "once a period at most: {fired:?}");
    assert_eq!(fired[0].1, 9001, "the nearest first");
    assert!(fired.windows(2).all(|w| w[1].2 != w[0].2), "the mount moved between shots");
    assert_eq!(fired.last().unwrap().1, 9003, "the far one last, once in range");
    assert_eq!((stone(s), spent(s)), (full - 6, 6), "a stone a bolt, spent");
    assert_eq!(s.combat.kills, 3);
    assert!(s.vehicles[0].position.y - start.y > Fx::from_int(5), "it walked on");
}

/// An elder at the gun aims with the reticle, an offset from the mount cut
/// to the 7 m range, and `Attack` shoots there from the moving crawler:
/// held, once a period; pressed again within it, nothing. A stone a bolt.
#[test]
fn an_elder_gunner_fires_at_the_reticle_clamped_at_the_period() {
    let (mut sim, agents) = lab();
    board(&mut sim, &agents, &[1]);
    assert_eq!(settlement(&sim).seat(agents[1] as usize), Some(Seat::Gunner { vehicle: 0 }));
    let (range, period, _) = numbers(settlement(&sim));
    assert_eq!(range, Fx::from_int(7));
    let reticle = |sim: &Simulation| settlement(sim).agents[agents[1] as usize].avatar().unwrap().aim;
    let near = FxVec2::new(fx(2.0), fx(-1.0));
    sim.step(&[aim(&agents, 1, near), south(&agents)]);
    assert_eq!(reticle(&sim), Some(near));
    sim.step(&[aim(&agents, 1, FxVec2::new(fx(30.0), Fx::ZERO))]);
    let cut = reticle(&sim).unwrap();
    assert!((cut.length() - range).abs() < Fx::from_ratio(1, 100) && cut.y == Fx::ZERO, "cut to 7 m: {cut:?}");
    assert_eq!(shots(settlement(&sim)), 0, "aiming shoots nothing");

    // Held at a far point: a bolt each period, at the reticle 7 m from the
    // mount where it is then, aimed at no threat.
    let mut fired = Vec::new();
    for _ in 0..3 * period {
        sim.step(&[fire(&agents, 1, FxVec2::new(fx(30.0), Fx::ZERO)), fire(&agents, 1, near)]);
        let s = settlement(&sim);
        if shots(s) > 0 {
            assert_eq!(shots(s), 1, "one a tick, however many presses");
            let p = s.projectiles.iter().max_by_key(|p| p.id).unwrap();
            let (from, height) = mount(s);
            assert_eq!((p.from, p.target), (from, None));
            assert!(close(p.to, from + cut), "at the reticle: {:?} from {:?}", p.to, from);
            assert_eq!(p.launch_height, height - sim.world().terrain().height(from), "from the gun's height");
            assert!(height > s.vehicles[0].height + Fx::from_int(4), "about 4.8 m up");
            fired.push((sim.world().tick(), from));
        }
    }
    assert_eq!(fired.len(), 3, "{fired:?}");
    assert!(fired.windows(2).all(|w| w[1].0 - w[0].0 == period), "once a period: {fired:?}");
    assert!(fired.windows(2).all(|w| w[1].1 != w[0].1), "the reticle moves with the crawler");
    let s = settlement(&sim);
    assert_eq!((stone(s), spent(s), s.combat.shots), (17, 3, 3));
    assert!(s.agents[agents[1] as usize].avatar().unwrap().swing.is_none(), "a gunner never swings");
    let tick = sim.world().tick();
    assert!(matches!(s.gun_fire_check(agents[1] as usize, tick), Ok(0) | Err(FireRefusal::Reloading { .. })));
    s.validate().unwrap();

    // Climbing down turns the reticle back into a facing.
    board(&mut sim, &agents, &[1]);
    let aim = reticle(&sim).unwrap();
    assert!((aim.length() - Fx::ONE).abs() < Fx::from_ratio(1, 100), "{aim:?}");
}

/// The Burst key at the gun throws the fireball from the mount to the
/// reticle, for no stone, and starts the elder's cooldown: pressed again
/// before it is over, refused; after, another.
#[test]
fn the_gunners_fireball_on_its_cooldown_costs_no_stone() {
    let (mut sim, agents) = lab();
    board(&mut sim, &agents, &[1]);
    let (_, _, cooldown) = numbers(settlement(&sim));
    let offset = FxVec2::new(fx(-3.0), fx(4.0));
    sim.step(&[aim(&agents, 1, offset)]);
    let gunner = agents[1] as usize;
    let tick = sim.world().tick();
    let at = mount(settlement(&sim)).0;
    assert_eq!(settlement(&sim).burst_check(gunner, tick).map(|a| matches!(a, super::Ability::Fireball { to } if to == at + offset)), Ok(true));
    sim.step(&[burst(&agents, 1)]);
    let s = settlement(&sim);
    let thrown: Vec<_> = s.events.iter().filter_map(|e| match *e {
        Event::FireballThrown { mount: Mount::Gun(0), to, .. } => Some(to),
        _ => None,
    }).collect();
    let (from, height) = mount(s);
    assert_eq!(thrown, vec![from + offset], "from the mount to the reticle");
    let f = s.fireballs.last().unwrap();
    assert_eq!((f.from, f.mount, f.launch_height), (from, Mount::Gun(0), height - sim.world().terrain().height(from)));
    assert_eq!((stone(s), spent(s)), (20, 0), "no stone");
    s.validate().unwrap();
    for _ in 0..cooldown - 2 {
        sim.step(&[burst(&agents, 1)]);
        let s = settlement(&sim);
        assert!(s.events.iter().any(|e| matches!(e, Event::BurstRefused { reason: BurstRefusal::FireballCooling { .. }, .. })), "cooling");
        assert!(!s.events.iter().any(|e| matches!(e, Event::FireballThrown { .. })));
    }
    sim.step(&[]);
    sim.step(&[burst(&agents, 1)]);
    assert!(settlement(&sim).events.iter().any(|e| matches!(e, Event::FireballThrown { mount: Mount::Gun(0), .. })), "ready again after its cooldown");
    // A fire burns where it came down, as the ballista's.
    for _ in 0..seconds(3.0) {
        sim.step(&[]);
    }
    assert!(!settlement(&sim).fires.is_empty());
    assert_eq!(stone(settlement(&sim)), 20);
}

/// An empty gun never fires: not for a sprite at it with a ghoul in
/// range, not for an elder holding the fire key, who hears why; the
/// fireball still goes, costing nothing.
#[test]
fn nothing_fires_empty() {
    let (mut sim, agents) = lab();
    {
        let s = sim.world_mut().settlement_mut();
        let stone_ix = s.item_index("stone").unwrap();
        s.vehicles[0].gun.as_mut().unwrap().store.remove(stone_ix, 20);
        s.accounts[stone_ix as usize].consumed += 20;
        s.validate().unwrap();
    }
    let sprite = ash(&sim);
    assert!(sim.world_mut().settlement_mut().man_gun(sprite, 0));
    ghoul(&mut sim, 9001, (-3.0, 2.0), 2, false);
    for _ in 0..seconds(5.0) {
        sim.step(&[]);
        assert_eq!(shots(settlement(&sim)), 0, "the sprite has nothing to shoot");
    }
    assert_eq!(settlement(&sim).threat(9001).map(|t| t.hp), Some(2));

    // An elder at the empty gun (the sprite gone back to the ground).
    let (mut sim, agents2) = lab();
    {
        let s = sim.world_mut().settlement_mut();
        let stone_ix = s.item_index("stone").unwrap();
        s.vehicles[0].gun.as_mut().unwrap().store.remove(stone_ix, 20);
        s.accounts[stone_ix as usize].consumed += 20;
    }
    let _ = agents;
    board(&mut sim, &agents2, &[1]);
    let tick = sim.world().tick();
    assert_eq!(settlement(&sim).gun_fire_check(agents2[1] as usize, tick), Err(FireRefusal::NoAmmo));
    for _ in 0..seconds(2.0) {
        sim.step(&[fire(&agents2, 1, FxVec2::new(fx(3.0), Fx::ZERO))]);
        assert_eq!(shots(settlement(&sim)), 0);
    }
    sim.step(&[burst(&agents2, 1)]);
    assert!(settlement(&sim).events.iter().any(|e| matches!(e, Event::FireballThrown { .. })), "the fireball needs no stone");
    settlement(&sim).validate().unwrap();
}

/// Only the gunner fires: the pilot's and a rider's fire key does nothing
/// (`FireRefusal::NotAtGun`) and their Burst key is refused.
#[test]
fn the_pilot_and_riders_cannot_fire() {
    let (mut sim, agents) = lab();
    board(&mut sim, &agents, &[2]);
    board(&mut sim, &agents, &[3]);
    let s = settlement(&sim);
    assert_eq!(s.seat(agents[2] as usize), Some(Seat::Gunner { vehicle: 0 }));
    assert!(s.seat(agents[3] as usize).and_then(Seat::place).is_some(), "a rider");
    let tick = sim.world().tick();
    for i in [0, 3] {
        assert_eq!(s.gun_fire_check(agents[i] as usize, tick), Err(FireRefusal::NotAtGun));
    }
    for _ in 0..seconds(1.0) {
        sim.step(&[fire(&agents, 0, FxVec2::new(Fx::ONE, Fx::ZERO)), fire(&agents, 3, FxVec2::new(Fx::ONE, Fx::ZERO))]);
        assert_eq!(shots(settlement(&sim)), 0);
        assert!(settlement(&sim).projectiles.is_empty());
    }
    sim.step(&[burst(&agents, 0), burst(&agents, 3)]);
    let refused = settlement(&sim).events.iter().filter(|e| matches!(e, Event::BurstRefused { reason: BurstRefusal::NotPossessing, .. })).count();
    assert_eq!(refused, 2);
    assert!(settlement(&sim).fireballs.is_empty());
    assert_eq!(stone(settlement(&sim)), 20);
}

/// Ghouls standing right under the deck's seats, swinging for 20 s, hit
/// nobody aboard: not the pilot, a rider or a gunner, elder or sprite.
#[test]
fn nobody_aboard_is_hit() {
    for sprite_gunner in [false, true] {
        let (mut sim, agents) = lab();
        if sprite_gunner {
            let sprite = ash(&sim);
            assert!(sim.world_mut().settlement_mut().man_gun(sprite, 0));
            board(&mut sim, &agents, &[1]);
        } else {
            board(&mut sim, &agents, &[1, 2]);
        }
        sim.step(&[]);
        let s = settlement(&sim);
        let seats: Vec<_> = (0..3).map(|i| s.seat(agents[i] as usize)).collect();
        let pose = Pose::of(&s.vehicles[0]);
        let mid = pose.position - s.vehicles[0].position;
        let spots = [mid, pose.at(crawler::GUNNER) - pose.position, pose.at(crawler::RIDERS[0]) - pose.position];
        for (i, d) in spots.iter().enumerate() {
            ghoul(&mut sim, 9001 + i as u64, (d.x.to_f64(), d.y.to_f64()), 100, true);
        }
        let health: Vec<_> = settlement(&sim).agents.iter().map(|a| a.body.health).collect();
        for _ in 0..seconds(20.0) {
            // Kept under the seats, walking for the far corner.
            let s = sim.world_mut().settlement_mut();
            let (middle, corner) = (s.vehicles[0].position, sim_corner(s));
            for (i, d) in spots.iter().enumerate() {
                if let Some(t) = s.threats.iter_mut().find(|t| t.id == 9001 + i as u64) {
                    t.position = middle + *d;
                    t.route = vec![corner];
                }
            }
            sim.step(&[]);
            let s = settlement(&sim);
            let at_aboard = |a: AgentIx| (0..3).any(|i| agents[i] == a) || s.agents[a as usize].sprite().is_some_and(|sp| sp.gun.is_some());
            assert!(!s.events.iter().any(|e| matches!(*e, Event::ThreatSwing { agent, .. } | Event::ThreatWindup { agent, .. } if at_aboard(agent))), "a swing at someone aboard");
            s.validate().unwrap();
        }
        let s = settlement(&sim);
        assert_eq!(s.agents.iter().map(|a| a.body.health).collect::<Vec<_>>(), health, "nobody hurt (sprite gunner: {sprite_gunner})");
        assert_eq!((0..3).map(|i| s.seat(agents[i] as usize)).collect::<Vec<_>>(), seats, "all still aboard");
        let aboard = |a: AgentIx| s.seat(a as usize).is_some() || s.agents[a as usize].sprite().is_some_and(|sp| sp.gun.is_some());
        assert!(s.threats.iter().all(|t| t.goal.agent().is_none_or(|a| !aboard(a))), "no ghoul chases anyone aboard");
    }
}

/// A point far off in the arena's north-west, for a ghoul to walk to.
fn sim_corner(s: &Settlement) -> FxVec2 {
    s.vehicles[0].position - FxVec2::new(Fx::from_int(20), Fx::from_int(20))
}

/// An elder gunner firing and throwing the fireball from the walking
/// crawler is predicted as a possessing elder is: their replay keeps their
/// reticle, place and crawler tick for tick with the simulation, and shows
/// no shot before the world confirms it. The world, saved with bolts and a
/// fireball in the air and a sprite's shots to come, goes on alike, and on
/// four threads.
#[test]
fn a_firing_gunner_is_predicted_saved_and_hashed() {
    let (mut sim, agents) = lab();
    board(&mut sim, &agents, &[1]);
    ghoul(&mut sim, 9001, (-2.0, 9.0), 6, false);
    sim.step(&[south(&agents)]);
    let gunner = agents[1];
    let mut compared = 0;
    for round in 0..4u64 {
        let confirmed = copy(sim.world());
        let mut replay = confirmed.settlement().unwrap().replay_avatar(gunner, confirmed.tick(), confirmed.terrain()).unwrap();
        let mut ahead = Simulation::from_world(copy(&confirmed), 1);
        let mut real_shots = 0;
        for t in 0..60u64 {
            let mine: Vec<CommandKind> = match t % 30 {
                0 => vec![CommandKind::AimAvatar { agent: gunner, aim: FxVec2::new(fx(-2.0 - round as f64), fx(6.0)) }],
                5 if round == 1 => vec![CommandKind::Burst { agent: gunner }],
                _ => vec![CommandKind::Attack { agent: gunner, weapon: Weapon::Sword, aim: FxVec2::new(fx(-2.0), fx(6.0 + t as f64 / 30.0)), muzzle: [Fx::ZERO; 3] }],
            };
            replay.step_with(mine.iter());
            ahead.step(&mine.into_iter().map(|kind| Command { player: owner(1), kind }).collect::<Vec<_>>());
            let real = settlement(&ahead);
            real_shots += shots(real);
            // The replay's crawler walks as confirmed; its gun's store and
            // turret are the world's to change.
            let (ix, predicted) = replay.vehicle().expect("still aboard");
            let v = &real.vehicles[ix as usize];
            assert_eq!((predicted.position, predicted.heading, &predicted.legs), (v.position, v.heading, &v.legs), "the crawler, round {round} tick {t}");
            let (mine, theirs) = (replay.state(), &real.agents[gunner as usize]);
            assert_eq!(mine.body.position, theirs.body.position);
            assert_eq!(mine.avatar().unwrap().aim, theirs.avatar().unwrap().aim, "the reticle");
            assert!(replay.events().is_empty(), "shots wait for the world");
            compared += 1;
        }
        assert!(real_shots > 0, "round {round} fired");
        for _ in 0..30 {
            sim.step(&[]);
        }
    }
    assert_eq!(compared, 240);

    // A sprite takes over; mid-flight, saved and loaded.
    let sprite = ash(&sim);
    board(&mut sim, &agents, &[1]);
    assert!(sim.world_mut().settlement_mut().man_gun(sprite, 0));
    ghoul(&mut sim, 9002, (3.0, 9.0), 6, false);
    sim.step(&[]);
    for _ in 0..seconds(2.0) {
        sim.step(&[]);
        if !settlement(&sim).projectiles.is_empty() {
            break;
        }
    }
    assert!(!settlement(&sim).projectiles.is_empty(), "a bolt in the air");
    let world = sim.into_world();
    let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
    let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), world.rules().clone()).unwrap();
    assert_eq!(loaded.settlement().unwrap().vehicles, world.settlement().unwrap().vehicles, "the gun's state");
    assert_eq!(loaded.settlement().unwrap().sprite_gunner_of(0), Some(sprite as usize), "the sprite at the gun");
    let threaded = copy(&world);
    let (mut a, mut b, mut c) = (Simulation::from_world(world, 1), Simulation::from_world(loaded, 1), Simulation::from_world(threaded, 4));
    let mut bolts = 0;
    for _ in 0..seconds(4.0) {
        let h = a.step(&[]).total;
        assert_eq!(h, b.step(&[]).total, "the save changed it");
        assert_eq!(h, c.step(&[]).total, "four threads changed it");
        bolts += shots(settlement(&a));
    }
    assert!(bolts > 0, "the sprite fired after the save");
}

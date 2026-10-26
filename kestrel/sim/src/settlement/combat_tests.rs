//! Combat and avatar tests, ported from the original game. Durations are in
//! milliseconds (`ms`), so they hold at any tick rate.

use content::scenario::{
    ticks_from_60hz, Agent, CargoLayout, Defender, Depot, HaulLink, Item, Producer, ProjectileKind, Scenario, Stack, Wave, Weapon,
    WeaponTuning,
};
use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::combat::{combat_roll, Event, Impact, Projectile, Threat};
use super::Settlement;
use crate::messages::Strike;
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

/// Ticks in `n` milliseconds.
fn ms(n: u64) -> u64 {
    content::millis_to_ticks(n)
}

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn p(x: f64, y: f64) -> [Fx; 2] {
    [fx(x), fx(y)]
}

// The original fixtures' haulers and their quick transfers.
pub(super) const TEST_HAULER: &str =
    "(sim: (brain: Hauler, body: (radius: 0.18), movement: (walk: 2.0), carry: (transfer: 6)), view: ())";
pub(super) const TEST_PLAYER: &str = "(base: \"exosuit\", sim: (carry: (transfer: 6)), view: ())";

/// The builtin content plus the original fixtures' slow ghouls, haulers and player.
fn test_content() -> Content {
    let ghoul = |hp: u32| format!("(base: \"ghoul\", sim: (movement: (walk: 0.5), health: (hp: {hp})), view: ())");
    let mut sources = content::builtin_sources();
    sources.push(("character/test_ghoul.ron".into(), ghoul(3)));
    sources.push(("character/test_ghoul_frail.ron".into(), ghoul(2)));
    sources.push(("character/test_hauler.ron".into(), TEST_HAULER.into()));
    sources.push(("character/test_player.ron".into(), TEST_PLAYER.into()));
    Content::from_sources(sources).unwrap()
}

fn stack(item: &str, units: u32) -> Stack {
    Stack { item: item.into(), units }
}

fn depot(id: &str, position: [Fx; 2], capacity: u32, stock: Vec<Stack>) -> Depot {
    Depot { id: id.into(), label: id.into(), position, capacity, stock, stock_anchor: [0.0, 0.08, -0.6], stock_layout: CargoLayout::depot() }
}

fn agent(id: &str) -> Agent {
    Agent {
        controlled: false,
        id: id.into(),
        label: id.into(),
        skin: "worker".into(),
        character: "test_hauler".into(),
        position: p(-2.0, 1.0),
        capacity: 3,
        links: vec!["wood".into()],
        cargo_anchor: [0.0, 0.5, -0.2],
        cargo_layout: CargoLayout::default(),
        equipment: None,
    }
}

fn player() -> Agent {
    let mut player = agent("player");
    player.controlled = true;
    player.character = "test_player".into();
    player.links.clear();
    player.position = p(0.0, 0.0);
    player.capacity = 8;
    player
}

fn scenario() -> Scenario {
    Scenario {
        extent: Fx::from_int(8),
        id: "test".into(),
        weapons: vec![],
        weapon_tuning: vec![],
        items: ["wood", "iron", "arrow", "cart"]
            .iter()
            .map(|s| Item { id: (*s).into(), label: (*s).into(), skin: (*s).into(), scale: 0.2 })
            .collect(),
        depots: vec![
            depot("forest", p(-3.0, 0.0), 64, vec![stack("wood", 32)]),
            depot("mill", p(3.0, 0.0), 32, vec![]),
            depot("factory", p(-1.0, 2.0), 16, vec![stack("wood", 2), stack("iron", 1)]),
            depot("yard", p(0.0, 2.0), 2, vec![]),
            depot("ammo", p(3.0, 2.0), 32, vec![]),
        ],
        sources: vec![],
        links: vec![HaulLink { id: "wood".into(), from: "forest".into(), to: "mill".into(), item: "wood".into(), target: 24, priority: 1 }],
        agents: vec![agent("a"), agent("b"), agent("c")],
        producers: vec![],
        defenders: vec![],
        waves: vec![],
        base_health: 10,
        player_health: 0,
    }
}

/// The original combat fixture: three haulers, a player at the center, all
/// four weapons, and a wave that never spawns on its own.
fn combat_scenario(tuning: Vec<WeaponTuning>) -> Scenario {
    let mut config = scenario();
    config.weapon_tuning = tuning;
    config.agents.push(player());
    config.weapons = vec![Weapon::Sword, Weapon::Spear, Weapon::Bow, Weapon::ArcCoil];
    config.waves.push(Wave {
        id: "standard".into(),
        position: p(0.0, 6.0),
        target: p(0.0, -6.0),
        skin: "ghoul".into(),
        character: "test_ghoul".into(),
        first: Fx::from_int(3600),
        every: Fx::from_int(10),
        count: 1,
    });
    config
}

struct Arena {
    sim: Simulation,
    player: usize,
    center: FxVec2,
}

impl Arena {
    fn new(scenario: &Scenario, obstacles: &[Disc]) -> Arena {
        let world = World::new_settlement(1, &test_content(), scenario, obstacles).unwrap();
        let center = world.terrain().center;
        let player = world.settlement().unwrap().agent_index("player").map_or(usize::MAX, |i| i as usize);
        Arena { sim: Simulation::from_world(world, 2), player, center }
    }

    fn combat(obstacles: &[Disc]) -> Arena {
        Arena::new(&combat_scenario(vec![]), obstacles)
    }

    fn at(&self, x: f64, y: f64) -> FxVec2 {
        FxVec2::new(self.center.x + fx(x), self.center.y + fx(y))
    }

    fn s(&self) -> &Settlement {
        self.sim.world().settlement().unwrap()
    }

    fn s_mut(&mut self) -> &mut Settlement {
        self.sim.world_mut().settlement_mut()
    }

    fn tick(&self) -> u64 {
        self.sim.world().tick()
    }

    /// A threat walking toward (0, -6), like the original fixture's.
    fn threat(&mut self, id: u64, x: f64, y: f64, hp: u32, armor: u32) {
        let (position, goal) = (self.at(x, y), self.at(0.0, -6.0));
        let s = self.s_mut();
        s.threats.push(Threat {
            id,
            wave: 0,
            position,
            heading: Fx::ZERO,
            travelled: Fx::ZERO,
            hp,
            armor,
            pierced: false,
            shocked_until: 0,
            route: vec![goal],
            stagger_until: 0,
            knockback: FxVec2::ZERO,
        });
        s.threats.sort_by_key(|t| t.id);
        s.next_id = s.next_id.max(id + 1);
    }

    /// Resolves an attack immediately, like the original `attack_aim`.
    fn attack_toward(&mut self, weapon: Weapon, aim: FxVec2) -> u32 {
        let tick = self.tick();
        let (origin, player) = (self.s().agents[self.player].position, self.player);
        let terrain = self.sim.world().terrain().clone();
        let (targets, beam_end, cover) = self.s().strike_targets(player as u16, origin, aim, weapon, &terrain);
        let message = Strike { agent: player as u16, weapon, origin, forward: aim, muzzle: [Fx::ZERO, fx(0.6), fx(0.25)], targets, beam_end, cover };
        self.s_mut().strike(tick, message)
    }

    fn attack(&mut self, weapon: Weapon) -> u32 {
        self.attack_toward(weapon, FxVec2::new(Fx::ZERO, Fx::ONE))
    }

    fn ready(&mut self) {
        let player = self.player;
        self.s_mut().agents[player].attack_ready = 0;
    }

    fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.sim.step(&[]);
            self.s().validate().unwrap();
        }
    }

    fn command(&mut self, kind: CommandKind) {
        self.sim.step(&[Command { player: PlayerId(0), kind }]);
    }

    fn hp(&self, id: u64) -> u32 {
        self.s().threat(id).map_or(0, |t| t.hp)
    }

    fn y(&self, id: u64) -> Fx {
        self.s().threat(id).unwrap().position.y - self.center.y
    }
}

fn projectile(id: u64, from: FxVec2, to: FxVec2, target: Option<u64>, arrival: u64) -> Projectile {
    Projectile { id, defender: None, target, item: None, from, launch_height: fx(1.6), to, born: 0, arrival, damage: 8, kind: ProjectileKind::Bolt }
}

#[test]
fn melee_staggers_a_wounded_ghoul_then_it_resumes_advancing() {
    let mut arena = Arena::combat(&[]);
    arena.s_mut().waves[0].speed = content::per_tick(fx(1.0));
    // An ID whose sword roll isn't a critical on this tick.
    let id = (999..).find(|&id| combat_roll(arena.tick(), id, 0x53574f5244) % 4 != 0).unwrap();
    arena.threat(id, 0.0, 1.1, 3, 0);
    assert_eq!(arena.attack(Weapon::Sword), 1);
    assert_eq!(arena.hp(id), 1);
    assert_eq!(arena.s().combat.melee_hits, 1);
    arena.step(ms(100));
    assert!(arena.s().threat(id).unwrap().heading.abs() < fx(0.01), "a recoiling ghoul keeps facing the attacker");
    arena.step(ms(1000));
    let advanced = arena.y(id);
    assert!(arena.s().combat.knockback_meters > fx(0.3));
    assert!(advanced < fx(1.5), "the wounded ghoul resumes approaching: {advanced}");
    arena.step(ms(1000));
    let limped = arena.y(id);
    assert!(((advanced - limped) - fx(0.55)).abs() < fx(0.03), "wounded ghouls walk at 55%: {}", advanced - limped);
    // Step in front of it and finish it.
    let player = arena.player;
    let stand = arena.at(0.0, limped.to_f64() - 1.0);
    arena.s_mut().agents[player].position = stand;
    arena.ready();
    assert_eq!(arena.attack(Weapon::Sword), 1);
    assert!(arena.s().threat(id).is_none());
    assert_eq!(arena.s().combat.melee_kills, 1);
}

#[test]
fn sword_critical_severs_a_head_but_armor_deflects_the_cut() {
    let mut arena = Arena::combat(&[]);
    let mut criticals = 0;
    for hit in 1..=4 {
        let id = 100 + hit;
        arena.threat(id, 0.0, 1.0, 3, 0);
        arena.ready();
        assert_eq!(arena.attack(Weapon::Sword), 1);
        let impact = arena.s().events.iter().rev().find_map(|e| match e {
            Event::Hit { target, impact, .. } if *target == id => Some(*impact),
            _ => None,
        });
        let critical = combat_roll(arena.tick(), id, 0x53574f5244) % 4 == 0;
        assert_eq!(impact, Some(if critical { Impact::Decapitate } else { Impact::Sword }));
        if critical {
            criticals += 1;
            assert!(arena.s().threat(id).is_none());
        }
        arena.s_mut().threats.clear();
    }
    assert_eq!(arena.s().combat.decapitations, criticals);
    arena.threat(200, 0.0, 1.0, 5, 2);
    arena.ready();
    arena.attack(Weapon::Sword);
    assert_eq!(arena.hp(200), 5);
    assert_eq!(arena.s().threat(200).unwrap().armor, 1);
    assert_eq!(arena.s().combat.deflections, 1);
}

#[test]
fn bow_breaks_armor_spear_punctures_and_coil_chains() {
    let mut arena = Arena::combat(&[]);
    arena.threat(100, 0.0, 1.8, 5, 2);
    arena.threat(101, 0.5, 2.0, 3, 0);
    assert_eq!(arena.attack(Weapon::Bow), 1);
    arena.step(ms(100));
    assert_eq!(arena.hp(100), 5);
    assert_eq!(arena.s().threat(100).unwrap().armor, 1);
    assert_eq!(arena.s().combat.deflections, 1);
    assert_eq!(arena.s().spent.len(), 1);
    arena.ready();
    assert_eq!(arena.attack(Weapon::Spear), 2);
    assert_eq!(arena.s().threat(100).unwrap().armor, 0);
    assert_eq!(arena.hp(100), 4);
    assert_eq!(arena.s().combat.armor_breaks, 1);
    arena.ready();
    assert_eq!(arena.attack(Weapon::ArcCoil), 2);
    assert_eq!(arena.s().combat.arc_hits, 2);
    let tick = arena.tick();
    assert!(arena.s().threats.iter().all(|t| t.shocked_until > tick));
}

#[test]
fn ranged_attacks_follow_the_aimed_line_instead_of_seeking_a_threat() {
    let mut arena = Arena::combat(&[]);
    arena.threat(100, 0.0, 2.0, 3, 0);
    let aside = FxVec2::new(fx(0.6), fx(2.0)).normalize_or_zero();
    assert_eq!(arena.attack_toward(Weapon::Bow, aside), 1);
    arena.step(ms(400));
    assert_eq!(arena.hp(100), 3);
    arena.ready();
    assert_eq!(arena.attack_toward(Weapon::ArcCoil, aside), 0);
    assert_eq!(arena.hp(100), 3);
    assert!(arena.s().events.iter().any(|e| matches!(e, Event::ArcFired { .. })));
}

#[test]
fn rock_blocks_melee_and_defender_shots() {
    let mut arena = Arena::combat(&[Disc { center: FxVec2::new(Fx::ZERO, fx(0.65)), radius: fx(0.15) }]);
    arena.threat(100, 0.0, 1.0, 3, 0);
    assert_eq!(arena.attack(Weapon::Sword), 0);
    assert_eq!(arena.hp(100), 3);
    let (from, to) = (arena.at(0.0, 0.0), arena.at(0.0, 1.0));
    arena.s_mut().projectiles.push(projectile(200, from, to, Some(100), 2));
    let mut blocked = false;
    for _ in 0..2 {
        arena.step(1);
        blocked |= arena.s().events.iter().any(|e| matches!(e, Event::BlockedShot { .. }));
    }
    assert_eq!(arena.hp(100), 3);
    assert!(blocked);
}

#[test]
fn defender_bolt_hits_first_body_and_does_not_home() {
    let mut arena = Arena::combat(&[]);
    arena.threat(100, 0.0, 1.0, 3, 0);
    arena.threat(101, 0.0, 2.0, 3, 0);
    let (from, to) = (arena.at(0.0, 0.0), arena.at(0.0, 2.0));
    arena.s_mut().projectiles.push(projectile(200, from, to, Some(101), 2));
    let mut hit_first = false;
    for _ in 0..2 {
        arena.step(1);
        hit_first |= arena.s().events.iter().any(|e| matches!(e, Event::Hit { target: 100, .. }));
    }
    assert!(hit_first);
    assert_eq!(arena.hp(101), 3);

    let mut missed = Arena::combat(&[]);
    missed.threat(101, 1.0, 2.0, 3, 0);
    let (from, to) = (missed.at(0.0, 0.0), missed.at(0.0, 2.0));
    missed.s_mut().projectiles.push(projectile(200, from, to, Some(101), 2));
    missed.step(3);
    assert_eq!(missed.hp(101), 3);
    assert!(missed.s().projectiles.is_empty());
}

#[test]
fn authored_weapon_tuning_changes_reach_damage_and_cooldown() {
    let tuning = WeaponTuning {
        weapon: Weapon::Sword,
        cooldown_ticks: 90,
        reach: Fx::from_int(2),
        cone: fx(0.35),
        damage: 3,
        knockback: fx(5.5),
        stagger_ticks: 15,
        projectile_speed: Fx::ZERO,
        critical_one_in: 0,
    };
    let mut arena = Arena::new(&combat_scenario(vec![tuning]), &[]);
    arena.threat(100, 0.0, 1.6, 8, 0);
    assert_eq!(arena.attack(Weapon::Sword), 1);
    assert_eq!(arena.hp(100), 5);
    let tick = arena.tick();
    assert_eq!(arena.s().agents[arena.player].attack_ready, tick + ticks_from_60hz(90), "90 ticks at 60 Hz");
    assert_eq!(arena.attack(Weapon::Sword), 0);
}

/// Through the command path: contact lands on the windup tick, never earlier.
#[test]
fn scheduled_attack_contacts_on_an_integer_simulation_tick() {
    let mut arena = Arena::combat(&[]);
    arena.threat(100, 0.0, 1.0, 8, 0);
    let agent = arena.player as u16;
    arena.command(CommandKind::Attack { agent, weapon: Weapon::Sword, aim: FxVec2::new(Fx::ZERO, Fx::ONE), muzzle: [Fx::ZERO, fx(0.6), fx(0.25)] });
    arena.step(ticks_from_60hz(13) - 1); // the exosuit's windup
    assert_eq!(arena.hp(100), 8);
    arena.step(1);
    assert!(arena.hp(100) < 8);
    let swing = arena.s().agents[arena.player].avatar.swing.clone().unwrap();
    assert!(swing.resolved && swing.hits == 1);
}

#[test]
fn ballista_pierces_or_impales_from_stable_per_projectile_rolls() {
    let mut arena = Arena::combat(&[]);
    // Each bolt hits on tick 1, born on tick 0.
    let pierces = |id: u64| combat_roll(0, id, id ^ 0x424f4c5b) % 3 == 0;
    let mut ids = Vec::new();
    for id in 100..200 {
        if pierces(id) == (ids.len() == 2) {
            ids.push(id);
        }
        if ids.len() == 3 {
            break;
        }
    }
    for (index, &id) in ids.iter().enumerate() {
        let x = index as f64 - 1.0;
        arena.threat(id, x, 2.0, 3, 0);
        let (from, to) = (arena.at(x, 0.0), arena.at(x, 2.0));
        arena.s_mut().projectiles.push(projectile(id, from, to, Some(id), 1));
    }
    arena.step(2);
    let s = arena.s();
    assert_eq!(s.combat.bolt_impales, 2);
    assert_eq!(s.combat.bolt_pierces, 1);
    assert_eq!(s.pinned.len(), 2);
    assert_eq!(s.threats.len(), 1);
    assert_eq!(s.threats.iter().filter(|t| t.pierced && t.hp == 2).count(), 1);
    assert_eq!(s.spent.len(), 1);
    let mut landed = false;
    for _ in 0..ms(600) {
        arena.step(1);
        landed |= arena.s().events.iter().any(|e| matches!(e, Event::PinnedLanded { .. }));
    }
    assert!(arena.s().pinned.is_empty());
    assert!(landed);
}

#[test]
fn processing_reserves_output_space_and_defense_cannot_fire_free_ammunition() {
    let mut config = scenario();
    config.agents.clear();
    config.producers.push(Producer {
        description: String::new(),
        radius: Fx::ZERO,
        id: "mill".into(),
        label: "Mill".into(),
        position: p(0.0, 0.0),
        skin: "mill".into(),
        input: "forest".into(),
        output: "ammo".into(),
        ingredients: vec![stack("wood", 1)],
        products: vec![stack("arrow", 4)],
        seconds: fx(0.2),
        max_cycles: None,
    });
    config.depots[4].capacity = 4;
    config.defenders.push(Defender {
        description: String::new(),
        label: String::new(),
        radius: Fx::ZERO,
        id: "turret".into(),
        position: p(3.0, 2.0),
        skin: "turret".into(),
        depot: "ammo".into(),
        ammo: "arrow".into(),
        range: Fx::from_int(6),
        period: fx(0.3),
        damage: 1,
        projectile_speed: Fx::from_int(12),
        muzzle_height: fx(1.6),
        projectile: ProjectileKind::Arrow,
    });
    config.waves.push(Wave {
        id: "ghouls".into(),
        position: p(7.0, 2.0),
        target: p(0.0, 2.0),
        skin: "ghoul".into(),
        character: "test_ghoul_frail".into(),
        first: Fx::ZERO,
        every: fx(1.5),
        count: 8,
    });
    let mut arena = Arena::new(&config, &[]);
    arena.command(CommandKind::SetMachine { producer: 0, enabled: false });
    arena.step(ms(2000));
    assert_eq!(arena.s().combat.shots, 0);
    arena.command(CommandKind::SetMachine { producer: 0, enabled: true });
    arena.step(ms(20_000));
    let s = arena.s();
    assert!(s.combat.kills >= 6, "kills {}", s.combat.kills);
    assert!(s.health > 0);
    assert_eq!(s.accounts[s.item_index("arrow").unwrap() as usize].spent, s.combat.shots);
    assert!(s.metrics.cycles > 1);
}

#[test]
fn controlled_transfers_share_reservations_and_cancellation_preserves_airborne_units() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let s = arena.s();
    let (depot, item) = (s.depot_index("forest").unwrap() as usize, s.item_index("wood").unwrap() as usize);
    let dock = s.depots[depot].position;
    let player = arena.player;
    arena.s_mut().agents[player].position = dock;
    let units = arena.s().transfer_limit(player, depot, item, false).min(3);
    assert!(units > 0);
    let agent = player as u16;
    let take = |units, deposit| CommandKind::Transfer { agent, depot: depot as u16, item: item as u16, units, deposit };
    arena.command(take(units, false));
    assert!(arena.s().agents[player].avatar.job.is_some());
    arena.command(take(1, false));
    let remaining = arena.s().agents[player].avatar.job.as_ref().unwrap().remaining;
    assert_eq!(arena.s().depots[depot].out[item], remaining, "a second transfer is refused while one runs");
    assert!(arena.s().transfers.values().any(|t| t.to == super::Endpoint::Cargo(agent)));
    arena.command(CommandKind::CancelTransfer { agent });
    arena.step(ms(500));
    let carried = arena.s().agents[player].cargo[item];
    assert!(carried >= 1 && carried < units, "airborne units land, the rest is released: {carried}");
    arena.command(take(carried, true));
    arena.step(ms(1000));
    assert_eq!(arena.s().agents[player].cargo.iter().sum::<u32>(), 0);
}

#[test]
fn walking_accelerates_to_speed_and_slides_along_obstacles() {
    let mut arena = Arena::combat(&[Disc { center: FxVec2::new(Fx::from_int(2), Fx::from_int(-3)), radius: Fx::ONE }]);
    let agent = arena.player as u16;
    let start = arena.s().agents[arena.player].position;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false } });
    arena.step(ms(1000) - 1);
    let walked = arena.s().agents[arena.player].position.y - start.y;
    // 3.8 m/s after about 0.15 s of acceleration.
    assert!(walked > fx(3.3) && walked < fx(3.8), "walked {walked} in one second");
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Stop });
    arena.step(ms(333));
    let stopped = arena.s().agents[arena.player].position;
    arena.step(ms(167));
    assert_eq!(arena.s().agents[arena.player].position, stopped);

    // Walking into the rock slides around it instead of entering it.
    let rock = arena.at(2.0, -3.0);
    let (player, start) = (arena.player, arena.at(0.0, -2.9));
    arena.s_mut().agents[player].position = start;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true } });
    for _ in 0..ms(2000) {
        arena.step(1);
        let d = crate::nav::distance(arena.s().agents[arena.player].position, rock);
        assert!(d >= fx(1.32), "inside the rock: {d}");
    }
    assert!(arena.s().agents[arena.player].position.x > rock.x, "slid past the rock");
}

#[test]
fn go_to_routes_around_obstacles_and_stops_on_the_point() {
    let mut arena = Arena::combat(&[Disc { center: FxVec2::new(Fx::ZERO, fx(-2.5)), radius: Fx::ONE }]);
    let agent = arena.player as u16;
    let goal = arena.at(0.0, -5.0);
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target: goal, run: false } });
    arena.step(ms(5000));
    let a = &arena.s().agents[arena.player];
    assert_eq!(a.position, goal);
    assert_eq!(a.avatar.velocity, FxVec2::ZERO);
}

const ARENA: &str = include_str!("../../../app/assets/levels/combat_arena_supply.ron");

/// The Red Clearing plays by itself: ghouls come in three waves and the
/// ballista spends its bolts on them.
#[test]
fn combat_arena_plays_and_is_deterministic() {
    let scenario = Scenario::parse(ARENA).unwrap();
    let run = |threads| {
        let world = World::new_settlement(9, &test_content(), &scenario, &[]).unwrap();
        let mut sim = Simulation::from_world(world, threads);
        let hashes: Vec<u64> = (0..ms(60_000)).map(|_| sim.step(&[]).total).collect();
        (hashes, sim)
    };
    let (one, sim) = run(1);
    let s = sim.world().settlement().unwrap();
    s.validate().unwrap();
    assert!(s.combat.shots > 0 && s.combat.kills > 0, "shots {} kills {}", s.combat.shots, s.combat.kills);
    assert!(s.waves.iter().all(|w| w.spawned > 0));
    assert!(s.health < s.base_health, "unopposed ghouls breach");
    assert_eq!(one, run(8).0);
}

#[test]
#[ignore]
/// Prints the forest and arena metrics every 30 s, for comparing with the original game.
/// Run with `--ignored --nocapture`.
fn parity_probe() {
    for (name, text) in [("forest", super::tests::FOREST), ("arena", ARENA)] {
        let scenario = Scenario::parse(text).unwrap();
        let world = World::new_settlement(1, &test_content(), &scenario, &[]).unwrap();
        let mut sim = Simulation::from_world(world, 4);
        for second in 1..=120 {
            for _ in 0..ms(1000) {
                sim.step(&[]);
            }
            if second % 30 == 0 {
                let s = sim.world().settlement().unwrap();
                println!("{name} t={second}s delivered={} shots={} kills={} impales={} pierces={} breaches={} health={}", s.metrics.delivered, s.combat.shots, s.combat.kills, s.combat.bolt_impales, s.combat.bolt_pierces, s.combat.breaches, s.health);
            }
        }
    }
}

/// Two players hold the Red Clearing together: a guest joins, each chases
/// the nearest ghoul and strikes it (sword and spear), and the guest also
/// fetches bolts from the rack. Each player's hits are their own.
#[test]
fn two_players_defend_the_clearing() {
    let scenario = Scenario::parse(ARENA).unwrap();
    let world = World::new_settlement(9, &test_content(), &scenario, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 2);
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let s = sim.world().settlement().unwrap();
    let players = [(host, s.avatar_of(host).unwrap(), Weapon::Sword), (guest, s.avatar_of(guest).unwrap(), Weapon::Spear)];
    let (rack, bolt) = (s.depot_index("bolt_rack").unwrap(), s.item_index("bolt").unwrap());

    // The guest takes two bolts off the rack first.
    let dock = s.depots[rack as usize].position;
    let fetch = |kind| Command { player: guest, kind };
    sim.step(&[
        fetch(CommandKind::MoveAvatar { agent: players[1].1, intent: Intent::GoTo { target: dock, run: true } }),
        fetch(CommandKind::Transfer { agent: players[1].1, depot: rack, item: bolt, units: 2, deposit: false }),
    ]);
    for _ in 0..ms(8000) {
        sim.step(&[]);
    }
    assert_eq!(sim.world().settlement().unwrap().agents[players[1].1 as usize].cargo[bolt as usize], 2, "the guest carries two bolts");

    let mut hits = [0u32; 2];
    for _ in 0..ms(60_000) {
        let s = sim.world().settlement().unwrap();
        let tick = sim.world().tick();
        let mut turn = Vec::new();
        for &(player, agent, weapon) in &players {
            let a = &s.agents[agent as usize];
            let Some(t) = s.threats.iter().min_by_key(|t| (t.position - a.position).length()) else { continue };
            let to = t.position - a.position;
            let kind = if to.length() > fx(1.0) {
                CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: to.normalize_or_zero(), run: true } }
            } else if tick >= a.attack_ready && a.avatar.swing.as_ref().is_none_or(|s| tick >= s.until) {
                CommandKind::Attack { agent, weapon, aim: to, muzzle: [Fx::ZERO, fx(0.6), fx(0.25)] }
            } else {
                continue;
            };
            turn.push(Command { player, kind });
        }
        sim.step(&turn);
        for event in &sim.world().settlement().unwrap().events {
            if let Event::Hit { agent: Some(agent), .. } = event {
                hits[players.iter().position(|p| p.1 == *agent).unwrap()] += 1;
            }
        }
    }
    let s = sim.world().settlement().unwrap();
    assert!(hits.iter().all(|&n| n > 3), "both players hit ghouls: {hits:?}");
    assert!(s.combat.melee_hits > 0 && s.combat.spear_hits > 0 && s.combat.kills > 5, "{:?}", s.combat);
}

/// The weapon in hand is shared state, so every peer can show it: players
/// equip their own avatar only, with weapons the scenario offers, and a
/// swing puts its weapon in hand too.
#[test]
fn players_equip_their_own_weapons() {
    let scenario = Scenario::parse(ARENA).unwrap();
    let world = World::new_settlement(9, &test_content(), &scenario, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 2);
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let s = sim.world().settlement().unwrap();
    let (mine, theirs) = (s.avatar_of(host).unwrap(), s.avatar_of(guest).unwrap());
    let weapon = |sim: &Simulation, agent: u16| sim.world().settlement().unwrap().agents[agent as usize].avatar.weapon;
    assert_eq!(weapon(&sim, theirs), Weapon::Sword);

    sim.step(&[
        Command { player: guest, kind: CommandKind::Equip { agent: theirs, weapon: Weapon::Bow } },
        // Not the host's avatar to arm.
        Command { player: host, kind: CommandKind::Equip { agent: theirs, weapon: Weapon::ArcCoil } },
    ]);
    assert_eq!(weapon(&sim, theirs), Weapon::Bow);
    assert_eq!(weapon(&sim, mine), Weapon::Sword);

    let aim = FxVec2::new(Fx::ZERO, Fx::ONE);
    sim.step(&[Command { player: host, kind: CommandKind::Attack { agent: mine, weapon: Weapon::Spear, aim, muzzle: [Fx::ZERO; 3] } }]);
    assert_eq!(weapon(&sim, mine), Weapon::Spear, "a swing puts its weapon in hand");
}

#[test]
fn laser_stops_at_obstacle_before_its_full_range() {
    let mut config = scenario();
    config.agents.push(player());
    config.weapons = vec![Weapon::Laser];
    let obstacle = Disc { center: FxVec2::new(Fx::ZERO, Fx::from_int(4)), radius: Fx::ONE };
    let arena = Arena::new(&config, &[obstacle]);
    let origin = arena.s().agents[arena.player].position;
    let terrain = arena.sim.world().terrain();
    let (targets, end, cover) = arena.s().strike_targets(arena.player as u16, origin, FxVec2::new(Fx::ZERO, Fx::ONE), Weapon::Laser, terrain);
    assert!(targets.is_empty());
    assert_eq!(cover, None);
    let end = end.unwrap();
    let travel = end.y - origin.y;
    assert!(travel > fx(2.8) && travel < fx(3.2), "laser should stop at the obstacle edge: {end:?}");
}

#[test]
fn laser_selects_the_struck_cover_piece_and_opens_it_incrementally() {
    use crate::terrain::Destructible;
    let mut config = scenario();
    config.agents.push(player());
    config.weapons = vec![Weapon::Laser];
    let arena = Arena::new(&config, &[]);
    let mut terrain = arena.sim.world().terrain().clone();
    terrain.destructibles = [-0.5, 0.5].into_iter().enumerate().map(|(asset, x)| Destructible {
        asset: asset as u16,
        disc: Disc { center: arena.at(x, 4.0), radius: fx(0.4) }, health: 2,
        max_health: 2, repairable: false, repair_progress: 0,
    }).collect();
    let origin = arena.s().agents[arena.player].position;
    let left = FxVec2::new(fx(-0.125), Fx::ONE);
    let right = FxVec2::new(fx(0.125), Fx::ONE);
    let (_, end, cover) = arena.s().strike_targets(arena.player as u16, origin, left, Weapon::Laser, &terrain);
    assert_eq!(cover, Some(0));
    assert!(end.unwrap().y < arena.at(0.0, 4.0).y);
    assert!(terrain.damage_cover(0, 1));
    assert_eq!(terrain.destructibles[0].health, 1);
    assert_eq!(arena.s().strike_targets(arena.player as u16, origin, left, Weapon::Laser, &terrain).2, Some(0));
    assert!(terrain.damage_cover(0, 1));
    assert_eq!(terrain.destructibles[0].health, 0);
    assert_eq!(arena.s().strike_targets(arena.player as u16, origin, left, Weapon::Laser, &terrain).2, None);
    assert_eq!(arena.s().strike_targets(arena.player as u16, origin, right, Weapon::Laser, &terrain).2, Some(1));
}

#[test]
fn firing_laser_does_not_stop_movement() {
    let mut config = scenario();
    config.agents.push(player());
    config.weapons = vec![Weapon::Laser];
    let mut arena = Arena::new(&config, &[]);
    let agent = arena.player as u16;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: true } });
    let before = arena.s().agents[arena.player].position;
    arena.command(CommandKind::Attack { agent, weapon: Weapon::Laser, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3] });
    arena.step(ms(100));
    let avatar = &arena.s().agents[arena.player];
    assert!(avatar.avatar.swing.is_some(), "shot should still be active");
    assert!(avatar.position.y > before.y, "avatar should move while firing");
}

#[test]
fn mouse_aim_changes_independently_of_travel_and_persists_without_firing() {
    let mut config = scenario();
    config.agents.push(player());
    config.weapons = vec![Weapon::Laser];
    let mut arena = Arena::new(&config, &[]);
    let agent = arena.player as u16;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk {
        direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false,
    } });
    arena.step(ms(200));
    let heading = arena.s().agents[arena.player].heading;
    let aim = FxVec2::new(Fx::ONE, Fx::ZERO);
    arena.command(CommandKind::AimAvatar { agent, aim });
    arena.step(ms(200));
    let avatar = &arena.s().agents[arena.player];
    assert_eq!(avatar.avatar.aim, Some(aim));
    assert_eq!(avatar.heading, heading);
    assert!(avatar.avatar.swing.is_none());
    assert!(avatar.avatar.velocity.y > Fx::ZERO);
}

#[test]
fn laser_hits_another_player_and_respawns_them() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.depots.clear();
    config.links.clear();
    config.weapons = vec![Weapon::Laser];
    config.player_health = 2;
    let world = World::new_settlement(9, &test_content(), &config, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 2);
    let spawn = sim.world().settlement().unwrap().agents[0].position;
    sim.world_mut().settlement_mut().spawn = spawn;
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let settlement = sim.world().settlement().unwrap();
    let (shooter, target) = (settlement.avatar_of(host).unwrap(), settlement.avatar_of(guest).unwrap());
    let start = settlement.agents[shooter as usize].position;
    sim.world_mut().settlement_mut().agents[target as usize].position = start + FxVec2::new(Fx::ZERO, Fx::from_int(3));
    let fire = || Command { player: host, kind: CommandKind::Attack {
        agent: shooter, weapon: Weapon::Laser,
        aim: FxVec2::new(Fx::ZERO, Fx::ONE), muzzle: [Fx::ZERO, fx(0.8), fx(0.25)],
    } };
    for remaining in [1, 0] {
        sim.step(&[fire()]);
        for _ in 0..ms(700) { sim.step(&[]); }
        let settlement = sim.world().settlement().unwrap();
        assert_eq!(settlement.agents[target as usize].avatar.health, remaining);
        assert_eq!(settlement.agents[shooter as usize].avatar.health, 2);
    }
    for _ in 0..ms(2200) { sim.step(&[]); }
    let settlement = sim.world().settlement().unwrap();
    assert_eq!(settlement.agents[target as usize].avatar.health, 2);
    assert_eq!(settlement.agents[target as usize].position, settlement.agents[target as usize].home);
}

/// A depot transfer walking to its dock is an accepted job: releasing
/// direct control keeps the walk and the transfer.
#[test]
fn releasing_keeps_a_depot_transfer_walking_to_its_dock() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let s = arena.s();
    let (depot, item) = (s.depot_index("forest").unwrap(), s.item_index("wood").unwrap());
    let dock = s.depots[depot as usize].position;
    let agent = arena.player as u16;
    arena.command(CommandKind::Transfer { agent, depot, item, units: 2, deposit: false });
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target: dock, run: false } });
    let route = arena.s().agents[arena.player].route.clone();
    assert!(!route.is_empty());
    arena.command(CommandKind::ReleaseDirectControl { agent });
    assert!(arena.s().agents[arena.player].avatar.job.is_some());
    assert!(!arena.s().agents[arena.player].route.is_empty(), "the dock walk continues");
    arena.step(ms(6000));
    assert_eq!(arena.s().agents[arena.player].cargo[item as usize], 2, "the transfer completed");
}

/// Haulers steer around the avatar at the radius it collides with: its
/// character's body radius, not a scenario value.
#[test]
fn haulers_see_the_avatar_at_its_character_radius() {
    let arena = Arena::combat(&[]);
    let s = arena.s();
    let player = &s.agents[arena.player];
    let def = s.character_of(arena.player).expect("the avatar has a character");
    assert_eq!(player.radius(), def.body.radius);
    assert_eq!(super::haulers::body(player).radius, def.body.radius);
}

/// A brute is wide while its plating lasts and shrinks to its body once
/// the armor breaks.
#[test]
fn brute_radius_follows_its_armor() {
    let scenario = Scenario::parse(ARENA).unwrap();
    let world = World::new_settlement(1, &test_content(), &scenario, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 1);
    let brutes = sim.world().settlement().unwrap().waves.iter().position(|w| w.armor > 0).expect("the arena has a brute wave");
    let mut arena = Arena { sim, player: 0, center: FxVec2::ZERO };
    let s = arena.s_mut();
    let mut brute = Threat {
        id: 900,
        wave: brutes as u16,
        position: FxVec2::ZERO,
        heading: Fx::ZERO,
        travelled: Fx::ZERO,
        hp: 5,
        armor: 2,
        pierced: false,
        shocked_until: 0,
        route: vec![],
        stagger_until: 0,
        knockback: FxVec2::ZERO,
    };
    let def = &s.characters[s.waves[brutes].character as usize];
    let (body, plated) = (def.body.radius, def.health.as_ref().unwrap().armor.as_ref().unwrap().radius);
    assert_eq!(s.threat_radius(&brute), plated);
    brute.armor = 0;
    assert_eq!(s.threat_radius(&brute), body);
    assert!(body < plated);
}

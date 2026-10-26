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
use crate::messages::{CarryUnit, Strike};
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
    "(sim: (brain: Hauler, body: (radius: 0.18), movement: (walk: 2.0), carry: (transfer: 6)), view: (skin: \"worker\"))";
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
            windup: None,
            ready_at: 0,
        });
        s.threats.sort_by_key(|t| t.id);
        s.next_id = s.next_id.max(id + 1);
    }

    /// Resolves an attack immediately, like the original `attack_aim`.
    fn attack_toward(&mut self, weapon: Weapon, aim: FxVec2) -> u32 {
        let tick = self.tick();
        let (origin, player) = (self.s().agents[self.player].body.position, self.player);
        let terrain = self.sim.world().terrain().clone();
        let height = self.s().agents[player].avatar().unwrap().height;
        let aimed = self.s().strike_targets(player as u16, height, origin, aim, weapon, &terrain);
        let message = Strike {
            agent: player as u16,
            weapon,
            origin,
            forward: aim,
            muzzle: [Fx::ZERO, fx(0.6), fx(0.25)],
            targets: aimed.targets,
            beam_end: aimed.beam_end,
            cover: aimed.cover,
            hit_player: aimed.hit_player,
            started: tick,
        };
        let (settlement, terrain) = self.sim.world_mut().settlement_and_terrain_mut();
        settlement.strike(tick, message, terrain)
    }

    fn attack(&mut self, weapon: Weapon) -> u32 {
        self.attack_toward(weapon, FxVec2::new(Fx::ZERO, Fx::ONE))
    }

    fn ready(&mut self) {
        let player = self.player;
        self.s_mut().agents[player].avatar_mut().unwrap().attack_ready = 0;
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
    arena.s_mut().agents[player].body.position = stand;
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
    assert_eq!(arena.s().agents[arena.player].avatar().unwrap().attack_ready, tick + ticks_from_60hz(90), "90 ticks at 60 Hz");
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
    let swing = arena.s().agents[arena.player].avatar().unwrap().swing.clone().unwrap();
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
    arena.s_mut().agents[player].body.position = dock;
    let units = arena.s().transfer_limit(player, depot, item, false).min(3);
    assert!(units > 0);
    let agent = player as u16;
    let take = |units, deposit| CommandKind::Transfer { agent, depot: depot as u16, item: item as u16, units, deposit };
    arena.command(take(units, false));
    assert!(arena.s().agents[player].carry.job.is_some());
    arena.command(take(1, false));
    let remaining = arena.s().agents[player].carry.job.as_ref().unwrap().remaining;
    assert_eq!(arena.s().depots[depot].inventory.reserved_out[item], remaining, "a second transfer is refused while one runs");
    assert!(arena.s().transfers.values().any(|t| t.to == super::InventoryRef::Agent(agent)));
    arena.command(CommandKind::CancelTransfer { agent });
    arena.step(ms(500));
    let carried = arena.s().agents[player].carry.inventory.counts[item];
    assert!(carried >= 1 && carried < units, "airborne units land, the rest is released: {carried}");
    arena.command(take(carried, true));
    arena.step(ms(1000));
    assert_eq!(arena.s().agents[player].carry.inventory.counts.iter().sum::<u32>(), 0);
}

/// A transfer reserves its units at the depot, and a cancel releases them,
/// on the tick the command applies (stage 5 of that tick).
#[test]
fn a_transfer_reserves_stock_and_a_cancel_releases_it_on_their_tick() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let s = arena.s();
    let (depot, item) = (s.depot_index("forest").unwrap(), s.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let agent = arena.player as u16;
    // Far from the dock, so no unit moves.
    assert!((s.agents[arena.player].body.position - s.depots[d].position).length() > fx(1.0));
    let (out, incoming) = (s.depots[d].inventory.reserved_out[i], s.depots[d].inventory.reserved_in[i]);

    arena.command(CommandKind::Transfer { agent, depot, item, units: 3, deposit: false });
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out + 3, "the pickup reserves its stock");
    assert_eq!(arena.s().agents[arena.player].carry.job.as_ref().unwrap().remaining, 3);
    arena.command(CommandKind::CancelTransfer { agent });
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out, "the cancel releases it");
    assert!(arena.s().agents[arena.player].carry.job.is_none());

    // Cargo to deposit, handed over outside the accounts (so no `validate`).
    let player = arena.player;
    arena.s_mut().agents[player].carry.inventory.counts[i] = 2;
    arena.command(CommandKind::Transfer { agent, depot, item, units: 2, deposit: true });
    assert_eq!(arena.s().depots[d].inventory.reserved_in[i], incoming + 2, "the deposit reserves room");
    arena.command(CommandKind::CancelTransfer { agent });
    assert_eq!(arena.s().depots[d].inventory.reserved_in[i], incoming);
}

/// A transfer holds its units in the player's cargo too, when the command
/// applies: room for a pickup, the units for a deposit. Each unit moved
/// takes its share, and dropping the job gives back the rest.
#[test]
fn a_transfer_reserves_the_cargo_side_and_a_cancel_releases_it() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let s = arena.s();
    let (depot, item) = (s.depot_index("forest").unwrap(), s.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let agent = arena.player as u16;
    let player = arena.player;
    let cargo = |arena: &Arena| {
        let carry = &arena.s().agents[player].carry;
        (carry.inventory.reserved_in[i], carry.inventory.reserved_out[i], carry.job.as_ref().map(|j| j.remaining))
    };
    // Far from the dock, so no unit moves.
    assert!((s.agents[player].body.position - s.depots[d].position).length() > fx(1.0));
    let free = s.agents[player].carry.inventory.free();

    arena.command(CommandKind::Transfer { agent, depot, item, units: 3, deposit: false });
    assert_eq!(cargo(&arena), (3, 0, Some(3)), "the pickup holds room in the cargo");
    assert_eq!(arena.s().agents[player].carry.inventory.free(), free - 3);
    arena.s().validate().unwrap();
    arena.command(CommandKind::CancelTransfer { agent });
    assert_eq!(cargo(&arena), (0, 0, None), "the cancel gives the room back");
    arena.s().validate().unwrap();

    // Pick up two units at the dock, then deposit them again.
    arena.s_mut().agents[player].body.position = arena.s().depots[d].position;
    arena.command(CommandKind::Transfer { agent, depot, item, units: 2, deposit: false });
    arena.step(ms(1000));
    assert_eq!(arena.s().agents[player].carry.inventory.counts[i], 2);
    assert_eq!(cargo(&arena), (0, 0, None), "every unit landed");
    arena.command(CommandKind::Transfer { agent, depot, item, units: 2, deposit: true });
    // The first unit left on the command's tick, with its share.
    assert_eq!(cargo(&arena), (0, 1, Some(1)), "the deposit holds its units in the cargo");
    assert_eq!(arena.s().agents[player].carry.inventory.counts[i], 1);
    arena.s().validate().unwrap();
    arena.command(CommandKind::CancelTransfer { agent });
    assert_eq!(cargo(&arena), (0, 0, None), "the cancel gives the unit back");
    assert_eq!(arena.s().agents[player].carry.inventory.available(item), 1);
    arena.s().validate().unwrap();
}

/// Respawning drops a running transfer, and its reserved stock goes back to
/// the depot on the same tick, as a cancel would. Also when the transfer and
/// the respawn come in one turn.
#[test]
fn respawning_releases_a_transfer_reservation() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let s = arena.s();
    let (depot, item) = (s.depot_index("forest").unwrap(), s.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let agent = arena.player as u16;
    // Far from the dock, so no unit moves.
    assert!((s.agents[arena.player].body.position - s.depots[d].position).length() > fx(1.0));
    let out = s.depots[d].inventory.reserved_out[i];

    arena.command(CommandKind::Transfer { agent, depot, item, units: 3, deposit: false });
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out + 3);
    arena.command(CommandKind::Respawn { agent });
    assert!(arena.s().agents[arena.player].carry.job.is_none());
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out, "the respawn releases the reservation");
    arena.s().validate().unwrap();

    let player = PlayerId(0);
    let turn = [CommandKind::Transfer { agent, depot, item, units: 2, deposit: false }, CommandKind::Respawn { agent }];
    arena.sim.step(&turn.map(|kind| Command { player, kind }));
    assert!(arena.s().agents[arena.player].carry.job.is_none());
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out, "reserved and released in one tick");
    arena.s().validate().unwrap();
}

/// Two players take the same depot's last units on one tick. Both actions
/// see the stock as it was when the tick began, so both avatars take the
/// job; stage 5 then reserves in command order (player order), and the
/// second reservation, which no longer fits, is refused and its job rolls
/// back. A cancel of the refused job in the same turn releases nothing.
#[test]
fn two_transfers_from_one_depot_on_one_tick_go_to_the_first_player() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let (host, guest) = (PlayerId(0), PlayerId(1));
    arena.sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let s = arena.s();
    let (depot, item) = (s.depot_index("factory").unwrap(), s.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let (a, b) = (s.avatar_of(host).unwrap(), s.avatar_of(guest).unwrap());
    assert_eq!(s.depots[d].inventory.available(item), 2);
    for agent in [a, b] {
        assert_eq!(s.transfer_limit(agent as usize, d, i, false), 2);
        assert!((s.agents[agent as usize].body.position - s.depots[d].position).length() > fx(1.0), "no unit moves");
    }
    let take = |player, agent| Command { player, kind: CommandKind::Transfer { agent, depot, item, units: 2, deposit: false } };

    // The guest's command comes first in the turn; the host still wins.
    arena.sim.step(&[take(guest, b), take(host, a)]);
    let s = arena.s();
    assert_eq!(s.depots[d].inventory.reserved_out[i], 2, "reserved once");
    assert_eq!(s.agents[a as usize].carry.job.as_ref().map(|j| j.remaining), Some(2), "the host's transfer stands");
    assert!(s.agents[b as usize].carry.job.is_none(), "the guest's rolled back");
    s.validate().unwrap();
    arena.sim.step(&[Command { player: host, kind: CommandKind::CancelTransfer { agent: a } }]);
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], 0);

    // The refused transfer cancelled in the same turn releases nothing.
    let cancel = Command { player: guest, kind: CommandKind::CancelTransfer { agent: b } };
    arena.sim.step(&[take(host, a), take(guest, b), cancel]);
    let s = arena.s();
    assert_eq!(s.depots[d].inventory.reserved_out[i], 2);
    assert!(s.agents[a as usize].carry.job.is_some() && s.agents[b as usize].carry.job.is_none());
    s.validate().unwrap();

    // A transfer cancelled in its own turn leaves the stock to the next.
    let cancel = Command { player: host, kind: CommandKind::CancelTransfer { agent: a } };
    arena.sim.step(&[cancel.clone()]);
    arena.sim.step(&[take(host, a), cancel, take(guest, b)]);
    let s = arena.s();
    assert_eq!(s.depots[d].inventory.reserved_out[i], 2);
    assert!(s.agents[a as usize].carry.job.is_none());
    assert_eq!(s.agents[b as usize].carry.job.as_ref().map(|j| j.remaining), Some(2), "the guest takes it");
    s.validate().unwrap();
}

#[test]
fn walking_accelerates_to_speed_and_slides_along_obstacles() {
    let mut arena = Arena::combat(&[Disc { center: FxVec2::new(Fx::from_int(2), Fx::from_int(-3)), radius: Fx::ONE }]);
    let agent = arena.player as u16;
    let start = arena.s().agents[arena.player].body.position;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false } });
    arena.step(ms(1000) - 1);
    let walked = arena.s().agents[arena.player].body.position.y - start.y;
    // 3.8 m/s after about 0.15 s of acceleration.
    assert!(walked > fx(3.3) && walked < fx(3.8), "walked {walked} in one second");
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Stop });
    arena.step(ms(333));
    let stopped = arena.s().agents[arena.player].body.position;
    arena.step(ms(167));
    assert_eq!(arena.s().agents[arena.player].body.position, stopped);

    // Walking into the rock slides around it instead of entering it.
    let rock = arena.at(2.0, -3.0);
    let (player, start) = (arena.player, arena.at(0.0, -2.9));
    arena.s_mut().agents[player].body.position = start;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true } });
    for _ in 0..ms(2000) {
        arena.step(1);
        let d = crate::nav::distance(arena.s().agents[arena.player].body.position, rock);
        assert!(d >= fx(1.32), "inside the rock: {d}");
    }
    assert!(arena.s().agents[arena.player].body.position.x > rock.x, "slid past the rock");
}

#[test]
fn go_to_routes_around_obstacles_and_stops_on_the_point() {
    let mut arena = Arena::combat(&[Disc { center: FxVec2::new(Fx::ZERO, fx(-2.5)), radius: Fx::ONE }]);
    let agent = arena.player as u16;
    let goal = arena.at(0.0, -5.0);
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target: goal, run: false } });
    arena.step(ms(5000));
    let a = &arena.s().agents[arena.player];
    assert_eq!(a.body.position, goal);
    assert_eq!(a.body.velocity, FxVec2::ZERO);
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
    assert_eq!(sim.world().settlement().unwrap().agents[players[1].1 as usize].carry.inventory.counts[bolt as usize], 2, "the guest carries two bolts");

    let mut hits = [0u32; 2];
    for _ in 0..ms(60_000) {
        let s = sim.world().settlement().unwrap();
        let tick = sim.world().tick();
        let mut turn = Vec::new();
        for &(player, agent, weapon) in &players {
            let a = &s.agents[agent as usize];
            let Some(t) = s.threats.iter().min_by_key(|t| (t.position - a.body.position).length()) else { continue };
            let to = t.position - a.body.position;
            let kind = if to.length() > fx(1.0) {
                CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: to.normalize_or_zero(), run: true } }
            } else if tick >= a.avatar().unwrap().attack_ready && a.avatar().unwrap().swing.as_ref().is_none_or(|s| tick >= s.until) {
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
/// swing puts its weapon in hand too. A command naming another player's
/// avatar drives the sender's own (`World::apply_commands`).
#[test]
fn players_equip_their_own_weapons() {
    let scenario = Scenario::parse(ARENA).unwrap();
    let world = World::new_settlement(9, &test_content(), &scenario, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 2);
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let s = sim.world().settlement().unwrap();
    let (mine, theirs) = (s.avatar_of(host).unwrap(), s.avatar_of(guest).unwrap());
    let weapon = |sim: &Simulation, agent: u16| sim.world().settlement().unwrap().agents[agent as usize].avatar().unwrap().weapon;
    assert_eq!(weapon(&sim, theirs), Weapon::Sword);

    sim.step(&[
        Command { player: guest, kind: CommandKind::Equip { agent: theirs, weapon: Weapon::Bow } },
        // Not the host's avatar to arm: the host arms their own.
        Command { player: host, kind: CommandKind::Equip { agent: theirs, weapon: Weapon::ArcCoil } },
    ]);
    assert_eq!(weapon(&sim, theirs), Weapon::Bow);
    assert_eq!(weapon(&sim, mine), Weapon::ArcCoil);

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
    let origin = arena.s().agents[arena.player].body.position;
    let terrain = arena.sim.world().terrain();
    let aimed = arena.s().strike_targets(arena.player as u16, arena.s().agents[arena.player].avatar().unwrap().height, origin, FxVec2::new(Fx::ZERO, Fx::ONE), Weapon::Laser, terrain);
    let (targets, end, cover) = (aimed.targets, aimed.beam_end, aimed.cover);
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
    let origin = arena.s().agents[arena.player].body.position;
    let left = FxVec2::new(fx(-0.125), Fx::ONE);
    let right = FxVec2::new(fx(0.125), Fx::ONE);
    let aimed = arena.s().strike_targets(arena.player as u16, arena.s().agents[arena.player].avatar().unwrap().height, origin, left, Weapon::Laser, &terrain);
    let (end, cover) = (aimed.beam_end, aimed.cover);
    assert_eq!(cover, Some(0));
    assert!(end.unwrap().y < arena.at(0.0, 4.0).y);
    assert!(terrain.damage_cover(0, 1));
    assert_eq!(terrain.destructibles[0].health, 1);
    assert_eq!(arena.s().strike_targets(arena.player as u16, arena.s().agents[arena.player].avatar().unwrap().height, origin, left, Weapon::Laser, &terrain).cover, Some(0));
    assert!(terrain.damage_cover(0, 1));
    assert_eq!(terrain.destructibles[0].health, 0);
    assert_eq!(arena.s().strike_targets(arena.player as u16, arena.s().agents[arena.player].avatar().unwrap().height, origin, left, Weapon::Laser, &terrain).cover, None);
    assert_eq!(arena.s().strike_targets(arena.player as u16, arena.s().agents[arena.player].avatar().unwrap().height, origin, right, Weapon::Laser, &terrain).cover, Some(1));
}

#[test]
fn firing_laser_does_not_stop_movement() {
    let mut config = scenario();
    config.agents.push(player());
    config.weapons = vec![Weapon::Laser];
    let mut arena = Arena::new(&config, &[]);
    let agent = arena.player as u16;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: true } });
    let before = arena.s().agents[arena.player].body.position;
    arena.command(CommandKind::Attack { agent, weapon: Weapon::Laser, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3] });
    arena.step(ms(100));
    let avatar = &arena.s().agents[arena.player];
    assert!(avatar.avatar().unwrap().swing.is_some(), "shot should still be active");
    assert!(avatar.body.position.y > before.y, "avatar should move while firing");
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
    let heading = arena.s().agents[arena.player].body.heading;
    let aim = FxVec2::new(Fx::ONE, Fx::ZERO);
    arena.command(CommandKind::AimAvatar { agent, aim });
    arena.step(ms(200));
    let avatar = &arena.s().agents[arena.player];
    assert_eq!(avatar.avatar().unwrap().aim, Some(aim));
    assert_eq!(avatar.body.heading, heading);
    assert!(avatar.avatar().unwrap().swing.is_none());
    assert!(avatar.body.velocity.y > Fx::ZERO);
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
    let spawn = sim.world().settlement().unwrap().agents[0].body.position;
    sim.world_mut().settlement_mut().spawn = spawn;
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let settlement = sim.world().settlement().unwrap();
    let (shooter, target) = (settlement.avatar_of(host).unwrap(), settlement.avatar_of(guest).unwrap());
    let start = settlement.agents[shooter as usize].body.position;
    sim.world_mut().settlement_mut().agents[target as usize].body.position = start + FxVec2::new(Fx::ZERO, Fx::from_int(3));
    let fire = || Command { player: host, kind: CommandKind::Attack {
        agent: shooter, weapon: Weapon::Laser,
        aim: FxVec2::new(Fx::ZERO, Fx::ONE), muzzle: [Fx::ZERO, fx(0.8), fx(0.25)],
    } };
    for remaining in [1, 0] {
        sim.step(&[fire()]);
        for _ in 0..ms(700) { sim.step(&[]); }
        let settlement = sim.world().settlement().unwrap();
        assert_eq!(settlement.agents[target as usize].body.health, remaining);
        assert_eq!(settlement.agents[shooter as usize].body.health, 2);
    }
    for _ in 0..ms(2200) { sim.step(&[]); }
    let settlement = sim.world().settlement().unwrap();
    assert_eq!(settlement.agents[target as usize].body.health, 2);
    assert_eq!(settlement.agents[target as usize].body.position, settlement.agents[target as usize].body.home);
}

/// A player killed mid-transfer drops the job, and its reserved stock goes
/// back to the depot on the tick it dies, as a respawn would.
#[test]
fn dying_releases_a_transfer_reservation() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    config.weapons = vec![Weapon::Laser];
    config.player_health = 1;
    let world = World::new_settlement(9, &test_content(), &config, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 2);
    let spawn = sim.world().settlement().unwrap().agents[0].body.position;
    sim.world_mut().settlement_mut().spawn = spawn;
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let settlement = sim.world().settlement().unwrap();
    let (shooter, target) = (settlement.avatar_of(host).unwrap(), settlement.avatar_of(guest).unwrap());
    let (depot, item) = (settlement.depot_index("forest").unwrap(), settlement.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let out = settlement.depots[d].inventory.reserved_out[i];
    let dock = settlement.depots[d].position;
    let start = settlement.agents[shooter as usize].body.position;
    let to = start + FxVec2::new(Fx::ZERO, Fx::from_int(3));
    sim.world_mut().settlement_mut().agents[target as usize].body.position = to;
    // Far from the dock, so no unit moves.
    assert!((to - dock).length() > fx(1.0));

    sim.step(&[Command { player: guest, kind: CommandKind::Transfer { agent: target, depot, item, units: 3, deposit: false } }]);
    assert_eq!(sim.world().settlement().unwrap().depots[d].inventory.reserved_out[i], out + 3);
    assert_eq!(sim.world().settlement().unwrap().agents[target as usize].carry.inventory.reserved_in[i], 3, "and room in the cargo");
    sim.step(&[Command { player: host, kind: CommandKind::Attack {
        agent: shooter, weapon: Weapon::Laser,
        aim: FxVec2::new(Fx::ZERO, Fx::ONE), muzzle: [Fx::ZERO, fx(0.8), fx(0.25)],
    } }]);
    for _ in 0..ms(700) { sim.step(&[]); }
    let settlement = sim.world().settlement().unwrap();
    assert_eq!(settlement.agents[target as usize].body.health, 0, "the laser killed the target");
    assert!(settlement.agents[target as usize].carry.job.is_none());
    assert_eq!(settlement.depots[d].inventory.reserved_out[i], out, "death releases the reservation");
    assert_eq!(settlement.agents[target as usize].carry.inventory.reserved_in[i], 0, "on both sides");
    settlement.validate().unwrap();

    for _ in 0..ms(2200) { sim.step(&[]); }
    let settlement = sim.world().settlement().unwrap();
    assert_eq!(settlement.agents[target as usize].body.health, 1, "respawned");
    assert_eq!(settlement.depots[d].inventory.reserved_out[i], out);
    settlement.validate().unwrap();
}

/// The Respawn action refills health and keeps the player hurtable.
#[test]
fn respawning_refills_health_and_keeps_the_player_hurtable() {
    let mut config = scenario();
    config.agents = vec![player()];
    config.links.clear();
    config.weapons = vec![Weapon::Laser];
    config.player_health = 2;
    let world = World::new_settlement(9, &test_content(), &config, &[]).unwrap();
    let mut sim = Simulation::from_world(world, 2);
    let spawn = sim.world().settlement().unwrap().agents[0].body.position;
    sim.world_mut().settlement_mut().spawn = spawn;
    let (host, guest) = (PlayerId(0), PlayerId(1));
    sim.step(&[Command { player: guest, kind: CommandKind::Join }]);
    let settlement = sim.world().settlement().unwrap();
    let (shooter, target) = (settlement.avatar_of(host).unwrap(), settlement.avatar_of(guest).unwrap());
    let start = settlement.agents[shooter as usize].body.position;
    let to = start + FxVec2::new(Fx::ZERO, Fx::from_int(3));
    let shoot = |sim: &mut Simulation, target_at: FxVec2| {
        sim.world_mut().settlement_mut().agents[target as usize].body.position = target_at;
        sim.step(&[Command { player: host, kind: CommandKind::Attack {
            agent: shooter, weapon: Weapon::Laser,
            aim: FxVec2::new(Fx::ZERO, Fx::ONE), muzzle: [Fx::ZERO, fx(0.8), fx(0.25)],
        } }]);
        for _ in 0..ms(1500) { sim.step(&[]); }
    };

    shoot(&mut sim, to);
    assert_eq!(sim.world().settlement().unwrap().agents[target as usize].body.health, 1, "hurt");
    sim.step(&[Command { player: guest, kind: CommandKind::Respawn { agent: target } }]);
    let body = &sim.world().settlement().unwrap().agents[target as usize].body;
    assert_eq!((body.health, body.max_health), (2, 2), "the respawn refills health");

    shoot(&mut sim, to);
    assert_eq!(sim.world().settlement().unwrap().agents[target as usize].body.health, 1, "still hurtable");
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
    let route = arena.s().agents[arena.player].body.route.clone();
    assert!(!route.is_empty());
    arena.command(CommandKind::ReleaseDirectControl { agent });
    assert!(arena.s().agents[arena.player].carry.job.is_some());
    assert!(!arena.s().agents[arena.player].body.route.is_empty(), "the dock walk continues");
    arena.step(ms(6000));
    assert_eq!(arena.s().agents[arena.player].carry.inventory.counts[item as usize], 2, "the transfer completed");
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
    assert_eq!(super::haulers::footprint(player).radius, def.body.radius);
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
        windup: None,
        ready_at: 0,
    };
    let def = &s.characters[s.waves[brutes].character as usize];
    let (body, plated) = (def.body.radius, def.health.as_ref().unwrap().armor.as_ref().unwrap().radius);
    assert_eq!(s.threat_radius(&brute), plated);
    brute.armor = 0;
    assert_eq!(s.threat_radius(&brute), body);
    assert!(body < plated);
}

// Ghouls hitting players in passing (GHOUL_ATTACK_HANDOFF.md, task 3). The
// test ghouls inherit the base ghoul's `attack`.

/// The combat fixture with hurtable players.
fn ghoul_arena(player_health: u32) -> Arena {
    let mut config = combat_scenario(vec![]);
    config.player_health = player_health;
    Arena::new(&config, &[])
}

fn ghoul_attack(arena: &Arena) -> content::character::AttackDef {
    let s = arena.s();
    s.characters[s.waves[0].character as usize].attack.expect("ghouls attack")
}

/// This tick's windups: (threat, agent, landing tick).
fn windups(arena: &Arena) -> Vec<(u64, u16, u64)> {
    arena.s().events.iter().filter_map(|e| match *e {
        Event::ThreatWindup { threat, agent, lands } => Some((threat, agent, lands)),
        _ => None,
    }).collect()
}

/// This tick's swings: (threat, agent, hit, damage, killed).
fn swings(arena: &Arena) -> Vec<(u64, u16, bool, u32, bool)> {
    arena.s().events.iter().filter_map(|e| match *e {
        Event::ThreatSwing { threat, agent, hit, damage, killed } => Some((threat, agent, hit, damage, killed)),
        _ => None,
    }).collect()
}

impl Arena {
    fn place(&mut self, agent: usize, x: f64, y: f64) -> FxVec2 {
        let at = self.at(x, y);
        self.s_mut().agents[agent].body.position = at;
        at
    }

    fn health(&self, agent: usize) -> u32 {
        self.s().agents[agent].body.health
    }

    /// Steps until the threat's windup has landed, checking it stands
    /// still meanwhile. Returns the landing tick.
    fn until_swing(&mut self, id: u64) -> u64 {
        let lands = self.s().threat(id).unwrap().windup.expect("winding up").lands;
        let planted = self.s().threat(id).unwrap().position;
        while self.tick() < lands {
            self.step(1);
            assert_eq!(self.s().threat(id).unwrap().position, planted, "a threat winding up stands still");
            assert!(swings(self).is_empty());
        }
        self.step(1);
        lands
    }
}

#[test]
fn a_ghoul_winds_up_then_hits_and_pushes_a_player_in_reach() {
    let mut arena = ghoul_arena(3);
    let attack = ghoul_attack(&arena);
    let player = arena.player;
    let at = arena.place(player, 0.0, 0.0);
    arena.threat(100, 0.0, 0.7, 3, 0);
    arena.step(1);
    let start = arena.tick() - 1;
    assert_eq!(windups(&arena), vec![(100, player as u16, start + attack.windup)]);
    let lands = arena.until_swing(100);
    assert_eq!(lands, start + attack.windup);
    assert_eq!(swings(&arena), vec![(100, player as u16, true, attack.damage, false)]);
    let (velocity, health) = (arena.s().agents[player].body.velocity, arena.health(player));
    assert_eq!(health, 3 - attack.damage);
    assert!(velocity.y < fx(0.01) - attack.knockback && velocity.x.abs() < fx(0.01), "pushed away from the ghoul: {velocity:?}");
    let t = arena.s().threat(100).unwrap();
    assert!(t.windup.is_none());
    assert_eq!(t.ready_at, lands + 1 + attack.cooldown);
    arena.step(ms(300));
    let a = &arena.s().agents[player];
    assert!(a.body.position.y < at.y - fx(0.05), "the push moved the player: {:?}", a.body.position - at);
    assert_eq!(a.body.velocity, FxVec2::ZERO, "its own braking stopped the push");
}

#[test]
fn a_player_who_steps_out_of_reach_during_the_windup_is_not_hit() {
    let mut arena = ghoul_arena(3);
    let attack = ghoul_attack(&arena);
    let player = arena.player;
    arena.place(player, 0.0, 0.0);
    arena.threat(100, 0.0, 0.7, 3, 0);
    arena.step(1);
    assert_eq!(windups(&arena).len(), 1);
    let agent = player as u16;
    arena.command(CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, -Fx::ONE), run: false } });
    let lands = arena.until_swing(100);
    let gap = crate::nav::distance(arena.s().agents[player].body.position, arena.s().threat(100).unwrap().position);
    assert!(gap > attack.reach, "walked out of reach: {gap}");
    assert_eq!(swings(&arena), vec![(100, agent, false, 0, false)], "the swing lands and misses");
    assert_eq!(arena.health(player), 3);
    assert_eq!(arena.s().threat(100).unwrap().ready_at, lands + 1 + attack.cooldown, "a miss starts the cooldown too");
}

#[test]
fn a_ghoul_swings_at_the_nearest_player_and_the_lowest_index_on_a_tie() {
    let mut arena = ghoul_arena(3);
    arena.sim.step(&[Command { player: PlayerId(1), kind: CommandKind::Join }]);
    let (host, guest) = (arena.player, arena.s().avatar_of(PlayerId(1)).unwrap() as usize);
    assert!(host < guest);
    // Equally far, on either side of the ghoul's straight route.
    arena.place(host, -0.5, 0.0);
    arena.place(guest, 0.5, 0.0);
    arena.threat(100, 0.0, 0.3, 3, 0);
    arena.step(1);
    let tie = arena.s().threat(100).unwrap().windup.unwrap().agent;
    assert_eq!(tie, host as u16, "the lowest agent index wins a tie");
    assert_eq!(windups(&arena).len(), 1, "one windup per threat");

    arena.s_mut().threats.clear();
    arena.place(host, -0.6, 0.0);
    arena.place(guest, 0.3, 0.0);
    arena.threat(101, 0.0, 0.3, 3, 0);
    arena.step(1);
    assert_eq!(arena.s().threat(101).unwrap().windup.unwrap().agent, guest as u16, "the nearer player");
}

#[test]
fn a_ghoul_waits_out_its_cooldown_before_the_next_windup() {
    let mut arena = ghoul_arena(10);
    let attack = ghoul_attack(&arena);
    let player = arena.player;
    arena.place(player, 0.5, 0.0);
    arena.threat(100, 0.0, 0.25, 3, 0);
    arena.step(1);
    let lands = arena.until_swing(100);
    assert_eq!(swings(&arena).len(), 1);
    let next = lands + 1 + attack.cooldown;
    while arena.tick() < next {
        arena.step(1);
        assert!(windups(&arena).is_empty() && arena.s().threat(100).unwrap().windup.is_none(), "cooling down");
    }
    arena.step(1);
    assert_eq!(windups(&arena), vec![(100, player as u16, next + attack.windup)], "the next windup starts as the cooldown ends");
}

#[test]
fn ghouls_never_attack_players_without_health() {
    let mut arena = ghoul_arena(0);
    let player = arena.player;
    arena.place(player, 0.3, 0.0);
    arena.threat(100, 0.0, 0.7, 3, 0);
    for _ in 0..ms(1500) {
        arena.step(1);
        assert!(windups(&arena).is_empty() && swings(&arena).is_empty());
        assert!(arena.s().threat(100).unwrap().windup.is_none());
    }
    assert!(arena.y(100) < fx(0.0), "it walked past: {}", arena.y(100));
}

#[test]
fn a_ghoul_walks_on_along_its_route_after_the_swing() {
    let mut arena = ghoul_arena(10);
    let player = arena.player;
    arena.place(player, 0.6, 0.0);
    arena.threat(100, 0.0, 0.5, 3, 0);
    arena.step(1);
    arena.until_swing(100);
    let landed = arena.y(100);
    arena.step(ms(500));
    let walked = landed - arena.y(100);
    assert!(walked > fx(0.2), "walked {walked} in half a second after the swing");
    assert!((arena.s().threat(100).unwrap().position.x - arena.center.x).abs() < fx(0.01), "still on its route");
}

/// A killing hit goes through `hurt_player`: the player dies with no push,
/// and its transfer gives its reservation back.
#[test]
fn a_ghoul_hit_kills_through_the_laser_kill_path() {
    // No haulers, so only the transfer moves the forest's reservations.
    let mut config = combat_scenario(vec![]);
    config.player_health = 1;
    config.agents.retain(|a| a.controlled);
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    let player = arena.player;
    let at = arena.place(player, 0.0, 0.0);
    let s = arena.s();
    let (depot, item) = (s.depot_index("forest").unwrap(), s.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let out = s.depots[d].inventory.reserved_out[i];
    assert!(crate::nav::distance(at, s.depots[d].position) > fx(1.0), "far from the dock, so no unit moves");
    arena.command(CommandKind::Transfer { agent: player as u16, depot, item, units: 3, deposit: false });
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out + 3);
    arena.threat(100, 0.0, 0.7, 3, 0);
    arena.step(1);
    let lands = arena.until_swing(100);
    assert_eq!(swings(&arena), vec![(100, player as u16, true, 1, true)]);
    let a = &arena.s().agents[player];
    assert_eq!(a.body.health, 0);
    assert!(a.avatar().unwrap().respawn_at > lands);
    assert_eq!(a.body.velocity, FxVec2::ZERO, "the dead aren't pushed");
    assert!(a.carry.job.is_none());
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out, "death released the reservation");
    // No swings at the dead.
    for _ in 0..ms(1000) {
        arena.step(1);
        assert!(windups(&arena).is_empty());
    }
}

#[test]
fn a_stagger_during_the_windup_cancels_the_swing() {
    let mut arena = ghoul_arena(3);
    let player = arena.player;
    arena.place(player, 0.0, 0.0);
    // An ID whose sword roll isn't a critical on the tick it is struck.
    let struck = arena.tick() + 1;
    let id = (999..).find(|&id| combat_roll(struck, id, 0x53574f5244) % 4 != 0).unwrap();
    arena.threat(id, 0.0, 0.7, 3, 0);
    arena.step(1);
    let lands = arena.s().threat(id).unwrap().windup.unwrap().lands;
    assert_eq!(arena.tick(), struck);
    arena.ready();
    assert_eq!(arena.attack(Weapon::Sword), 1);
    let t = arena.s().threat(id).unwrap();
    assert!(t.windup.is_none(), "the stagger cancelled the windup");
    assert!(t.stagger_until > struck);
    while arena.tick() <= lands {
        arena.step(1);
        assert!(swings(&arena).is_empty());
        if arena.tick() < arena.s().threat(id).unwrap().stagger_until {
            assert!(windups(&arena).is_empty(), "no windup while staggered");
        }
    }
    assert_eq!(arena.health(player), 3);
}

/// Swings resolve the same on any thread count.
#[test]
fn ghoul_attacks_are_deterministic() {
    let run = |threads: usize| {
        let mut config = combat_scenario(vec![]);
        config.player_health = 4;
        let world = World::new_settlement(5, &test_content(), &config, &[]).unwrap();
        let center = world.terrain().center;
        let player = world.settlement().unwrap().agent_index("player").unwrap() as usize;
        let mut arena = Arena { sim: Simulation::from_world(world, threads), player, center };
        arena.sim.step(&[Command { player: PlayerId(1), kind: CommandKind::Join }]);
        let guest = arena.s().avatar_of(PlayerId(1)).unwrap() as usize;
        arena.place(player, -0.4, 0.0);
        arena.place(guest, 0.4, 0.0);
        for (id, x) in [(100, -0.6), (101, 0.0), (102, 0.6)] {
            arena.threat(id, x, 1.2, 3, 0);
        }
        let mut hashes = Vec::new();
        let mut hits = 0;
        for _ in 0..ms(4000) {
            hashes.push(arena.sim.step(&[]).total);
            hits += swings(&arena).iter().filter(|s| s.2).count();
        }
        (hashes, hits)
    };
    let (one, hits) = run(1);
    assert!(hits > 0, "the ghouls hit someone");
    assert_eq!(one, run(4).0);
}

// How high a ghoul reaches, and how its push moves a busy player.

/// A player hovering on the jetpack is too high for a ghoul to start a
/// swing at.
#[test]
fn a_ghoul_does_not_reach_a_hovering_player() {
    let mut arena = ghoul_arena(3);
    let player = arena.player;
    arena.place(player, 0.0, 0.0);
    arena.command(CommandKind::Jetpack { agent: player as u16, enabled: true });
    arena.step(ms(1000));
    assert!(arena.s().agents[player].avatar().unwrap().height > fx(1.7), "hovering: {}", arena.s().agents[player].avatar().unwrap().height);
    arena.threat(100, 0.0, 0.7, 3, 0);
    for _ in 0..ms(1000) {
        arena.step(1);
        assert!(windups(&arena).is_empty(), "no windup at a hovering player");
    }
    assert_eq!(arena.health(player), 3);
}

/// Reach is checked again when the swing lands: a player who lifts off
/// during the windup is missed.
#[test]
fn a_player_who_lifts_off_during_the_windup_is_missed() {
    let mut arena = ghoul_arena(3);
    let player = arena.player;
    arena.place(player, 0.0, 0.0);
    arena.threat(100, 0.0, 0.7, 3, 0);
    arena.step(1);
    assert_eq!(windups(&arena).len(), 1);
    arena.command(CommandKind::Jetpack { agent: player as u16, enabled: true });
    arena.until_swing(100);
    assert!(arena.s().agents[player].avatar().unwrap().height > super::combat::SWING_HEIGHT);
    assert_eq!(swings(&arena), vec![(100, player as u16, false, 0, false)]);
    assert_eq!(arena.health(player), 3);
}

/// A jump isn't high enough to dodge a swing.
#[test]
fn a_jump_does_not_dodge_a_ghoul() {
    let mut config = combat_scenario(vec![]);
    config.player_health = 3;
    config.agents.last_mut().unwrap().character = "exosuit_moonlit".into();
    let mut arena = Arena::new(&config, &[]);
    let player = arena.player;
    arena.place(player, 0.0, 0.0);
    arena.threat(100, 0.0, 0.7, 3, 0);
    arena.step(1);
    assert_eq!(windups(&arena).len(), 1);
    arena.command(CommandKind::Jump { agent: player as u16 });
    let lands = arena.s().threat(100).unwrap().windup.unwrap().lands;
    while arena.tick() < lands {
        arena.step(1);
    }
    let height = arena.s().agents[player].avatar().unwrap().height;
    assert!(height > fx(0.4), "near the jump's apex as the swing lands: {height}");
    arena.step(1);
    assert_eq!(swings(&arena), vec![(100, player as u16, true, 1, false)]);
}

/// How far a ghoul's hit pushes the player in the half second after it.
/// `busy` runs as the windup starts, with the tick it lands; it may step,
/// but not past that tick.
fn pushed(busy: impl FnOnce(&mut Arena, u64)) -> Fx {
    let mut arena = ghoul_arena(3);
    let player = arena.player;
    arena.place(player, 0.0, 0.0);
    arena.threat(100, 0.0, 0.7, 3, 0);
    arena.step(1);
    assert_eq!(windups(&arena).len(), 1);
    let lands = arena.s().threat(100).unwrap().windup.unwrap().lands;
    busy(&mut arena, lands);
    while arena.tick() < lands {
        arena.step(1);
    }
    let before = arena.s().agents[player].body.position;
    arena.step(1);
    assert_eq!(swings(&arena), vec![(100, player as u16, true, 1, false)]);
    assert_eq!(arena.s().agents[player].avatar().unwrap().dig, None, "a push ends a dig");
    arena.step(ms(500));
    let a = &arena.s().agents[player];
    assert_eq!((a.body.velocity, a.body.push), (FxVec2::ZERO, FxVec2::ZERO), "braked out");
    crate::nav::distance(before, a.body.position)
}

#[test]
fn a_ghoul_pushes_a_swinging_player_as_far_as_an_idle_one() {
    let idle = pushed(|_, _| {});
    assert!(idle > fx(0.08) && idle < fx(0.16), "an idle player moves about 0.14 m: {idle}");
    let swinging = pushed(|arena, lands| {
        // A sword swing away from the ghoul, still going when it lands.
        arena.step(lands - 2 - arena.tick());
        let agent = arena.player as u16;
        arena.command(CommandKind::Attack { agent, weapon: Weapon::Sword, aim: FxVec2::new(Fx::ZERO, -Fx::ONE), muzzle: [Fx::ZERO; 3] });
        let swing = arena.s().agents[agent as usize].avatar().unwrap().swing.clone().expect("swinging");
        assert!(swing.until > lands + 1, "the swing outlasts the hit");
    });
    assert!((swinging - idle).abs() < fx(0.02), "swinging {swinging}, idle {idle}");
}

#[test]
fn a_ghoul_push_ends_a_dig_and_moves_the_player() {
    let idle = pushed(|_, _| {});
    let digging = pushed(|arena, _| {
        let (_, terrain) = arena.sim.world_mut().settlement_and_terrain_mut();
        terrain.diggable = true;
        let agent = arena.player as u16;
        arena.command(CommandKind::DigDown { agent });
        assert!(arena.s().agents[agent as usize].avatar().unwrap().dig.is_some());
    });
    assert!((digging - idle).abs() < fx(0.02), "digging {digging}, idle {idle}");
}

/// Joiners each get a spot of their own around the spawn, by slot, skipping
/// spots blocked by cover, so eight players never stand inside each other
/// or a wall.
#[test]
fn joiners_spawn_apart_and_clear_of_cover() {
    let probe = Arena::combat(&[]);
    let spawn = probe.s().agents[probe.player].body.home - probe.center;
    // Cover on the first joiner's preferred spot, 1.2 m to +x.
    let rock = Disc { center: FxVec2::new(spawn.x + fx(1.2), spawn.y), radius: fx(0.5) };
    let mut arena = Arena::combat(&[rock]);
    for slot in 1..8 {
        arena.sim.step(&[Command { player: PlayerId(slot), kind: CommandKind::Join }]);
    }
    let s = arena.s();
    let terrain = arena.sim.world().terrain();
    let homes: Vec<FxVec2> = (0..8).map(|slot| s.agents[s.avatar_of(PlayerId(slot)).unwrap() as usize].body.home).collect();
    for (i, &a) in homes.iter().enumerate() {
        let agent = &s.agents[s.avatar_of(PlayerId(i as u32)).unwrap() as usize];
        assert_eq!(agent.body.position, a, "player {i} starts at home");
        assert!(terrain.clear(a, a, agent.radius()), "player {i} spawns inside cover");
        for (j, &b) in homes.iter().enumerate().skip(i + 1) {
            assert!((a - b).length() >= fx(1.0), "players {i} and {j} spawn {} m apart", (a - b).length());
        }
    }
    s.validate().unwrap();
}

/// A player who leaves takes their avatar with them. The avatars after it
/// move down an index, keep their owners, and still take their players'
/// commands, even ones that name the old index.
#[test]
fn leaving_removes_the_avatar_and_later_avatars_keep_their_players() {
    let mut arena = Arena::combat(&[]);
    for slot in 1..4 {
        arena.sim.step(&[Command { player: PlayerId(slot), kind: CommandKind::Join }]);
    }
    let s = arena.s();
    let agents = s.agents.len();
    let old = s.avatar_of(PlayerId(3)).unwrap();
    let home = s.agents[old as usize].body.home;

    arena.sim.step(&[Command { player: PlayerId(2), kind: CommandKind::Leave }]);
    let s = arena.s();
    assert_eq!(s.agents.len(), agents - 1);
    assert_eq!(s.avatar_of(PlayerId(2)), None);
    let now = s.avatar_of(PlayerId(3)).unwrap();
    assert_eq!(now, old - 1);
    assert_eq!(s.agents[now as usize].id, "player-3");
    assert_eq!(s.agents[now as usize].body.home, home);
    s.validate().unwrap();

    // Sent before the leave, so it names the old index.
    let intent = Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: false };
    arena.sim.step(&[Command { player: PlayerId(3), kind: CommandKind::MoveAvatar { agent: old, intent: intent.clone() } }]);
    assert_eq!(arena.s().agents[now as usize].avatar().unwrap().intent, Some(intent));

    // The host's avatar is every joiner's template and never leaves.
    arena.sim.step(&[Command { player: PlayerId(0), kind: CommandKind::Leave }]);
    assert!(arena.s().avatar_of(PlayerId(0)).is_some());
    // Coming back gives a fresh avatar at the same spot.
    arena.sim.step(&[Command { player: PlayerId(2), kind: CommandKind::Join }]);
    let s = arena.s();
    let back = s.avatar_of(PlayerId(2)).unwrap();
    assert_eq!(s.agents[back as usize].id, "player-2");
    s.validate().unwrap();
}

/// What pointed at the avatars after a leaving one moves with them; what
/// pointed at the leaving one goes. Units flying to or from the leaver land
/// at once, and what it carried is written off, so material still balances.
#[test]
fn leaving_moves_references_to_later_avatars_and_balances_material() {
    let mut config = combat_scenario(vec![]);
    config.agents = vec![player()];
    config.links.clear();
    let mut arena = Arena::new(&config, &[]);
    for slot in 1..3 {
        arena.sim.step(&[Command { player: PlayerId(slot), kind: CommandKind::Join }]);
    }
    let s = arena.s();
    let (leaver, stayer) = (s.avatar_of(PlayerId(1)).unwrap(), s.avatar_of(PlayerId(2)).unwrap());
    let (depot, item) = (s.depot_index("forest").unwrap(), s.item_index("wood").unwrap());
    let (d, i) = (depot as usize, item as usize);
    let out = s.depots[d].inventory.reserved_out[i];

    // The leaver carries a unit and has another flying to it, with a unit
    // still reserved; a unit flies to the stayer too.
    let take = |player, agent, units| Command { player: PlayerId(player), kind: CommandKind::Transfer { agent, depot, item, units, deposit: false } };
    arena.sim.step(&[take(1, leaver, 3), take(2, stayer, 1)]);
    assert_eq!(arena.s().depots[d].inventory.reserved_out[i], out + 4);
    let tick = arena.tick();
    arena.s_mut().carry_unit(tick, CarryUnit { agent: leaver });
    arena.step(ms(1000));
    assert_eq!(arena.s().agents[leaver as usize].carry.inventory.counts[i], 1);
    let tick = arena.tick();
    for agent in [leaver, stayer] {
        arena.s_mut().carry_unit(tick, CarryUnit { agent });
    }
    assert_eq!(arena.s().transfers.len(), 2);
    let s = arena.s_mut();
    s.dock_owner[d] = Some(stayer);
    s.piano_notes.push(super::piano::PianoNote { agent: stayer, piano: 0, pitch: 60, velocity: 90, serial: 1 });
    s.piano_notes.push(super::piano::PianoNote { agent: leaver, piano: 0, pitch: 62, velocity: 90, serial: 2 });
    arena.threat(900, 0.0, 3.0, 10, 0);
    let s = arena.s_mut();
    s.threats[0].windup = Some(super::combat::Windup { agent: stayer, lands: tick + 100 });
    s.validate().unwrap();

    // Directly, so this tick's stages don't redo what the test checks
    // (docks are resolved every tick).
    arena.s_mut().leave(PlayerId(1));
    let s = arena.s();
    let moved = s.avatar_of(PlayerId(2)).unwrap();
    assert_eq!(moved, stayer - 1);
    assert_eq!(s.dock_owner[d], Some(moved));
    assert_eq!(s.piano_notes.iter().map(|n| n.agent).collect::<Vec<_>>(), [moved]);
    assert_eq!(s.threats[0].windup.map(|w| w.agent), Some(moved));
    let flying: Vec<_> = s.transfers.values().map(|t| t.to).collect();
    assert_eq!(flying, [super::InventoryRef::Agent(moved)], "the stayer's unit still flies to it");
    assert_eq!(s.depots[d].inventory.reserved_out[i], out, "the leaver's reservation is released");
    assert_eq!(s.accounts[i].abandoned, 2, "its carried unit and the one flying to it");
    s.validate().unwrap();
    arena.step(ms(1000));
    assert_eq!(arena.s().agents[moved as usize].carry.inventory.counts[i], 1, "the stayer's unit landed");
}

/// Eight players walking, aiming and fighting ghouls, with one leaving and
/// another joining partway: the same on any thread count, material still
/// balances, and the tick time is printed for the eight-player handoff.
#[test]
fn eight_players_fight_and_come_and_go_deterministically() {
    let run = |threads: usize| {
        let mut config = combat_scenario(vec![]);
        config.player_health = 6;
        let world = World::new_settlement(7, &test_content(), &config, &[]).unwrap();
        let center = world.terrain().center;
        let player = world.settlement().unwrap().agent_index("player").unwrap() as usize;
        let mut arena = Arena { sim: Simulation::from_world(world, threads), player, center };
        for slot in 1..8 {
            arena.sim.step(&[Command { player: PlayerId(slot), kind: CommandKind::Join }]);
        }
        for id in 0..12 {
            arena.threat(100 + id, (id % 6) as f64 - 2.5, 3.0 + (id / 6) as f64, 3, 0);
        }
        let mut hashes = Vec::new();
        let started = std::time::Instant::now();
        let ticks = ms(5000);
        for tick in 0..ticks {
            let mut turn = Vec::new();
            for slot in 0..8u32 {
                let player = PlayerId(slot);
                let Some(agent) = arena.s().avatar_of(player) else { continue };
                let phase = (tick + slot as u64 * 13) % 90;
                let direction = FxVec2::new(fx(if phase < 45 { 0.6 } else { -0.6 }), fx(0.8));
                if phase % 30 == 0 {
                    turn.push(Command { player, kind: CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction, run: false } } });
                }
                turn.push(Command { player, kind: CommandKind::AimAvatar { agent, aim: direction.normalize_or_zero() } });
                if phase == 10 {
                    let weapon = [Weapon::Sword, Weapon::Spear, Weapon::Bow, Weapon::ArcCoil][slot as usize % 4];
                    turn.push(Command { player, kind: CommandKind::Attack { agent, weapon, aim: direction.normalize_or_zero(), muzzle: [Fx::ZERO, fx(0.6), fx(0.25)] } });
                }
            }
            if tick == ticks / 3 {
                turn.push(Command { player: PlayerId(3), kind: CommandKind::Leave });
            }
            if tick == ticks * 2 / 3 {
                turn.push(Command { player: PlayerId(3), kind: CommandKind::Join });
            }
            hashes.push(arena.sim.step(&turn).total);
            arena.s().validate().unwrap();
        }
        let per_tick = started.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        eprintln!("8 players, {threads} threads: {per_tick:.3} ms a tick (validation included)");
        assert_eq!(arena.s().agents.iter().filter(|a| a.avatar().is_some()).count(), 8);
        hashes
    };
    assert_eq!(run(1), run(4));
}

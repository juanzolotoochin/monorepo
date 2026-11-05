//! Health on the Sacred Tree map (`SACRED_TREE_BALLISTA.md`, B6, B7, B8):
//! sprites die and come back at the tree with their job, a building
//! shelters whoever is inside it, an enemy with nobody to chase breaks the
//! buildings in its way back into sites (putting their occupant out and
//! spilling what they held onto ground piles), the elder dies and respawns,
//! and the tree at 0 ends the run. Every test checks every invariant, the
//! balances among them, on every tick.

use content::scenario::{Scenario, Wave};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::{AgentIx, BuildingIx, Event, Goal, Hold, Quarry, Seat, Settlement, Slot, SpriteJob, Stage, Step};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const FERN: AgentIx = 2;
/// The prebuilt Wood Gatherer, west of the tree.
const GATHERER: BuildingIx = 0;
const BALLISTA_KIND: u16 = 2;
const CHASER: &str = "ghoul_sacred_tree";
/// The wave south of the tree: ghouls walking north to the trunk.
const SOUTH: ([f64; 2], [f64; 2]) = ([0.0, -7.5], [0.0, -1.8]);
/// Past the Wood Gatherer's slot side, to the tree's west: its route runs
/// within a ghoul's reach of the gatherer.
const WEST: ([f64; 2], [f64; 2]) = ([-7.5, 2.4], [-2.0, -0.8]);

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn point(p: [f64; 2]) -> [Fx; 2] {
    [fx(p[0]), fx(p[1])]
}

/// The scene with `count` Sacred Tree ghouls coming from `from` to `to`,
/// one a second from `first` seconds.
fn town((from, to): ([f64; 2], [f64; 2]), first: f64, count: u32) -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    // Only the enemies a test sends: none of the map's own waves.
    scenario.waves.clear();
    scenario.waves.push(Wave {
        id: "test".into(),
        position: point(from),
        target: point(to),
        character: CHASER.into(),
        first: fx(first),
        every: fx(1.0),
        count,
    });
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn settlement_mut(sim: &mut Simulation) -> &mut Settlement {
    sim.world_mut().settlement_mut()
}

/// A scenario point in the settlement's coordinates.
fn at(sim: &Simulation, p: [f64; 2]) -> FxVec2 {
    sim.world().terrain().center + FxVec2::new(fx(p[0]), fx(p[1]))
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    let s = settlement(sim);
    s.validate().unwrap();
    assert!(s.balances().values().all(|b| b.conserved));
}

/// Steps until `done`, at most `ticks` ticks.
fn until(sim: &mut Simulation, ticks: u64, mut done: impl FnMut(&Simulation) -> bool) {
    for _ in 0..ticks {
        if done(sim) {
            return;
        }
        step(sim, &[]);
    }
    panic!("not done after {ticks} ticks");
}

fn goal(sim: &Simulation) -> Option<Goal> {
    settlement(sim).threats.first().map(|t| t.goal)
}

fn events(sim: &Simulation) -> &[Event] {
    &settlement(sim).events
}

/// The ThreatSwing events of the last tick: (agent, hit, damage, killed).
fn swings(sim: &Simulation) -> Vec<(AgentIx, bool, u32, bool)> {
    events(sim)
        .iter()
        .filter_map(|e| match *e {
            Event::ThreatSwing { agent, hit, damage, killed, .. } => Some((agent, hit, damage, killed)),
            _ => None,
        })
        .collect()
}

/// Whether building `b` broke on the last tick.
fn broke(sim: &Simulation, b: BuildingIx) -> bool {
    events(sim).contains(&Event::BuildingBroken { building: b })
}

/// Units on the ground piles, by item.
fn on_the_ground(s: &Settlement) -> Vec<u32> {
    let lab = s.piles.as_ref().unwrap();
    (0..s.items.len()).map(|i| lab.piles.values().map(|p| p.inventory.count(i as u16)).sum()).collect()
}

/// Moves sprite `agent` to `p` (scenario meters), leaving its home beside
/// the tree, with its plan dropped: it plans again from there.
fn put(sim: &mut Simulation, agent: AgentIx, p: [f64; 2]) {
    let position = at(sim, p);
    let a = &mut settlement_mut(sim).agents[agent as usize];
    a.body.position = position;
    a.body.route.clear();
    let sprite = a.sprite_mut().unwrap();
    sprite.plan.clear();
    sprite.step = 0;
}

/// Walks the elder to building `b`'s slot spot and boards it.
fn possess(sim: &mut Simulation, b: BuildingIx) {
    let s = settlement(sim);
    let spot = s.buildings[b as usize].slot_position(s.building_kind(b));
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: spot, run: true } })]);
    until(sim, 30 * HZ, |sim| {
        let a = &settlement(sim).agents[ELDER as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
    step(sim, &[command(CommandKind::Board { agent: ELDER })]);
    until(sim, HZ, |sim| settlement(sim).seat(ELDER as usize) == Some(Seat::Building(b)));
}

/// Puts a built building of `kind` at `p` (scenario meters), as a
/// blueprint whose parts all went in. Returns its index.
fn build(sim: &mut Simulation, kind: u16, p: [f64; 2]) -> BuildingIx {
    let position = at(sim, p);
    step(sim, &[command(CommandKind::PlaceBlueprint { kind, position })]);
    let s = settlement_mut(sim);
    let b = (s.buildings.len() - 1) as BuildingIx;
    assert_eq!(s.buildings[b as usize].position, position, "the blueprint went down");
    s.buildings[b as usize].stage = Stage::Built;
    b
}

/// Puts `units` of `item` in building `b`'s output, as if produced.
fn stock(sim: &mut Simulation, b: BuildingIx, item: &str, units: u32) {
    let s = settlement_mut(sim);
    let item = s.item_index(item).unwrap();
    assert!(s.buildings[b as usize].output.add(item, units));
    s.accounts[item as usize].produced += units as u64;
}

/// A sprite waiting outside on the ghouls' way (its job's only building is
/// staffed) is chased and hit three times: it dies, its chaser gives up,
/// nothing swings at it or chases it while it is dead, and 15 s later it is
/// back at home beside the tree with its job. The slot has come free
/// meanwhile, so it plans again and takes it.
#[test]
fn a_sprite_dies_and_comes_back_at_the_tree_with_its_job() {
    let mut sim = town(SOUTH, 1.0, 1);
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: FERN, job: SpriteJob::Staff(0) })]);
    until(&mut sim, 20 * HZ, |sim| settlement(sim).agents[FERN as usize].sprite().unwrap().holds == Some(Hold::Inside(GATHERER)));
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(0) })]);
    put(&mut sim, ASH, [1.6, -4.5]);
    step(&mut sim, &[]);
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().plan, vec![Step::Wait], "the gatherer is staffed");
    let (home, max) = {
        let a = &settlement(&sim).agents[ASH as usize];
        (a.body.home, a.body.max_health)
    };
    assert_eq!(max, 3, "the sprite character's health");
    let mut hits = 0;
    until(&mut sim, 40 * HZ, |sim| {
        for (agent, hit, damage, killed) in swings(sim) {
            assert_eq!(agent, ASH);
            if hit {
                hits += 1;
                assert_eq!((damage, killed), (1, hits == 3));
            }
        }
        !settlement(sim).agents[ASH as usize].body.alive()
    });
    assert_eq!(hits, 3);
    let died = sim.world().tick() - 1;
    let a = &settlement(&sim).agents[ASH as usize];
    let sprite = a.sprite().unwrap();
    assert_eq!((sprite.holds, sprite.plan.is_empty(), sprite.job), (None, true, SpriteJob::Staff(0)));
    let (corpse, character) = (a.body.position, a.character);
    // The slot comes free while it is dead.
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: FERN, job: SpriteJob::Idle })]);
    let respawn = settlement(&sim).characters[character as usize].sprite.unwrap().respawn;
    assert_eq!(respawn, 15 * HZ);
    let mut returned = false;
    until(&mut sim, 20 * HZ, |sim| {
        let s = settlement(sim);
        let a = &s.agents[ASH as usize];
        returned = s.events.contains(&Event::SpriteReturned { agent: ASH });
        if !returned {
            assert!(!a.body.alive());
            assert_eq!(a.body.position, corpse, "a dead sprite lies still");
            assert!(swings(sim).is_empty(), "nothing swings at a dead sprite");
            assert_ne!(goal(sim), Some(Goal::Chase(Quarry::Agent(ASH))), "nor chases it");
        }
        returned
    });
    assert_eq!(sim.world().tick() - 1, died + respawn, "back after its respawn delay");
    let a = &settlement(&sim).agents[ASH as usize];
    assert_eq!((a.body.position, a.body.health), (home, max), "at home beside the tree, healed");
    assert_eq!(a.sprite().unwrap().job, SpriteJob::Staff(0), "with its job");
    until(&mut sim, 20 * HZ, |sim| settlement(sim).agents[ASH as usize].sprite().unwrap().holds == Some(Hold::Inside(GATHERER)));
}

/// A sprite working in the Wood Gatherer is never hit while a ghoul
/// breaks the building around it (B6, B8). At 0 the gatherer is a site at
/// its first part again, the sprite is put out beside it and plans again
/// (no other gatherer: it waits), and the wood in the output lies on a
/// ground pile.
#[test]
fn a_ghoul_breaks_a_staffed_gatherer_without_hitting_its_sprite() {
    let mut sim = town(WEST, 25.0, 1);
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(0) })]);
    until(&mut sim, 20 * HZ, |sim| settlement(sim).agents[ASH as usize].sprite().unwrap().holds == Some(Hold::Inside(GATHERER)));
    stock(&mut sim, GATHERER, "wood", 3);
    let wood = settlement(&sim).item_index("wood").unwrap() as usize;
    let full = settlement(&sim).building_kind(GATHERER).health;
    assert_eq!(full, 10);
    let mut output = 0;
    let mut struck = 0;
    until(&mut sim, 90 * HZ, |sim| {
        let s = settlement(sim);
        assert!(swings(sim).iter().all(|&(agent, ..)| agent != ASH), "the building shelters its sprite");
        assert_eq!(s.agents[ASH as usize].body.health, s.agents[ASH as usize].body.max_health);
        for e in &s.events {
            if let Event::BuildingStruck { building, hit: true, damage, .. } = *e {
                assert_eq!((building, damage), (GATHERER, 1));
                struck += 1;
            }
        }
        if broke(sim, GATHERER) {
            return true;
        }
        output = s.buildings[GATHERER as usize].output.total();
        false
    });
    assert_eq!(struck, full, "one point a hit");
    let s = settlement(&sim);
    let b = &s.buildings[GATHERER as usize];
    assert_eq!(b.site().map(|(part, materials)| (part, materials.holds_any())), Some((0, false)), "a bare site again");
    assert_eq!((b.slot, b.health, b.output.total()), (Slot::Empty, full, 0));
    let a = &s.agents[ASH as usize];
    let sprite = a.sprite().unwrap();
    assert_eq!(sprite.holds, None, "put out");
    assert!((a.body.position - b.position).length() > s.building_kind(GATHERER).radius, "beside the building");
    let produced = s.events.contains(&Event::BuildingProduced { building: GATHERER }) as u32;
    assert_eq!(on_the_ground(s)[wood], output + produced, "the output spilled");
    assert!(output >= 3);
    step(&mut sim, &[]);
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().plan, vec![Step::Wait], "no gatherer to staff: it waits");
}

/// An elder possessing a stocked ballista on the ghouls' way is never hit
/// while the ballista is broken around them. At 0 they are put out beside
/// it with their reticle turned back into a facing, and the ammo lies on a
/// ground pile.
#[test]
fn a_ghoul_breaks_a_possessed_ballista_and_puts_the_elder_out() {
    let mut sim = town(SOUTH, 30.0, 1);
    let ballista = build(&mut sim, BALLISTA_KIND, [1.2, -4.5]);
    stock(&mut sim, ballista, "stone", 6);
    let stone = settlement(&sim).item_index("stone").unwrap() as usize;
    possess(&mut sim, ballista);
    settlement_mut(&mut sim).agents[ELDER as usize].avatar_mut().unwrap().aim = Some(FxVec2::new(fx(0.0), fx(2.0)));
    let max = settlement(&sim).agents[ELDER as usize].body.max_health;
    assert_eq!(max, 6, "the map's player health");
    until(&mut sim, 90 * HZ, |sim| {
        let s = settlement(sim);
        assert!(swings(sim).is_empty(), "nothing swings at the seated elder");
        assert_eq!(s.agents[ELDER as usize].body.health, max);
        broke(sim, ballista)
    });
    let s = settlement(&sim);
    let b = &s.buildings[ballista as usize];
    assert_eq!((b.site().map(|(part, _)| part), b.slot, b.output.total()), (Some(0), Slot::Empty, 0));
    assert_eq!(b.turret, Default::default());
    let a = &s.agents[ELDER as usize];
    let avatar = a.avatar().unwrap();
    assert_eq!(avatar.seat, None, "put out");
    assert_eq!(avatar.aim, Some(FxVec2::new(fx(0.0), fx(1.0))), "the reticle back to a facing");
    assert_eq!(Some(a.body.position), s.leave_spot(ballista, a.radius(), sim.world().terrain()), "where leaving puts them");
    assert_eq!(on_the_ground(s)[stone], 6, "the ammo spilled");
    assert!(s.fire_check(ELDER as usize, sim.world().tick()).is_err(), "no ballista to fire");
}

/// A building put down on a ghoul's planned way, with nobody to chase
/// about: the ghoul plans its way afresh round the new site (M5.6), but
/// passes within reach of it and attacks it. Once it is a bare site, with
/// nothing left to lose, the ghoul walks on round it and breaches at the
/// tree.
#[test]
fn a_ghoul_walks_round_a_building_put_on_its_way_and_breaks_it_in_passing() {
    let mut sim = town(SOUTH, 1.0, 1);
    until(&mut sim, 3 * HZ, |sim| !settlement(sim).threats.is_empty());
    let site = build(&mut sim, 0, [0.0, -4.0]);
    let s = settlement(&sim);
    let (b, t) = (&s.buildings[site as usize], &s.threats[0]);
    let radius = s.building_kind(site).radius;
    let legs = std::iter::once(t.position).chain(t.route.iter().copied()).collect::<Vec<_>>();
    assert!(legs.windows(2).all(|leg| crate::terrain::segment_disc(leg[0], leg[1], b.position, radius).is_none()), "its route goes round the site");
    let reach = s.characters[t.character as usize].attack.unwrap().reach;
    until(&mut sim, 20 * HZ, |sim| goal(sim) == Some(Goal::Chase(Quarry::Building(site))));
    let s = settlement(&sim);
    assert!(s.building_gap(s.threats[0].position, site) <= reach, "attacked in reach, in passing");
    until(&mut sim, 60 * HZ, |sim| broke(sim, site));
    step(&mut sim, &[]);
    assert_eq!(goal(&sim), Some(Goal::Tree), "nothing left to lose");
    until(&mut sim, 60 * HZ, |sim| {
        assert_ne!(goal(sim), Some(Goal::Chase(Quarry::Building(site))), "a bare site is left alone");
        settlement(sim).combat.breaches == 1
    });
}

/// A site's delivered materials spill onto ground piles when it breaks,
/// one pile per item, and units on their way to it land first: the
/// balances hold.
#[test]
fn a_broken_site_spills_its_materials() {
    let mut sim = town(SOUTH, 600.0, 1);
    let position = at(&sim, [4.0, -3.0]);
    step(&mut sim, &[command(CommandKind::PlaceBlueprint { kind: 0, position })]);
    let s = settlement_mut(&mut sim);
    let b = (s.buildings.len() - 1) as BuildingIx;
    let (wood, stone) = (s.item_index("wood").unwrap(), s.item_index("stone").unwrap());
    let Stage::Site { part, materials } = &mut s.buildings[b as usize].stage else { panic!("a site") };
    *part = 1;
    assert!(materials.add(wood, 2) && materials.add(stone, 1));
    s.accounts[wood as usize].produced += 2;
    s.accounts[stone as usize].produced += 1;
    assert!(s.breakable(b));
    let terrain = sim.world().terrain().clone();
    let s = settlement_mut(&mut sim);
    s.break_building(b, &terrain);
    assert_eq!(s.buildings[b as usize].site().map(|(part, m)| (part, m.holds_any())), Some((0, false)));
    assert!(!s.breakable(b), "a bare site has nothing left to lose");
    let ground = on_the_ground(s);
    assert_eq!((ground[wood as usize], ground[stone as usize]), (2, 1));
    assert_eq!(s.piles.as_ref().unwrap().piles.len(), 2, "one pile per item");
    s.validate().unwrap();
    step(&mut sim, &[]);
}

/// The elder, standing on the ghouls' way, is chased and hit until they
/// die (the map's player health, 6), then respawns at home with full
/// health.
#[test]
fn the_elder_dies_and_respawns() {
    let mut sim = town(SOUTH, 6.0, 1);
    let near = at(&sim, [1.4, -4.0]);
    step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: near, run: false } })]);
    let home = settlement(&sim).agents[ELDER as usize].body.home;
    let mut hits = 0;
    until(&mut sim, 60 * HZ, |sim| {
        for (agent, hit, damage, killed) in swings(sim) {
            assert_eq!(agent, ELDER);
            if hit {
                hits += 1;
                assert_eq!((damage, killed), (1, hits == 6));
            }
        }
        !settlement(sim).agents[ELDER as usize].body.alive()
    });
    until(&mut sim, 5 * HZ, |sim| settlement(sim).agents[ELDER as usize].body.alive());
    let a = &settlement(&sim).agents[ELDER as usize];
    assert_eq!((a.body.position, a.body.health), (home, a.body.max_health));
}

/// Breaches take the tree's health, 20 on this map; the one that takes the
/// last ends the run (B7): the tree falls once, and no more enemies come.
#[test]
fn the_tree_falls_at_zero_and_the_run_ends() {
    let mut sim = town(SOUTH, 1.0, 30);
    assert_eq!((settlement(&sim).base_health, settlement(&sim).health), (20, 20));
    assert!(!settlement(&sim).tree_fallen());
    settlement_mut(&mut sim).health = 1;
    let mut fell = 0;
    until(&mut sim, 40 * HZ, |sim| {
        fell += events(sim).iter().filter(|e| **e == Event::TreeFell).count();
        settlement(sim).combat.breaches == 1
    });
    assert_eq!(fell, 1);
    let s = settlement(&sim);
    assert!(s.tree_fallen());
    assert_eq!(s.health, 0);
    let spawned = s.waves[0].spawned;
    assert!(spawned < 30, "the wave was still coming");
    for _ in 0..10 * HZ {
        step(&mut sim, &[]);
        assert!(!events(&sim).contains(&Event::TreeFell), "it falls once");
    }
    assert_eq!(settlement(&sim).waves[0].spawned, spawned, "no more enemies come");
}

//! Sprites: the Sacred Tree's four sprites take jobs, claim the Wood
//! Gatherer's slot, walk there and work it; the others wait and take it
//! when it frees up (A2). Every test checks every invariant on every tick.

use content::scenario::{Building, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::haulers::{footprint, RETRY_TICKS};
use super::{AgentIx, BuildingIx, Hold, Occupant, Settlement, Slot, SpriteJob, Step};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
/// The sprites, by agent index: after the elder (0), in scenario order.
const ASH: AgentIx = 1;
const FERN: AgentIx = 2;
const MOSS: AgentIx = 3;
const REED: AgentIx = 4;
const WOOD_GATHERER: SpriteJob = SpriteJob::Staff(0);

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn load(scenario: &Scenario) -> Result<World, String> {
    // The tree's trunk, as the scene's architecture blocks it.
    World::new_settlement(1, &Content::builtin(), scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }])
}

fn sim_of(scenario: &Scenario) -> Simulation {
    Simulation::from_world(load(scenario).unwrap(), 1)
}

fn sacred_tree() -> Simulation {
    sim_of(&Scenario::parse(SACRED_TREE).unwrap())
}

/// The scene with a second Wood Gatherer on the clearing's east side.
fn two_gatherers() -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.buildings.push(Building { kind: "wood_gatherer".into(), position: [fx(4.6), fx(-2.2)] });
    sim_of(&scenario)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn holds(sim: &Simulation, sprite: AgentIx) -> Option<Hold> {
    settlement(sim).agents[sprite as usize].sprite().unwrap().holds
}

fn step_of(sim: &Simulation, sprite: AgentIx) -> Option<Step> {
    settlement(sim).agents[sprite as usize].sprite().unwrap().current()
}

fn slot(sim: &Simulation, building: BuildingIx) -> Slot {
    settlement(sim).buildings[building as usize].slot
}

fn wood(sim: &Simulation) -> u32 {
    let s = settlement(sim);
    s.buildings[0].output.count(s.item_index("wood").unwrap())
}

fn job(agent: AgentIx, job: SpriteJob) -> Command {
    Command { player: PlayerId(0), kind: CommandKind::SetSpriteJob { agent, job } }
}

/// Steps one tick with `turn`, checking every invariant after it.
fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

fn steps(sim: &mut Simulation, ticks: u64) {
    for _ in 0..ticks {
        step(sim, &[]);
    }
}

/// Steps until `done`, at most `ticks` ticks; returns how many it took.
fn until(sim: &mut Simulation, ticks: u64, done: impl Fn(&Simulation) -> bool) -> u64 {
    for n in 0..ticks {
        if done(sim) {
            return n;
        }
        step(sim, &[]);
    }
    panic!("not done after {ticks} ticks");
}

#[test]
fn the_scene_makes_its_sprites_after_its_agents_idle_at_home() {
    let mut sim = sacred_tree();
    let s = settlement(&sim);
    let ids: Vec<&str> = s.agents.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["elder", "sprite_ash", "sprite_fern", "sprite_moss", "sprite_reed"]);
    for a in &s.agents[1..] {
        let sprite = a.sprite().unwrap();
        assert_eq!((sprite.job, sprite.holds), (SpriteJob::Idle, None));
        assert_eq!((a.carry.inventory.capacity, a.body.health, a.body.max_health), (0, 3, 3), "no cargo, the sprite character's health");
    }
    // Idle sprites stay home.
    steps(&mut sim, 5 * HZ);
    let s = settlement(&sim);
    assert!(s.agents[1..].iter().all(|a| a.body.position == a.body.home));
    assert_eq!(s.buildings[0].slot, Slot::Empty);
}

#[test]
fn a_sprite_needs_a_sprite_character() {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.sprites[0].character = "elder".into();
    let error = load(&scenario).err().expect("refused");
    assert!(error.contains("sprite sprite_ash") && error.contains("needs a Sprite one"), "{error}");
}

#[test]
fn a_job_sends_a_sprite_to_claim_enter_and_work_the_gatherer() {
    let mut sim = sacred_tree();
    step(&mut sim, &[job(ASH, WOOD_GATHERER)]);
    // Claimed on the tick the job came: stage 3 asked, stage 5 granted.
    assert_eq!(slot(&sim, 0), Slot::Claimed(ASH));
    assert_eq!(holds(&sim, ASH), Some(Hold::Claimed(0)));
    let spot = {
        let s = settlement(&sim);
        s.buildings[0].slot_position(s.building_kind(0))
    };
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().plan, [Step::Claim(0), Step::GoTo(spot), Step::Enter(0)]);
    // A claim alone produces nothing while the sprite walks.
    let walked = until(&mut sim, 20 * HZ, |sim| {
        if holds(sim, ASH) == Some(Hold::Claimed(0)) {
            assert_eq!((wood(sim), settlement(sim).buildings[0].progress), (0, Fx::ZERO));
        }
        holds(sim, ASH) == Some(Hold::Inside(0))
    });
    assert!(walked > 2 * HZ, "it walked there, in {walked} ticks");
    assert_eq!(slot(&sim, 0), Slot::Occupied(Occupant::Sprite(ASH)));
    assert_eq!(step_of(&sim, ASH), Some(Step::Enter(0)));
    let a = &settlement(&sim).agents[ASH as usize];
    assert!(crate::nav::distance(a.body.position, spot) <= Fx::from_ratio(1, 10));
    // Inside, it leaves the steering footprints.
    assert!(!footprint(a).solid);
    // One unit per period, the first a period after it stepped in.
    steps(&mut sim, 5 * HZ);
    assert_eq!(wood(&sim), 1);
    let position = settlement(&sim).agents[ASH as usize].body.position;
    steps(&mut sim, 10 * HZ);
    assert_eq!(wood(&sim), 3);
    assert_eq!(settlement(&sim).agents[ASH as usize].body.position, position, "it stays at the slot");
}

#[test]
fn a_second_sprite_waits_and_takes_the_slot_when_the_first_goes_idle() {
    let mut sim = sacred_tree();
    step(&mut sim, &[job(FERN, WOOD_GATHERER), job(ASH, WOOD_GATHERER)]);
    until(&mut sim, 20 * HZ, |sim| holds(sim, ASH) == Some(Hold::Inside(0)));
    // Fern lost the claim and waits where it stood, visibly.
    let fern = settlement(&sim).agents[FERN as usize].body.position;
    for _ in 0..10 * HZ {
        step(&mut sim, &[]);
        assert_eq!(step_of(&sim, FERN), Some(Step::Wait));
        assert_eq!(holds(&sim, FERN), None);
    }
    assert!(crate::nav::distance(settlement(&sim).agents[FERN as usize].body.position, fern) < Fx::from_ratio(1, 2));
    // Ash goes idle: the slot is free at once, Ash walks home, and Fern
    // takes the slot.
    step(&mut sim, &[job(ASH, SpriteJob::Idle)]);
    assert_eq!(holds(&sim, ASH), None);
    assert_ne!(slot(&sim, 0), Slot::Occupied(Occupant::Sprite(ASH)));
    let waited = until(&mut sim, RETRY_TICKS + 1, |sim| holds(sim, FERN).is_some());
    assert!(waited <= RETRY_TICKS);
    until(&mut sim, 20 * HZ, |sim| holds(sim, FERN) == Some(Hold::Inside(0)));
    until(&mut sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[ASH as usize];
        a.body.route.is_empty() && crate::nav::distance(a.body.position, a.body.home) <= Fx::from_ratio(1, 10)
    });
    step(&mut sim, &[]);
    assert_eq!(step_of(&sim, ASH), None, "home: the idle plan is done");
    let before = wood(&sim);
    steps(&mut sim, 5 * HZ);
    assert_eq!(wood(&sim), before + 1);
}

#[test]
fn a_job_change_mid_walk_gives_up_the_claim() {
    let mut sim = sacred_tree();
    step(&mut sim, &[job(ASH, WOOD_GATHERER)]);
    steps(&mut sim, HZ);
    assert_eq!(holds(&sim, ASH), Some(Hold::Claimed(0)));
    assert!(!settlement(&sim).agents[ASH as usize].body.route.is_empty(), "walking");
    // A kind with none built: it gives up the claim and waits.
    step(&mut sim, &[job(ASH, SpriteJob::Staff(1))]);
    assert_eq!((slot(&sim, 0), holds(&sim, ASH)), (Slot::Empty, None));
    assert_eq!(step_of(&sim, ASH), Some(Step::Wait));
    // And another sprite can take the slot.
    step(&mut sim, &[job(REED, WOOD_GATHERER)]);
    assert_eq!(slot(&sim, 0), Slot::Claimed(REED));
    // The same job again changes nothing.
    step(&mut sim, &[job(REED, WOOD_GATHERER)]);
    assert_eq!(slot(&sim, 0), Slot::Claimed(REED));
}

#[test]
fn a_claim_race_on_one_tick_goes_to_the_lowest_index() {
    let mut sim = sacred_tree();
    // Moss's command goes first; Fern still wins.
    step(&mut sim, &[job(MOSS, WOOD_GATHERER), job(REED, WOOD_GATHERER), job(FERN, WOOD_GATHERER)]);
    assert_eq!(slot(&sim, 0), Slot::Claimed(FERN));
    // The others find the slot taken on their next step, and wait.
    step(&mut sim, &[]);
    assert_eq!((step_of(&sim, MOSS), step_of(&sim, REED)), (Some(Step::Wait), Some(Step::Wait)));
}

#[test]
fn bad_jobs_change_nothing() {
    let mut sim = sacred_tree();
    let before = settlement(&sim).agents.clone();
    // Not a sprite, no such sprite, no such kind.
    step(&mut sim, &[job(0, WOOD_GATHERER), job(9, WOOD_GATHERER), job(ASH, SpriteJob::Staff(7))]);
    let s = settlement(&sim);
    assert_eq!(s.agents.iter().map(|a| a.sprite().map(|s| s.job)).collect::<Vec<_>>(), before.iter().map(|a| a.sprite().map(|s| s.job)).collect::<Vec<_>>());
    assert_eq!(s.buildings[0].slot, Slot::Empty);
}

/// Jobs change every few seconds over two buildings and four sprites: the
/// slots and the sprites always agree (`validate`), and no sprite waits
/// longer than a retry while a slot of its kind is free.
#[test]
fn no_sprite_keeps_waiting_while_a_matching_slot_is_free() {
    let mut sim = two_gatherers();
    let sprites = [ASH, FERN, MOSS, REED];
    step(&mut sim, &sprites.map(|s| job(s, WOOD_GATHERER)));
    let mut waiting = [0u64; 4];
    let mut entries = 0;
    for tick in 1..90 * HZ {
        // Every 6 s one sprite (in turn) goes idle or back to work.
        let turn: Vec<Command> = if tick % (6 * HZ) == 0 {
            let phase = (tick / (6 * HZ)) as usize;
            let sprite = sprites[phase % 4];
            let idle = (phase / 4) % 2 == 0;
            vec![job(sprite, if idle { SpriteJob::Idle } else { WOOD_GATHERER })]
        } else {
            Vec::new()
        };
        let inside_before = sprites.iter().filter(|&&s| matches!(holds(&sim, s), Some(Hold::Inside(_)))).count();
        step(&mut sim, &turn);
        let s = settlement(&sim);
        entries += sprites.iter().filter(|&&a| matches!(holds(&sim, a), Some(Hold::Inside(_)))).count().saturating_sub(inside_before);
        let free = s.buildings.iter().any(|b| b.built() && b.slot == Slot::Empty);
        for (i, &a) in sprites.iter().enumerate() {
            let sprite = s.agents[a as usize].sprite().unwrap();
            waiting[i] = if sprite.current() == Some(Step::Wait) && sprite.job == WOOD_GATHERER && free { waiting[i] + 1 } else { 0 };
            assert!(waiting[i] <= RETRY_TICKS + 1, "{} waited {} ticks with a slot free at tick {tick}", s.agents[a as usize].id, waiting[i]);
        }
        // Never two in one slot: every slot names one agent, and each sprite
        // holds one slot (`validate`); here, the inside count fits.
        assert!(sprites.iter().filter(|&&a| matches!(holds(&sim, a), Some(Hold::Inside(_)))).count() <= 2);
    }
    assert!(entries >= 6, "sprites came and went: {entries} entries");
}

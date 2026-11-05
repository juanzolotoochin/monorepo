//! The stability soak (A8, handoff "Tests"): a town played for minutes in
//! cycles of reassigning every sprite, possessing a building (Burst when
//! ready) and leaving it, carrying an output's units, dropping them and
//! picking them back up, and putting up new buildings. Every tick checks
//! every invariant (`validate`, conservation and pile reservations among
//! them), and that no sprite waits longer than a retry while a building of
//! its job's kind has a free slot.

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::haulers::RETRY_TICKS;
use super::{AgentIx, BuildingIx, BuildingKindIx, DepotIx, Event, InteractTarget, Interaction, ItemIx, Seat, Settlement, Slot, SpriteJob, Step};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
/// The sprites, by agent index: after the elder, in scenario order.
const SPRITES: [AgentIx; 4] = [1, 2, 3, 4];
const WOOD: ItemIx = 0;
const STONE: ItemIx = 1;
const TREE: [DepotIx; 2] = [0, 1];
const WOOD_GATHERER: BuildingKindIx = 0;
const STONE_GATHERER: BuildingKindIx = 1;

/// The scene, its tree's trunk blocking the ground as the architecture does.
fn sacred_tree() -> Simulation {
    let scenario = Scenario::parse(SACRED_TREE).unwrap();
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn interact(target: InteractTarget) -> Command {
    command(CommandKind::Interact { agent: ELDER, target })
}

/// The town under watch: every step validates it and counts how long each
/// sprite has waited while a slot of its kind stood free.
struct Soak {
    sim: Simulation,
    waiting: [u64; 4],
    longest: u64,
}

impl Soak {
    fn new() -> Self {
        Soak { sim: sacred_tree(), waiting: [0; 4], longest: 0 }
    }

    fn s(&self) -> &Settlement {
        self.sim.world().settlement().unwrap()
    }

    fn step(&mut self, turn: &[Command]) {
        self.sim.step(turn);
        let s = self.sim.world().settlement().unwrap();
        s.validate().unwrap();
        assert!(s.balances().values().all(|b| b.conserved), "conserved at tick {}", self.sim.world().tick());
        let free = |kind: BuildingKindIx| s.buildings.iter().any(|b| b.kind == kind && b.built() && b.slot == Slot::Empty);
        for (i, &a) in SPRITES.iter().enumerate() {
            let sprite = s.agents[a as usize].sprite().unwrap();
            let stranded = matches!(sprite.job, SpriteJob::Staff(kind) if free(kind)) && sprite.current() == Some(Step::Wait);
            self.waiting[i] = if stranded { self.waiting[i] + 1 } else { 0 };
            self.longest = self.longest.max(self.waiting[i]);
            assert!(self.waiting[i] <= RETRY_TICKS + 1, "{} waited {} ticks with a slot free at tick {}", s.agents[a as usize].id, self.waiting[i], self.sim.world().tick());
        }
    }

    fn steps(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.step(&[]);
        }
    }

    fn until(&mut self, ticks: u64, what: &str, done: impl Fn(&Settlement) -> bool) {
        for _ in 0..ticks {
            if done(self.s()) {
                return;
            }
            self.step(&[]);
        }
        panic!("{what}: not done after {ticks} ticks, at tick {}", self.sim.world().tick());
    }

    /// The elder's carry job and the units in the air are done.
    fn settle(&mut self) {
        self.until(10 * HZ, "settled", |s| s.agents[ELDER as usize].carry.job.is_none() && s.transfers.is_empty());
    }

    fn walk_to(&mut self, target: FxVec2) {
        self.step(&[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } })]);
        self.until(20 * HZ, "walked", |s| {
            let a = &s.agents[ELDER as usize];
            a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
        });
    }

    /// Where the elder stands to use building `b`'s output (or site).
    fn walk_to_building(&mut self, b: BuildingIx) {
        let building = &self.s().buildings[b as usize];
        let spot = building.output_position + (building.output_position - building.position).normalize_or_zero() * Fx::from_ratio(2, 5);
        self.walk_to(spot);
    }

    fn target(&self) -> Option<(InteractTarget, Interaction)> {
        let world = self.sim.world();
        self.s().interact_target(ELDER, world.tick(), world.terrain())
    }

    /// Interacts with `target` (in reach and usable, the sim checks) and
    /// waits for any units to land.
    fn use_target(&mut self, target: InteractTarget) {
        self.step(&[interact(target)]);
        let refused = self.s().events.iter().any(|e| matches!(e, Event::BundleRefused { agent: ELDER, .. }));
        assert!(!refused, "{target:?} refused: {:?}", self.s().events);
        self.settle();
    }

    fn cargo(&self) -> u32 {
        self.s().agents[ELDER as usize].carry.inventory.total()
    }

    /// Empties the bundle into the tree's piles.
    fn empty_bundle(&mut self) {
        for (item, depot) in [(WOOD, TREE[0]), (STONE, TREE[1])] {
            if self.s().agents[ELDER as usize].carry.inventory.count(item) > 0 {
                let dock = self.s().depots[depot as usize].position;
                self.walk_to(dock);
                self.use_target(InteractTarget::Depot(depot));
            }
        }
        assert_eq!(self.cargo(), 0);
    }

    /// Possesses building `b` with interact from beside it (a staffed one
    /// too: its sprite is put out), Bursts if the cooldown allows, works a
    /// while and leaves with interact. Returns whether it Burst.
    fn possess(&mut self, b: BuildingIx, seconds: u64) -> bool {
        let to = self.s().leave_spot(b, self.s().agents[ELDER as usize].radius(), self.sim.world().terrain()).unwrap();
        self.walk_to(to);
        self.step(&[interact(InteractTarget::Slot(b))]);
        assert_eq!(self.s().seat(ELDER as usize), Some(Seat::Building(b)), "possessed {b}");
        let burst = self.s().burst_check(ELDER as usize, self.sim.world().tick()).is_ok();
        if burst {
            self.step(&[command(CommandKind::Burst { agent: ELDER })]);
            assert!(self.s().events.iter().any(|e| matches!(e, Event::BuildingBurst { .. })), "burst");
        }
        self.steps(seconds * HZ);
        self.step(&[interact(InteractTarget::Slot(b))]);
        assert_eq!(self.s().seat(ELDER as usize), None, "left {b}");
        burst
    }

    /// Carries up to a bundle of building `b`'s output out to the first of
    /// `spots` with room to drop it (the town grows over them), drops it
    /// there, picks it back up and puts it in the tree.
    fn carry_and_drop(&mut self, b: BuildingIx, spots: &[FxVec2]) -> bool {
        if self.s().buildings[b as usize].output.total() == 0 {
            return false;
        }
        self.walk_to_building(b);
        self.use_target(InteractTarget::Output(b));
        let room = spots.iter().any(|&spot| {
            self.walk_to(spot);
            let world = self.sim.world();
            self.s().drop_plan(ELDER, world.tick(), world.terrain()).is_ok()
        });
        assert!(room, "room to drop somewhere");
        self.step(&[command(CommandKind::Drop { agent: ELDER })]);
        assert!(!self.s().events.iter().any(|e| matches!(e, Event::BundleRefused { agent: ELDER, .. })), "drop refused: {:?}", self.s().events);
        self.settle();
        assert_eq!(self.cargo(), 0, "dropped");
        let Some((pile @ InteractTarget::Pile(_), _)) = self.target() else { panic!("the dropped pile is the target: {:?}", self.target()) };
        self.use_target(pile);
        self.empty_bundle();
        true
    }

    /// Puts up a building of `kind` at `position`, its materials put in the
    /// tree first and carried from there one item at a time.
    fn build(&mut self, kind: BuildingKindIx, position: FxVec2) -> BuildingIx {
        self.step(&[command(CommandKind::PlaceBlueprint { kind, position })]);
        let b = self.s().events.iter().find_map(|e| match *e {
            Event::BlueprintPlaced { building } => Some(building),
            _ => None,
        });
        let b = b.expect("placed");
        while !self.s().buildings[b as usize].built() {
            let k = &self.s().building_kinds[kind as usize];
            let needs = self.s().buildings[b as usize].needs(k);
            for (item, units) in needs {
                let depot = TREE[item as usize];
                self.step(&[command(CommandKind::AddStock { depot, item, units })]);
                let dock = self.s().depots[depot as usize].position;
                self.walk_to(dock);
                self.use_target(InteractTarget::Depot(depot));
                self.walk_to_building(b);
                self.use_target(InteractTarget::Site(b));
                self.empty_bundle();
            }
            self.walk_to_building(b);
            self.use_target(InteractTarget::Site(b));
        }
        b
    }

    /// A scenario point (meters from the clearing's center) in world tiles.
    fn at(&self, x: i32, z: i32) -> FxVec2 {
        self.sim.world().terrain().center + FxVec2::new(Fx::from_int(x), Fx::from_int(z))
    }
}

/// Sixteen cycles over a growing town: each reassigns all four sprites
/// (wood, stone or idle, in turn), possesses one building with interact,
/// Bursting when ready, carries and drops what an output holds, and every
/// fourth puts up a building. Nothing strands a sprite, every invariant
/// holds on every tick, and at the end every sprite with a job and a free
/// building of its kind is at work.
#[test]
fn possess_leave_reassign_and_build_cycles_strand_no_sprite() {
    let mut soak = Soak::new();
    // Where buildings go, in turn: around the clearing's open sides.
    let spots = [(5, 0, STONE_GATHERER), (4, 4, WOOD_GATHERER), (4, -4, STONE_GATHERER), (-4, -4, WOOD_GATHERER)];
    let drops = [(3, -2), (-2, -3), (1, -4), (-3, -1)];
    let (mut bursts, mut carried, mut built) = (0, 0, 0);
    for cycle in 0..16usize {
        let kinds = [SpriteJob::Staff(WOOD_GATHERER), SpriteJob::Staff(STONE_GATHERER), SpriteJob::Idle];
        let jobs: Vec<Command> = SPRITES.iter().enumerate().map(|(i, &agent)| command(CommandKind::SetSpriteJob { agent, job: kinds[(cycle + i) % 3] })).collect();
        soak.step(&jobs);
        soak.steps(2 * HZ);
        if cycle % 4 == 0 && built < spots.len() {
            let (x, z, kind) = spots[built];
            let at = soak.at(x, z);
            soak.build(kind, at);
            built += 1;
        }
        let buildings = soak.s().buildings.len() as BuildingIx;
        let b = (cycle as BuildingIx * 3) % buildings;
        bursts += usize::from(soak.possess(b, (cycle % 4 + 2) as u64));
        let spots: Vec<FxVec2> = (0..drops.len()).map(|i| drops[(cycle + i) % drops.len()]).map(|(x, z)| soak.at(x, z)).collect();
        carried += usize::from(soak.carry_and_drop(b, &spots));
        soak.steps((cycle % 3) as u64 * HZ);
    }
    assert_eq!(built, spots.len());
    assert!(bursts >= 4, "{bursts} Bursts");
    assert!(carried >= 8, "{carried} loads carried and dropped");
    // Settled: every sprite with a job works, as each kind has two
    // buildings and at most two sprites want it.
    soak.until(30 * HZ, "everyone at work", |s| {
        SPRITES.iter().all(|&a| {
            let sprite = s.agents[a as usize].sprite().unwrap();
            sprite.job == SpriteJob::Idle || s.buildings.iter().any(|b| b.slot == Slot::Occupied(super::Occupant::Sprite(a)))
        })
    });
    let s = soak.s();
    assert_eq!(s.piles.as_ref().map_or(0, |p| p.piles.len()), 0, "every drop was picked back up");
    assert!(soak.longest <= RETRY_TICKS + 1);
}

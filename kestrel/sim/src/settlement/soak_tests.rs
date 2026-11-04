//! The stability soak (A8, handoff "Tests"): a town played for minutes in
//! cycles of reassigning every sprite, possessing a building (Burst when
//! ready) and leaving it, carrying an output's units, dropping them and
//! picking them back up, and putting up new buildings. Every tick checks
//! every invariant (`validate`, conservation and pile reservations among
//! them), and that no sprite waits longer than a retry while a building of
//! its job's kind has a free slot, nor stays dead past its return.
//!
//! The second soak plays the map's waves (M5.6) while doing all that and
//! defending: stocking, manning, possessing and firing a ballista,
//! throwing fireballs, and putting broken buildings back up. Both play on
//! the map: the town on its hill in the 256 m mine (M6.3).

use content::scenario::Scenario;
use content::TICKS_PER_SECOND;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::haulers::RETRY_TICKS;
use super::{AgentIx, BuildingIx, BuildingKindIx, DepotIx, Event, InteractTarget, Interaction, ItemIx, Seat, Settlement, Slot, SpriteJob, Step};
use crate::{Command, CommandKind, PlayerId, Simulation};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
/// The sprites, by agent index: after the elder, in scenario order.
const SPRITES: [AgentIx; 4] = [1, 2, 3, 4];
const WOOD: ItemIx = 0;
const STONE: ItemIx = 1;
const TREE: [DepotIx; 2] = [0, 1];
const WOOD_GATHERER: BuildingKindIx = 0;
const BALLISTA: BuildingKindIx = 1;
/// The build soak's test-only kind (`tests::with_stone_gatherer`): the
/// scene has no building that makes stone (M6.4), but the soak reassigns
/// sprites across two gathering kinds.
const STONE_GATHERER: BuildingKindIx = 2;

/// The scene on its map: the hill in the middle of its mine (M6.3).
fn sacred_tree() -> Simulation {
    super::hill_tests::sacred_tree_map(Scenario::parse(SACRED_TREE).unwrap())
}

/// The map with no waves, and a test-only Stone Gatherer to build.
fn calm_sacred_tree() -> Simulation {
    let mut scenario = super::tests::with_stone_gatherer(SACRED_TREE);
    scenario.waves.clear();
    super::hill_tests::sacred_tree_map(scenario)
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
    fn new(sim: Simulation) -> Self {
        Soak { sim, waiting: [0; 4], longest: 0 }
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
            // A dead sprite is back on its return tick (B6).
            let tick = self.sim.world().tick();
            assert!(s.agents[a as usize].body.alive() || tick <= sprite.respawn_at + 1, "{} still dead {} ticks past its return", s.agents[a as usize].id, tick - sprite.respawn_at);
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
        let b = b.unwrap_or_else(|| panic!("placed at {position:?}: {:?}", self.s().events));
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
/// building of its kind is at work. No waves come: the east one's column
/// would walk through the spots it builds on.
#[test]
fn possess_leave_reassign_and_build_cycles_strand_no_sprite() {
    let mut soak = Soak::new(calm_sacred_tree());
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

/// What the wave soak saw, from the events.
#[derive(Debug, Default)]
struct Seen {
    sprite_shots: u32,
    elder_shots: u32,
    fireballs: u32,
    fire_kills: u32,
    kills: u32,
    breaches: u32,
    broken: u32,
    sprite_deaths: u32,
    sprite_returns: u32,
    elder_deaths: u32,
    elder_digs: u32,
}

impl Soak {
    /// A scenario point in meters, as the supply writes them.
    fn point(&self, x: f64, z: f64) -> FxVec2 {
        let f = |v: f64| Fx::from_bits((v * 4294967296.0).round() as i64);
        self.sim.world().terrain().center + FxVec2::new(f(x), f(z))
    }

    /// Steps with `turn` and adds up what happened.
    fn watch(&mut self, turn: &[Command], seen: &mut Seen) {
        let alive = self.s().agents[ELDER as usize].body.alive();
        let sprites: Vec<bool> = SPRITES.iter().map(|&a| self.s().agents[a as usize].body.alive()).collect();
        let seat = self.s().seat(ELDER as usize).and_then(|seat| seat.building());
        let digging = self.s().agents[ELDER as usize].avatar().is_some_and(|a| a.dig.is_some());
        self.step(turn);
        let s = self.s();
        for e in &s.events {
            match *e {
                Event::BuildingShot { building } if Some(building) == seat => seen.elder_shots += 1,
                Event::BuildingShot { .. } => seen.sprite_shots += 1,
                Event::FireballThrown { .. } => seen.fireballs += 1,
                Event::Hit { killed: true, impact, .. } => {
                    seen.kills += 1;
                    seen.fire_kills += u32::from(impact == super::combat::Impact::Fire);
                }
                Event::Breach { .. } => seen.breaches += 1,
                Event::BuildingBroken { .. } => seen.broken += 1,
                Event::SpriteReturned { .. } => seen.sprite_returns += 1,
                _ => {}
            }
        }
        seen.elder_deaths += u32::from(alive && !s.agents[ELDER as usize].body.alive());
        seen.elder_digs += u32::from(!digging && s.agents[ELDER as usize].avatar().is_some_and(|a| a.dig.is_some()));
        seen.sprite_deaths += SPRITES.iter().zip(sprites).filter(|&(&a, was)| was && !s.agents[a as usize].body.alive()).count() as u32;
    }

    fn wait(&mut self, ticks: u64, seen: &mut Seen) {
        for _ in 0..ticks {
            self.watch(&[], seen);
        }
    }

    /// Walks the elder to `target`, as far as it gets in 20 s: a dead elder
    /// waits to be back at home and sets off again. Ghouls may be in the
    /// way, so it may not get there.
    fn go(&mut self, target: FxVec2, seen: &mut Seen) {
        let start = self.sim.world().tick();
        let order = command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } });
        self.watch(std::slice::from_ref(&order), seen);
        while self.sim.world().tick() < start + 20 * HZ {
            let a = &self.s().agents[ELDER as usize];
            if !a.body.alive() {
                self.wait(1, seen);
                if self.s().agents[ELDER as usize].body.alive() {
                    self.watch(std::slice::from_ref(&order), seen);
                }
                continue;
            }
            if a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop) {
                return;
            }
            self.wait(1, seen);
        }
    }

    /// Interacts with `target` if it is the elder's target now, and waits
    /// (at most 10 s) for its units to land. Whether it was.
    fn try_use(&mut self, target: InteractTarget, seen: &mut Seen) -> bool {
        if self.target().map(|(t, _)| t) != Some(target) {
            return false;
        }
        self.watch(&[interact(target)], seen);
        for _ in 0..10 * HZ {
            let s = self.s();
            if s.agents[ELDER as usize].carry.job.is_none() && s.transfers.is_empty() {
                break;
            }
            self.watch(&[], seen);
        }
        true
    }

    /// Puts whatever the bundle holds back in the tree, as far as it can.
    fn unload(&mut self, seen: &mut Seen) {
        for (item, depot) in [(WOOD, TREE[0]), (STONE, TREE[1])] {
            if self.s().agents[ELDER as usize].carry.inventory.count(item) > 0 {
                let dock = self.s().depots[depot as usize].position;
                self.go(dock, seen);
                self.try_use(InteractTarget::Depot(depot), seen);
            }
        }
    }

    /// Where the elder stands to use building `b`'s output or site.
    fn front(&self, b: BuildingIx) -> FxVec2 {
        let building = &self.s().buildings[b as usize];
        building.output_position + (building.output_position - building.position).normalize_or_zero() * Fx::from_ratio(2, 5)
    }

    /// Fetches a bundle of `item` from the tree (topping its pile up first,
    /// as a stocked town would be) and hands it to building `b`'s site or
    /// store; what's left goes back.
    fn bring(&mut self, b: BuildingIx, item: ItemIx, site: bool, seen: &mut Seen) {
        let depot = TREE[item as usize];
        self.watch(&[command(CommandKind::AddStock { depot, item, units: 5 })], seen);
        let dock = self.s().depots[depot as usize].position;
        self.go(dock, seen);
        self.try_use(InteractTarget::Depot(depot), seen);
        self.go(self.front(b), seen);
        self.try_use(if site { InteractTarget::Site(b) } else { InteractTarget::Output(b) }, seen);
        self.unload(seen);
    }

    /// Puts site `b` back up: its parts' materials brought from the tree
    /// and installed, a few tries at most (a ghoul may be on the way).
    fn rebuild(&mut self, b: BuildingIx, seen: &mut Seen) {
        for _ in 0..6 {
            if self.s().buildings[b as usize].built() {
                return;
            }
            let k = &self.s().building_kinds[self.s().buildings[b as usize].kind as usize];
            for (item, _) in self.s().buildings[b as usize].needs(k) {
                self.bring(b, item, true, seen);
            }
            self.go(self.front(b), seen);
            self.try_use(InteractTarget::Site(b), seen);
        }
    }

    /// Possesses gatherer `b` a couple of seconds, Bursting if it may, and
    /// leaves, as `possess` does, but a refused boarding is no failure.
    fn work(&mut self, b: BuildingIx, seen: &mut Seen) {
        let Some(to) = self.s().leave_spot(b, self.s().agents[ELDER as usize].radius(), self.sim.world().terrain()) else { return };
        self.go(to, seen);
        if !self.try_use(InteractTarget::Slot(b), seen) || self.s().seat(ELDER as usize) != Some(Seat::Building(b)) {
            return;
        }
        if self.s().burst_check(ELDER as usize, self.sim.world().tick()).is_ok() {
            self.watch(&[command(CommandKind::Burst { agent: ELDER })], seen);
        }
        self.wait(2 * HZ, seen);
        if self.s().seat(ELDER as usize).is_some() {
            self.watch(&[interact(InteractTarget::Slot(b))], seen);
        }
    }

    /// Walks the elder down to the `n`th stone tile a miner's search finds
    /// from the tree's stone pile (`miners::search`), from the tile behind
    /// the one it digs from, so it faces it, and digs a layer in front
    /// (M6.2). A ghoul or a miner may be in the way, or the tile dug out
    /// already, so the dig may be refused.
    fn dig_stone(&mut self, n: usize, seen: &mut Seen) {
        let (dock, radius) = (self.s().depots[TREE[1] as usize].position, self.s().agents[ELDER as usize].radius());
        let targets = super::miners::search(self.sim.world().terrain(), crate::terrain::Terrain::pit_tile(dock), radius);
        let Some(t) = targets.get(n % targets.len().max(1)).copied() else { return };
        let behind = (2 * t.stand.0 - t.tile.0, 2 * t.stand.1 - t.tile.1);
        self.go(crate::terrain::Terrain::pit_center(behind), seen);
        self.go(crate::terrain::Terrain::pit_center(t.stand), seen);
        self.watch(&[command(CommandKind::DigDown { agent: ELDER })], seen);
        self.wait(5 * HZ / 2, seen);
    }

    /// Possesses ballista `b` for `seconds`, shooting at the nearest ghoul
    /// in range whenever it may and throwing the fireball at it when ready,
    /// then leaves. Whether it got in.
    fn defend(&mut self, b: BuildingIx, seconds: u64, seen: &mut Seen) -> bool {
        let Some(to) = self.s().leave_spot(b, self.s().agents[ELDER as usize].radius(), self.sim.world().terrain()) else { return false };
        self.go(to, seen);
        if !self.try_use(InteractTarget::Slot(b), seen) || self.s().seat(ELDER as usize) != Some(Seat::Building(b)) {
            return false;
        }
        for _ in 0..seconds * HZ {
            if self.s().seat(ELDER as usize).is_none() {
                return true;
            }
            let (center, range) = {
                let s = self.s();
                (s.buildings[b as usize].position, s.building_kind(b).defence.as_ref().map_or(Fx::ZERO, |d| d.range))
            };
            let nearest = self.s().threats.iter().map(|t| t.position - center).filter(|o| o.length() <= range).min_by_key(|o| o.length());
            let tick = self.sim.world().tick();
            let mut turn = Vec::new();
            if let Some(aim) = nearest {
                turn.push(command(CommandKind::AimAvatar { agent: ELDER, aim }));
                if self.s().burst_check(ELDER as usize, tick).is_ok() {
                    turn.push(command(CommandKind::Burst { agent: ELDER }));
                } else if self.s().fire_check(ELDER as usize, tick).is_ok() {
                    turn.push(command(CommandKind::Attack { agent: ELDER, weapon: content::scenario::Weapon::Sword, aim, muzzle: [Fx::ZERO; 3] }));
                }
            }
            self.watch(&turn, seen);
        }
        if self.s().seat(ELDER as usize).is_some() {
            self.watch(&[interact(InteractTarget::Slot(b))], seen);
        }
        true
    }
}

/// The map played through all its waves (13 minutes, the last ghouls
/// walking up from the ring) by a town that defends: before the first wave
/// it puts up a ballista beside the tree and a Wood Gatherer on each lane's
/// way, the west's and near the east column's; then it cycles
/// reassigning the sprites (Ash always on the ballista, the others walking
/// to gatherers across the waves' ways or mining down the ramps, M6.5, left
/// mid-errand as it may be, fleeing ghouls off the hill, M6.7), stocking
/// the ballista, possessing it to shoot and throw fireballs while ghouls
/// are about, digging stone off the hill or possessing a gatherer to Burst
/// while none are, and putting broken buildings back up. Digs replan the
/// ghouls' routes that pass near them. Every tick checks every
/// invariant and the balances, that no sprite waits past a retry with a
/// slot free or stays dead past its return; the tree stands.
#[test]
fn defending_against_the_waves_strands_no_sprite() {
    let mut soak = Soak::new(sacred_tree());
    let mut seen = Seen::default();
    // A ballista by each ramp's end (M6.7: one alone leaves the west
    // lanes to breach).
    let ballistas = [soak.build(BALLISTA, soak.point(3.0, -1.4)), soak.build(BALLISTA, soak.point(-3.0, -1.4))];
    let west = soak.build(WOOD_GATHERER, soak.point(-6.2, -3.4));
    let east = soak.build(WOOD_GATHERER, soak.point(6.0, -1.6));
    soak.empty_bundle();
    let gatherers = [0, west, east];
    eprintln!("wave soak: built at {} s", soak.sim.world().tick() / HZ);
    assert!(soak.sim.world().tick() < 165 * HZ, "built in the calm start");
    let mut cycle = 0usize;
    while soak.sim.world().tick() < 730 * HZ {
        // Ash and Fern man the ballistas; the others rotate over the
        // gatherers and mining every fourth cycle, so they walk across the
        // clearing, stay a while in buildings on the waves' ways, and dig
        // off the hill.
        let rota = [SpriteJob::Staff(WOOD_GATHERER), SpriteJob::Mine, SpriteJob::Mine];
        let mut jobs: Vec<Command> = SPRITES[..2].iter().map(|&agent| command(CommandKind::SetSpriteJob { agent, job: SpriteJob::Staff(BALLISTA) })).collect();
        jobs.extend(SPRITES[2..].iter().enumerate().map(|(i, &agent)| command(CommandKind::SetSpriteJob { agent, job: rota[(cycle / 4 + i) % 3] })));
        soak.watch(&jobs, &mut seen);
        for b in ballistas.into_iter().chain([west, east]) {
            if !soak.s().buildings[b as usize].built() {
                soak.rebuild(b, &mut seen);
            }
        }
        for b in ballistas {
            if soak.s().buildings[b as usize].built() && soak.s().buildings[b as usize].output.free() >= 5 {
                soak.bring(b, STONE, false, &mut seen);
            }
        }
        // Ghouls on the hill: the elder defends from the ballista nearer
        // the nearest. Those off it, after a miner below, are the miner's
        // lookout.
        let tree = soak.sim.world().terrain().center;
        let nearest = soak.s().threats.iter().map(|t| t.position).filter(|p| (*p - tree).length() < Fx::from_int(15)).min_by_key(|p| (*p - tree).length());
        let ballista = nearest.map(|p| if p.x < tree.x { ballistas[1] } else { ballistas[0] });
        if let Some(ballista) = ballista.filter(|&b| soak.s().buildings[b as usize].built()) {
            soak.defend(ballista, 8 + cycle as u64 % 5, &mut seen);
        } else if cycle % 3 == 0 && soak.s().agents[ELDER as usize].carry.inventory.total() == 0 {
            // The elder digs stone for the miners to carry up.
            soak.dig_stone(cycle / 3, &mut seen);
        } else {
            let b = gatherers[cycle % gatherers.len()];
            if soak.s().buildings[b as usize].built() && soak.s().agents[ELDER as usize].carry.inventory.total() == 0 {
                soak.work(b, &mut seen);
            }
        }
        soak.wait((cycle % 3) as u64 * HZ, &mut seen);
        cycle += 1;
    }
    let s = soak.s();
    let mined = s.accounts[STONE as usize].generated;
    eprintln!("wave soak: {cycle} cycles, {seen:?}, tree {}, {mined} stone mined", s.health);
    assert!(!s.tree_fallen(), "the tree stands: {seen:?}");
    assert!(s.waves.iter().all(|w| w.spawned == w.count), "every wave came");
    assert!(seen.sprite_shots > 0 && seen.elder_shots > 0, "{seen:?}");
    assert!(seen.fireballs >= 3, "{seen:?}");
    assert!(seen.kills >= 20 && seen.fire_kills > 0, "{seen:?}");
    assert!(seen.broken >= 2 && seen.sprite_deaths >= 1 && seen.sprite_returns == seen.sprite_deaths, "buildings broke, sprites died and came back: {seen:?}");
    assert!(soak.longest <= RETRY_TICKS + 1);
    assert!(mined >= 6, "the miners and the elder dug stone: {mined}");
    assert!(seen.elder_digs >= 3, "the elder dug: {seen:?}");
}

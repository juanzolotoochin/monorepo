//! The stability soak (A8, handoff "Tests"): a town played for minutes in
//! cycles of reassigning every sprite, possessing a building (Burst when
//! ready) and leaving it, carrying an output's units, dropping them and
//! picking them back up, and putting up new buildings. Every tick checks
//! every invariant (`validate`, conservation and pile reservations among
//! them), and that no sprite waits longer than a retry while a building of
//! its job's kind has a free slot, nor stays dead past its return.
//!
//! The second soak plays the map's nights (M5.6, M7.2: five days and
//! nights, a wave each night and the sunrise burning the stragglers away)
//! while doing all that and defending: stocking, manning, possessing and firing a ballista,
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
const STONE_GATHERER: BuildingKindIx = 3;

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
    dispelled: u32,
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
                Event::Dispelled { .. } => seen.dispelled += 1,
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
        let amber = self.s().amber_depot();
        for (item, depot) in [(WOOD, TREE[0]), (STONE, TREE[1])].into_iter().chain(amber) {
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
    /// (M6.2). In the town's volume the search finds walls (M8.4): the
    /// elder digs level into one as a miner does, from a stand whose tile
    /// behind is on its floor (so it walks up facing the wall). A ghoul or a
    /// miner may be in the way, or the tile dug out already, so the dig may
    /// be refused.
    fn dig_stone(&mut self, n: usize, seen: &mut Seen) {
        let (dock, radius) = (self.s().depots[TREE[1] as usize].position, self.s().agents[ELDER as usize].radius());
        let terrain = self.sim.world().terrain();
        let behind = |t: &super::miners::Target| (2 * t.stand.0 - t.tile.0, 2 * t.stand.1 - t.tile.1);
        let targets: Vec<_> = super::miners::search(terrain, &self.s().light, crate::terrain::Terrain::pit_tile(dock), radius)
            .into_iter()
            .filter(|t| match (&terrain.volume, t.wall) {
                (Some(volume), Some(floor)) => volume.floor_near(behind(t), floor).is_some(),
                _ => true,
            })
            .collect();
        let Some(t) = targets.get(n % targets.len().max(1)).copied() else { return };
        self.go(crate::terrain::Terrain::pit_center(behind(&t)), seen);
        self.go(crate::terrain::Terrain::pit_center(t.stand), seen);
        let direction = if t.wall.is_some() { crate::volume::DigDirection::Level } else { crate::volume::DigDirection::Down };
        self.watch(&[command(CommandKind::DigFront { agent: ELDER, direction })], seen);
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

/// The map played through five days and nights (nearly 19 minutes, the
/// last night repeating the fourth) by a town that defends: before the
/// first dusk
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
    assert!(soak.sim.world().tick() < 150 * HZ, "built on the first day");
    let clock = soak.s().clock.expect("days and nights");
    let nights = 5;
    let mut cycle = 0usize;
    while soak.sim.world().tick() < nights * clock.cycle() + 5 * HZ {
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
    let due: u32 = s.waves.iter().map(|w| (1..=nights).map(|n| w.on_night(n)).sum::<u32>()).sum();
    assert_eq!(s.waves.iter().map(|w| w.spawned).sum::<u32>(), due, "every night came");
    assert_eq!(s.nights_survived(soak.sim.world().tick()), Some(nights));
    assert_eq!(u64::from(seen.dispelled), s.dispelled);
    assert!(seen.sprite_shots > 0 && seen.elder_shots > 0, "{seen:?}");
    assert!(seen.fireballs >= 3, "{seen:?}");
    assert!(seen.kills >= 20 && seen.fire_kills > 0, "{seen:?}");
    assert!(seen.broken >= 2 && seen.sprite_deaths >= 1 && seen.sprite_returns == seen.sprite_deaths, "buildings broke, sprites died and came back: {seen:?}");
    assert!(soak.longest <= RETRY_TICKS + 1);
    assert!(mined >= 6, "the miners and the elder dug stone: {mined}");
    assert!(seen.elder_digs >= 3, "the elder dug: {seen:?}");
}

// The depths soak (M8.8 of the depths design, `kestrel/SACRED_TREE_DEPTHS.md`
// "Tests"): the town through three days and nights with the elder digging
// a stair and a tunnel under it and putting up lanterns, the miners
// mining, and the waves coming.

/// What the depths soak saw of the elder's tunnel.
#[derive(Debug, Default)]
struct Depths {
    /// Digs sent, and those that went through.
    sent: u32,
    dug: u32,
    refused: std::collections::BTreeMap<String, u32>,
    /// Times the elder didn't get to its stand (a ghoul, a miner in the way).
    missed: u32,
    /// Its lowest feet, under the surface where it dug.
    deepest: Fx,
    /// Amber it picked up in the tunnel, in units.
    amber_picked: u32,
    /// Lantern blueprints put down, and refused.
    lanterns: u32,
    lantern_refused: u32,
    /// The most ghouls at once more than a meter under the top floor.
    ghouls_under: u32,
    /// The most floors in lantern light.
    lit: u32,
    /// Units of piles in its way it carried out.
    carried: u32,
    /// Amber cells among those its digs cleared.
    amber_cells: u32,
    /// Where the tunnel turned (from the tree's tile).
    turns: Vec<(i32, i32)>,
}

/// The elder's tunnel: where it stands to dig next, which way, the stair's
/// steps still to dig down, and tiles dug level since the last lantern.
struct Tunnel {
    stand: (i32, i32),
    way: (i32, i32),
    stair: u32,
    since_lantern: u32,
    lantern_kind: BuildingKindIx,
    /// The feet's height at the stand, and the tiles dug, in order.
    feet: Fx,
    dug: Vec<(i32, i32)>,
}

/// A lantern every this many tiles of level tunnel.
const LANTERN_EVERY: u32 = 8;

impl Soak {
    /// The elder digs its tunnel on a tile: carries what it holds up to the
    /// tree first, walks to its stand and turns the way, digs (a step down on the stair, then the plain dig, F: level),
    /// steps in, picks up any amber the dig dropped and, past every
    /// `LANTERN_EVERY` tiles, puts a lantern's blueprint down behind it if
    /// the tree has a stone and an amber for it that no other site waits
    /// for. A pile in the way it carries out; another refused tile turns
    /// the tunnel left.
    fn dig_tunnel(&mut self, t: &mut Tunnel, seen: &mut Seen, d: &mut Depths) {
        use crate::terrain::Terrain;
        if self.cargo() > 0 {
            self.unload(seen);
        }
        // Back from the surface (the tree, a fight), in by the stair: a
        // walk to the stand alone may end on the ground over it.
        let feet = self.s().agents[ELDER as usize].avatar().unwrap().height;
        if (feet - t.feet).abs() > Fx::ONE {
            for &w in [3, 11].iter().filter_map(|&i| t.dug.get(i)) {
                self.go(Terrain::pit_center(w), seen);
            }
        }
        self.go(Terrain::pit_center(t.stand), seen);
        // Face the way: two ticks' walk, then stand (`tunnel_soak`'s turn).
        let way = FxVec2::new(Fx::from_int(t.way.0), Fx::from_int(t.way.1));
        self.watch(&[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::Walk { direction: way, run: false } })], seen);
        self.wait(TURN - 1, seen);
        self.watch(&[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::Stop })], seen);
        self.wait(12, seen);
        let a = &self.s().agents[ELDER as usize];
        if !a.body.alive() || Terrain::pit_tile(a.body.position) != t.stand || (a.avatar().unwrap().height - t.feet).abs() > Fx::from_ratio(3, 4) {
            d.missed += 1;
            return;
        }
        let front = (t.stand.0 + t.way.0, t.stand.1 + t.way.1);
        let (world, tick) = (self.sim.world(), self.sim.world().tick());
        let tree = Terrain::pit_tile(world.terrain().center);
        let would = if t.stair > 0 {
            self.s().dig_volume_of(ELDER, tick, world.terrain(), crate::volume::DigDirection::Down)
        } else {
            self.s().dig_plain_of(ELDER, tick, world.terrain())
        };
        match would.unwrap() {
            Ok((tile, bottom)) if tile == front => {
                let volume = self.sim.world().terrain().volume.as_ref().unwrap();
                d.amber_cells += (bottom..bottom + crate::volume::HEADROOM).filter(|&l| volume.ore(tile, l) == Some(crate::mine::AMBER)).count() as u32;
            }
            Ok(_) => {
                *d.refused.entry("Heading".into()).or_default() += 1;
                return;
            }
            Err(why) => {
                *d.refused.entry(format!("{why:?}")).or_default() += 1;
                if matches!(why, crate::terrain::DigRefusal::Clear) {
                    // Dug already (a miner): walk on.
                    t.stand = front;
                } else if matches!(why, crate::terrain::DigRefusal::Pile) {
                    // A pile of earlier digs on the way: carry it out (the
                    // next call takes it up to the tree).
                    if let Some((target @ InteractTarget::Pile(_), _)) = self.target() {
                        if self.try_use(target, seen) {
                            d.carried += self.cargo();
                        }
                    }
                } else {
                    t.way = (t.way.1, -t.way.0);
                    d.turns.push((t.stand.0 - tree.0, t.stand.1 - tree.1));
                }
                return;
            }
        }
        let kind = if t.stair > 0 {
            CommandKind::DigFront { agent: ELDER, direction: crate::volume::DigDirection::Down }
        } else {
            CommandKind::DigDown { agent: ELDER }
        };
        let digs = seen.elder_digs;
        d.sent += 1;
        self.watch(&[command(kind)], seen);
        self.wait(5 * HZ / 2, seen);
        if seen.elder_digs == digs {
            return;
        }
        d.dug += 1;
        let surface = self.sim.world().terrain().generated_top(front).map_or(Fx::ZERO, crate::volume::height);
        if t.stair > 0 {
            t.stair -= 1;
        } else {
            t.since_lantern += 1;
        }
        let was = t.stand;
        t.stand = front;
        t.dug.push(front);
        self.go(Terrain::pit_center(front), seen);
        let feet = self.s().agents[ELDER as usize].avatar().unwrap().height;
        t.feet = feet;
        d.deepest = d.deepest.min(feet - surface);
        // The dig's amber, if it dropped any.
        let amber = self.s().amber_depot().map(|(item, _)| item);
        for _ in 0..3 {
            let Some((target @ InteractTarget::Pile(id), _)) = self.target() else { break };
            let pile = self.s().piles.as_ref().and_then(|p| p.piles.get(&id));
            if pile.is_none_or(|p| p.item() != amber) || self.cargo() > 0 {
                break;
            }
            if self.try_use(target, seen) {
                d.amber_picked += self.cargo();
            }
        }
        // A lantern when the tree can pay for it.
        if t.stair == 0 && (t.since_lantern >= LANTERN_EVERY || d.lanterns == 0) && self.affords(t.lantern_kind) {
            self.watch(&[command(CommandKind::PlaceBlueprint { kind: t.lantern_kind, position: Terrain::pit_center(was) })], seen);
            if self.s().events.iter().any(|e| matches!(e, Event::BlueprintPlaced { .. })) {
                d.lanterns += 1;
                t.since_lantern = 0;
            } else {
                d.lantern_refused += 1;
            }
        }
    }

    /// Whether the tree's piles hold the materials for a building of
    /// `kind` that no site of that kind already waits for.
    fn affords(&self, kind: BuildingKindIx) -> bool {
        let s = self.s();
        let k = &s.building_kinds[kind as usize];
        let have = |item: ItemIx| s.depots.iter().map(|dep| dep.inventory.count(item)).sum::<u32>();
        let owed = |item: ItemIx| -> u32 {
            s.buildings.iter().filter(|b| b.kind == kind && !b.built()).flat_map(|b| b.needs(k)).filter(|w| w.0 == item).map(|w| w.1).sum()
        };
        k.parts.iter().flatten().all(|&(item, units)| have(item) >= units + owed(item))
    }
}

/// The town through three days and nights (11 min 15 s) with its depths:
/// two ballistas by the ramps as in the wave soak, Ash and Fern on them,
/// Moss and Reed mining; the elder defends while ghouls are near the tree
/// and otherwise digs a stair down 6 m east from the hill's north foot and
/// a tunnel on north from its bottom, picking up the amber it digs and
/// carrying it up, and puts up a lantern every 8 m of tunnel the tree can
/// pay for (the miners bring its stone and amber down and build it).
/// Every tick validates the settlement and the balances; every 3D plan the
/// town's bodies make is counted (`nav::tally`) and none expands more than
/// `nav::PATH_BUDGET` nodes. The tree stands every night.
#[test]
fn the_town_digs_and_lights_its_depths_through_three_nights() {
    let mut soak = Soak::new(sacred_tree());
    let mut seen = Seen::default();
    let mut d = Depths::default();
    let ballistas = [soak.build(BALLISTA, soak.point(3.0, -1.4)), soak.build(BALLISTA, soak.point(-3.0, -1.4))];
    soak.empty_bundle();
    let lantern_kind = soak.s().building_kinds.iter().position(|k| k.is_lantern()).unwrap() as BuildingKindIx;
    let mut jobs: Vec<Command> = SPRITES[..2].iter().map(|&agent| command(CommandKind::SetSpriteJob { agent, job: SpriteJob::Staff(BALLISTA) })).collect();
    jobs.extend(SPRITES[2..].iter().map(|&agent| command(CommandKind::SetSpriteJob { agent, job: SpriteJob::Mine })));
    soak.watch(&jobs, &mut seen);
    let tree_tile = crate::terrain::Terrain::pit_tile(soak.sim.world().terrain().center);
    let start = (tree_tile.0 - 8, tree_tile.1 - 22);
    let feet = soak.sim.world().terrain().tile_height(start).unwrap();
    let mut tunnel = Tunnel { stand: start, way: (1, 0), stair: 12, since_lantern: 0, lantern_kind, feet, dug: Vec::new() };
    let (amber, stone) = (soak.s().amber_depot().unwrap().0, soak.s().stone_depot().unwrap().0);
    soak.sim.on_pool(crate::nav::tally::take);
    let clock = soak.s().clock.expect("days and nights");
    let nights = 3;
    let mut cycle = 0usize;
    let mut turned = false;
    while soak.sim.world().tick() < nights * clock.cycle() + 5 * HZ {
        for b in ballistas {
            if !soak.s().buildings[b as usize].built() {
                soak.rebuild(b, &mut seen);
            }
            if soak.s().buildings[b as usize].built() && soak.s().buildings[b as usize].output.free() >= 5 {
                soak.bring(b, STONE, false, &mut seen);
            }
        }
        let tree = soak.sim.world().terrain().center;
        let nearest = soak.s().threats.iter().map(|t| t.position).filter(|p| (*p - tree).length() < Fx::from_int(15)).min_by_key(|p| (*p - tree).length());
        let ballista = nearest.map(|p| if p.x < tree.x { ballistas[1] } else { ballistas[0] });
        if let Some(ballista) = ballista.filter(|&b| soak.s().buildings[b as usize].built()) {
            soak.defend(ballista, 8 + cycle as u64 % 5, &mut seen);
        } else {
            // At the stair's bottom the tunnel turns north.
            if tunnel.stair == 0 && !turned {
                (tunnel.way, turned) = ((0, -1), true);
            }
            soak.dig_tunnel(&mut tunnel, &mut seen, &mut d);
        }
        let s = soak.s();
        let terrain = soak.sim.world().terrain();
        let under = s.threats.iter().filter(|t| t.height < terrain.height(t.position) - Fx::ONE).count() as u32;
        d.ghouls_under = d.ghouls_under.max(under);
        d.lit = d.lit.max(s.light.lantern_floors() as u32);
        cycle += 1;
    }
    let plans = soak.sim.on_pool(crate::nav::tally::take);
    let s = soak.s();
    let lanterns_built = s.buildings.iter().filter(|b| b.kind == lantern_kind && b.built()).count();
    let (mined_stone, mined_amber) = (s.accounts[stone as usize].generated, s.accounts[amber as usize].generated);
    eprintln!("depths soak: {cycle} cycles, {seen:?}");
    eprintln!("depths soak: tunnel {d:?}, {lanterns_built} lanterns built, ends at {:?}", (tunnel.stand.0 - tree_tile.0, tunnel.stand.1 - tree_tile.1));
    eprintln!("depths soak: plans {plans:?}");
    eprintln!(
        "depths soak: tree {} of {}, {} nights, {mined_stone} stone and {mined_amber} amber dug, {} stone and {} amber at the tree",
        s.health,
        s.base_health,
        s.nights_survived(soak.sim.world().tick()).unwrap(),
        s.stock("tree_stone", "stone"),
        s.stock("tree_amber", "amber")
    );
    assert!(!s.tree_fallen(), "the tree stands: {seen:?}");
    assert_eq!(s.nights_survived(soak.sim.world().tick()), Some(nights));
    assert!(plans.plans > 200 && plans.most_expanded <= crate::nav::PATH_BUDGET, "{plans:?}");
    assert!(d.dug >= 40 && d.deepest <= Fx::from_int(-4), "the elder dug a stair and a tunnel under 4 m: {d:?}");
    // M8.8's tuning: a lantern or so a day from the tunnel's own amber.
    assert!(mined_amber >= 3 && lanterns_built >= 2, "the tunnel paid for its lanterns: {mined_amber} amber, {lanterns_built} lanterns, {d:?}");
}

// The Tunnel Lab soak (V.8 of the verticality design,
// `kestrel/VERTICALITY.md`): the avatar walks and digs at random through
// the mountain, the cave and over the bridge, the test walker following.

/// A small deterministic generator for the soak's choices (xorshift64*).
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// From `0` up to, not including, `n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A whole number of meters from `-r` to `r`.
    fn meters(&mut self, r: i32) -> Fx {
        Fx::from_int(self.below(2 * r as u64 + 1) as i32 - r)
    }
}

/// How long the soak's avatar steps to turn before a dig: two ticks, so
/// it stays in its tile's middle (its footprint in its own tile).
const TURN: u64 = 2;

/// How long the Tunnel Lab soak plays: two minutes.
const TUNNEL_SOAK_TICKS: u64 = 120 * HZ;

/// A body of `radius` at `p`, its feet at `z`, stands on a floor (none of
/// its five points over a wall or a passage without headroom), not under
/// it, with its head under any ceiling; and the cells its body fills at its
/// middle, from a layer over its feet to its head, are empty.
fn standing_free(terrain: &crate::terrain::Terrain, who: &str, p: FxVec2, z: Fx, radius: Fx, tick: u64) {
    use crate::volume::{layer_at, BODY_HEIGHT};
    let floor = terrain.footprint_at(p, z, radius).unwrap_or_else(|| panic!("{who} has no floor at tick {tick}: {:?}, feet {z:?}", p - terrain.center));
    assert!(floor <= z + Fx::from_ratio(1, 100), "{who} stands under its floor {floor:?} at tick {tick}: {:?}, feet {z:?}", p - terrain.center);
    if let Some(ceiling) = terrain.ceiling_at(p, z, radius) {
        assert!(ceiling >= z + BODY_HEIGHT - Fx::from_ratio(1, 100), "{who}'s head is over the ceiling {ceiling:?} at tick {tick}: {:?}, feet {z:?}", p - terrain.center);
    }
    let volume = terrain.volume.as_ref().unwrap();
    let tile = crate::terrain::Terrain::pit_tile(p);
    // From a layer over the feet (a ramp's sloped floor runs through the
    // cell under its tile's top) to the head.
    let (low, high) = (layer_at(z + crate::terrain::LAYER), layer_at(z + BODY_HEIGHT - Fx::from_ratio(1, 100)));
    for layer in low..=high {
        assert!(!volume.is_solid(tile, layer), "{who} is inside solid cell {layer} at tick {tick}: {:?}, feet {z:?}", p - terrain.center);
    }
}

/// What the soak saw: how many digs went through, of each direction, the
/// refusals by reason, the walks to a point, and the lowest and highest
/// the avatar's and the walker's feet went.
#[derive(Debug, Default)]
struct Dug {
    digs: [u32; 3],
    refused: std::collections::BTreeMap<String, u32>,
    walks: u32,
    lowest: Fx,
    highest: Fx,
    walker_lowest: Fx,
    walker_highest: Fx,
    /// Plans from the walker to the avatar every half second (M8.1): how
    /// many, how many didn't reach it, and the most nodes one expanded.
    plans: u32,
    unreached: u32,
    most_expanded: u32,
}

/// Where the soak's avatar is in a dig: going to its tile's middle (so the
/// footprint is in its own tile only), stepping two ticks the way it will
/// dig, or stopped and about to dig.
enum Plan {
    Free,
    Center { direction: crate::volume::DigDirection, heading: FxVec2, until: u64, tries: u32 },
    Turn { direction: crate::volume::DigDirection, stop_at: u64, dig_at: u64, tries: u32 },
}

/// The sixteen headings of the soak's turns, about 22.5° apart.
fn heading(i: u64) -> FxVec2 {
    let (x, y) = [(4, 0), (4, 2), (3, 3), (2, 4)][i as usize % 4];
    let (x, y) = match i / 4 % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    };
    FxVec2::new(Fx::from_int(x), Fx::from_int(y)).normalize_or_zero()
}

/// Plays the Tunnel Lab for [`TUNNEL_SOAK_TICKS`] from `seed`: whenever
/// the avatar is free, it goes to its tile's middle, turns a random way
/// (mostly toward a wall beside it, trying other ways while only a down
/// dig would go) and digs level, down or up, mostly one that digs
/// something; or walks to a random point (a 3D route),
/// anywhere or near one of the map's sights; or walks a random heading a
/// while (often onto what it just dug). The walker follows all along.
/// Every tick validates the settlement and checks both bodies stand free.
/// Returns every tick's hash.
fn tunnel_soak(seed: u64) -> (Vec<u64>, Dug) {
    use crate::terrain::Terrain;
    use crate::volume::DigDirection;
    let mut sim = super::path_tests::tunnel_lab_with_walker();
    let mut dice = Dice(seed);
    let mut dug = Dug::default();
    let me = sim.world().settlement().unwrap().player().unwrap();
    let directions = [DigDirection::Level, DigDirection::Down, DigDirection::Up];
    let (mut hashes, mut plan, mut next_at, mut walking) = (Vec::new(), Plan::Free, 0u64, false);
    let walk = |intent| command(CommandKind::MoveAvatar { agent: me, intent });
    while sim.world().tick() < TUNNEL_SOAK_TICKS {
        let tick = sim.world().tick();
        let s = sim.world().settlement().unwrap();
        let a = &s.agents[me as usize];
        let digging = a.avatar().unwrap().dig.is_some();
        let still = a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop);
        let mut turn = Vec::new();
        match plan {
            Plan::Center { direction, heading, until, tries } if still || tick >= until => {
                turn.push(walk(Intent::Walk { direction: heading, run: false }));
                plan = Plan::Turn { direction, stop_at: tick + TURN, dig_at: tick + TURN + 12, tries };
            }
            Plan::Turn { stop_at, .. } if tick == stop_at => turn.push(walk(Intent::Stop)),
            Plan::Turn { direction, dig_at, tries, .. } if tick >= dig_at => {
                // Mostly a direction that digs something here (on open
                // ground a level or up dig has nothing to take), else the
                // one drawn, refused or not.
                let would = |d| s.dig_volume_of(me, tick, sim.world().terrain(), d).unwrap();
                let open: Vec<_> = directions.into_iter().filter(|&d| would(d).is_ok()).collect();
                if (direction != DigDirection::Down && would(direction).is_err() || open.len() == 1) && tries < 8 {
                    // Nothing for it this way: another heading, to look for a wall.
                    let middle = Terrain::pit_center(Terrain::pit_tile(a.body.position));
                    turn.push(walk(Intent::GoTo { target: middle, run: false }));
                    plan = Plan::Center { direction, heading: heading(dice.below(16)), until: tick + HZ, tries: tries + 1 };
                } else {
                    let direction = if !open.is_empty() && dice.below(8) > 0 { open[dice.below(open.len() as u64) as usize] } else { direction };
                    match would(direction) {
                        Ok(_) => dug.digs[directions.iter().position(|&d| d == direction).unwrap()] += 1,
                        Err(why) => *dug.refused.entry(format!("{why:?}")).or_default() += 1,
                    }
                    turn.push(command(CommandKind::DigFront { agent: me, direction }));
                    // Then the next as soon as it is dug (or now, if refused).
                    (plan, next_at, walking) = (Plan::Free, tick + 1, false);
                }
            }
            Plan::Free if (tick >= next_at || (walking && still)) && !digging => {
                let center = sim.world().terrain().center;
                // Arrived from a walk to a point (by a wall, mostly): dig.
                let roll = if walking && still { 0 } else { dice.below(10) };
                walking = false;
                match roll {
                    // Dig: to the tile's middle, a turn, then the dig.
                    0..=4 => {
                        let middle = Terrain::pit_center(Terrain::pit_tile(a.body.position));
                        turn.push(walk(Intent::GoTo { target: middle, run: false }));
                        let direction = directions[dice.below(3) as usize];
                        // Mostly toward a wall, if one is beside the tile.
                        let volume = sim.world().terrain().volume.as_ref().unwrap();
                        let floor = volume.floor_at(middle, a.avatar().unwrap().height).map_or(0, |(layer, _)| layer);
                        let walls: Vec<u64> = (0..16)
                            .filter(|&i| {
                                let front = Terrain::front_tile(middle, heading(i));
                                (floor..floor + crate::volume::HEADROOM + 1).any(|layer| volume.is_solid(front, layer))
                            })
                            .collect();
                        let i = if !walls.is_empty() && dice.below(4) > 0 { walls[dice.below(walls.len() as u64) as usize] } else { dice.below(16) };
                        plan = Plan::Center { direction, heading: heading(i), until: tick + HZ, tries: 0 };
                    }
                    // Somewhere at random by a 3D route: anywhere on the
                    // map, or near one of its sights, where there are
                    // walls to dig: the west cut, the tunnel, the cave,
                    // the bridge's stairs and the mountain's foot.
                    5..=8 => {
                        let target = match dice.below(6) {
                            0 => FxVec2::new(dice.meters(28), dice.meters(28)),
                            // Beside the cut's and the tunnel's walls.
                            1 => FxVec2::new(Fx::from_int(5) + dice.meters(3), Fx::from_int(if dice.below(2) == 0 { 1 } else { -1 })),
                            2 => FxVec2::new(Fx::from_int(14) + dice.meters(8), Fx::from_int(if dice.below(2) == 0 { 1 } else { -1 })),
                            3 => FxVec2::new(Fx::from_int(-11) + dice.meters(2), Fx::from_int(13) + dice.meters(3)),
                            4 => FxVec2::new(Fx::from_int(-23) + dice.meters(4), Fx::from_int(13) + dice.meters(2)),
                            _ => FxVec2::new(Fx::from_int(2) + dice.meters(2), dice.meters(8)),
                        };
                        turn.push(walk(Intent::GoTo { target: center + target, run: dice.below(2) == 0 }));
                        dug.walks += 1;
                        (next_at, walking) = (tick + 15 * HZ, true);
                    }
                    // A random heading for a while: often onto what was
                    // just dug.
                    _ => {
                        turn.push(walk(Intent::Walk { direction: heading(dice.below(16)), run: false }));
                        next_at = tick + HZ / 2 + dice.below(2 * HZ);
                    }
                }
            }
            _ => {}
        }
        hashes.push(sim.step(&turn).total);
        check(&sim, me, &mut dug);
    }
    (hashes, dug)
}

/// The soak's checks after each tick: the settlement validates, and the
/// avatar and the walker stand free. Every half second it also plans from
/// the walker to the avatar, as the walker does, for the plans' sizes.
fn check(sim: &Simulation, me: AgentIx, dug: &mut Dug) {
    let (world, tick) = (sim.world(), sim.world().tick());
    let s = world.settlement().unwrap();
    s.validate().unwrap_or_else(|e| panic!("tick {tick}: {e}"));
    let avatar = &s.agents[me as usize];
    let feet = avatar.avatar().unwrap().height;
    standing_free(world.terrain(), "the avatar", avatar.body.position, feet, avatar.radius(), tick);
    let walker = s.agents.iter().find(|a| a.id == "walker_0").unwrap();
    let walker_feet = walker.sprite().unwrap().height;
    standing_free(world.terrain(), "the walker", walker.body.position, walker_feet, walker.body.base_radius, tick);
    dug.lowest = dug.lowest.min(feet);
    dug.highest = dug.highest.max(feet);
    dug.walker_lowest = dug.walker_lowest.min(walker_feet);
    dug.walker_highest = dug.walker_highest.max(walker_feet);
    if tick % super::follow::REPLAN_TICKS == 0 {
        let plan = crate::nav::path_toward_at(world.terrain(), walker.body.position, walker_feet, avatar.body.position, Some(feet), walker.body.base_radius, &[]);
        dug.plans += 1;
        dug.unreached += u32::from(!plan.reached);
        dug.most_expanded = dug.most_expanded.max(plan.expanded);
    }
}

/// Two minutes of random walks and digs in the Tunnel Lab: no panic,
/// every invariant every tick, neither the avatar nor the walker ever in a
/// solid cell or without headroom, every plan within the budget, and two
/// runs alike tick for tick.
#[test]
fn the_tunnel_lab_soaks_walking_and_digging_at_random() {
    let (hashes, dug) = tunnel_soak(0x5eed_7a11);
    eprintln!("tunnel soak: {dug:?}");
    assert!(dug.digs.iter().all(|&n| n >= 2) && dug.digs.iter().sum::<u32>() >= 10, "digs of every direction went through: {dug:?}");
    assert!(dug.walks >= 5, "{dug:?}");
    // No plan searches the whole map any more (once 1.2 s, M8.1).
    assert!(dug.plans >= 200 && dug.most_expanded <= crate::nav::PATH_BUDGET, "{dug:?}");
    assert_eq!(hashes, tunnel_soak(0x5eed_7a11).0, "deterministic");
}

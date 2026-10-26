use content::{Content, MoverDef, Rules};
use fx::{Fx, FxVec2, FxVec3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use web_time::Instant;

use crate::hash::{hash_of, StateHasher, TickHash};
use crate::id::{Id, Ids, IdsState, Location};
use crate::messages::{Damage, Eat, EatFailed, Killed, Messages, Spawn};
use crate::mover::{Mover, MoverRow, Movers, HUNGRY_AT, REACH};
use crate::rng::{stream, Rng};
use crate::spatial::{for_each_near, SpatialIndex};
use crate::settlement::Settlement;
use crate::terrain::{Disc, Terrain};
use crate::timer::TimerWheel;

/// Chunks are square columns of this many tiles per side, covering the full height.
pub const CHUNK_TILES: i64 = 32;
const FOOD_MAX: i32 = 100;
const MAX_SPAWN_COMMAND: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldConfig {
    pub seed: u64,
    pub chunks_x: u32,
    pub chunks_y: u32,
    pub max_movers: u32,
}

impl WorldConfig {
    pub fn chunk_count(&self) -> usize {
        (self.chunks_x * self.chunks_y) as usize
    }

    /// The world's extent in tiles.
    pub fn size(&self) -> FxVec2 {
        FxVec2::new(
            Fx::from_int(self.chunks_x as i32 * CHUNK_TILES as i32),
            Fx::from_int(self.chunks_y as i32 * CHUNK_TILES as i32),
        )
    }

    /// The chunk containing `pos`, clamping positions outside the world.
    pub fn chunk_of(&self, pos: FxVec3) -> usize {
        let x = (pos.x.floor_to_int() / CHUNK_TILES).clamp(0, self.chunks_x as i64 - 1);
        let y = (pos.y.floor_to_int() / CHUNK_TILES).clamp(0, self.chunks_y as i64 - 1);
        (y * self.chunks_x as i64 + x) as usize
    }

    /// The tile coordinates of a chunk's minimum corner.
    pub fn chunk_origin(&self, chunk: usize) -> (i64, i64) {
        let chunk = chunk as i64;
        let width = self.chunks_x as i64;
        ((chunk % width) * CHUNK_TILES, (chunk / width) * CHUNK_TILES)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PlayerId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandKind {
    /// Spawns up to `count` movers around `center`.
    SpawnMovers { center: FxVec2, count: u32 },
    /// Sets a mover's velocity, waking it.
    Impulse { mover: Id<Mover>, velocity: FxVec2 },
    /// Digs out the rock column at this tile, if there is one.
    Dig { tile: (i32, i32) },
    /// Switches a settlement haul link on or off.
    SetLink { link: u16, enabled: bool },
    /// Switches a settlement machine on or off.
    SetMachine { producer: u16, enabled: bool },
    /// Switches a settlement defender on or off.
    SetDefender { defender: u16, enabled: bool },
    /// Sets how a player's avatar moves.
    MoveAvatar { agent: u16, intent: crate::settlement::avatar::Intent },
    /// Sets the upper body's facing independently of travel.
    AimAvatar { agent: u16, aim: FxVec2 },
    /// A player's avatar attacks toward `aim` (a direction on the ground).
    Attack { agent: u16, weapon: content::scenario::Weapon, aim: FxVec2, muzzle: [Fx; 3] },
    /// A player's avatar takes units from (or, with `deposit`, gives units to) a depot.
    Transfer { agent: u16, depot: u16, item: u16, units: u32, deposit: bool },
    /// Cancels an avatar's transfer; units in the air still land.
    CancelTransfer { agent: u16 },
    /// Puts a weapon in a player's avatar's hand.
    Equip { agent: u16, weapon: content::scenario::Weapon },
    /// Holds a player's jetpack on or off.
    Jetpack { agent: u16, enabled: bool },
    /// Begins a grounded jump in scenes that enable it.
    Jump { agent: u16 },
    /// Puts a player back at the spawn point.
    Respawn { agent: u16 },
    /// Pile lab: walk to a pile and transfer, `pace` seconds per unit (0: all at once).
    Pile { agent: u16, action: crate::settlement::piles::PileAction, pace: Fx, approach: Option<FxVec2> },
    /// Pile lab: a dispenser adds units to its output pile.
    Dispense { dispenser: u32, units: u32 },
    /// Adds stock to a settlement depot (scripted demos and tests).
    AddStock { depot: u16, item: u16, units: u32 },
    /// Memory trial: start recording, deploy the ghost, or forget it.
    MemoryRecord,
    MemoryDeploy,
    MemoryClear,
    /// The sending player joined: gives them an avatar if they have none.
    Join,
    /// The sending player's name, for their avatar.
    Name { name: String },
    /// A player's avatar digs out a layer of the tile it stands on.
    DigDown { agent: u16 },
    /// The player stopped driving their avatar directly (pause, focus loss,
    /// a controller disconnecting): see `Settlement::release_direct_control`.
    ReleaseDirectControl { agent: u16 },
    /// Holds or releases repair on a nearby cover asset.
    RepairCover { agent: u16, asset: Option<u16> },
    /// Press or release a piano key. Any input device can issue this action.
    PlayPiano { agent: u16, piano: u16, pitch: u8, velocity: u8 },
    /// Enter a parked vehicle or exit the vehicle currently driven.
    ToggleVehicle { agent: u16, vehicle: u16 },
}

impl CommandKind {
    /// The avatar a command drives, if it drives one.
    pub fn avatar(&self) -> Option<u16> {
        match *self {
            CommandKind::MoveAvatar { agent, .. }
            | CommandKind::AimAvatar { agent, .. }
            | CommandKind::Attack { agent, .. }
            | CommandKind::Transfer { agent, .. }
            | CommandKind::CancelTransfer { agent }
            | CommandKind::Jetpack { agent, .. }
            | CommandKind::Jump { agent }
            | CommandKind::Equip { agent, .. }
            | CommandKind::Respawn { agent }
            | CommandKind::DigDown { agent }
            | CommandKind::ReleaseDirectControl { agent }
            | CommandKind::RepairCover { agent, .. }
            | CommandKind::PlayPiano { agent, .. }
            | CommandKind::ToggleVehicle { agent, .. }
            | CommandKind::Pile { agent, .. } => Some(agent),
            _ => None,
        }
    }
}

/// One player input, scheduled for a tick by the network layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub player: PlayerId,
    pub kind: CommandKind,
}

#[derive(Clone, Debug)]
pub struct Chunk {
    pub movers: Movers,
    /// A shared resource, changed only by `Eat` messages and housekeeping.
    pub food: i32,
    /// Movers in this chunk that are awake. A chunk with none is skipped by
    /// every stage, though other chunks' queries still see its movers.
    awake: u32,
    /// Set when anything in the chunk changes, so housekeeping rehashes and
    /// reindexes only changed chunks.
    dirty: bool,
    /// Movers that fell asleep during this tick's self update, with their wake tick.
    new_sleepers: Vec<(Id<Mover>, u64)>,
    hash: u64,
}

impl Chunk {
    fn new() -> Chunk {
        Chunk {
            movers: Movers::default(),
            food: FOOD_MAX,
            awake: 0,
            dirty: true,
            new_sleepers: Vec::new(),
            hash: 0,
        }
    }

    pub fn awake(&self) -> u32 {
        self.awake
    }

    fn compute_hash(&self, index: usize) -> u64 {
        let mut h = StateHasher::default();
        h.write_u64(index as u64);
        h.write(&self.food);
        h.write(&self.awake);
        h.write(&self.movers);
        h.finish()
    }
}

pub struct World {
    config: WorldConfig,
    rules: Rules,
    /// The largest body radius and speed in the rules. Queries widen their
    /// search by these.
    max_body_radius: Fx,
    max_speed: Fx,
    /// The next tick to run.
    tick: u64,
    chunks: Vec<Chunk>,
    /// Kept outside `chunks` so self update can mutate a chunk while reading
    /// every chunk's index.
    indices: Vec<SpatialIndex>,
    ids: Ids<Mover>,
    terrain: Terrain,
    settlement: Option<Box<Settlement>>,
    timers: TimerWheel<Id<Mover>>,
    /// Messages created during apply, handled next tick.
    pending: Messages,
    /// Filled during apply, drained in housekeeping, in apply order.
    despawns: Vec<Id<Mover>>,
    spawns: Vec<MoverRow>,
    /// How long the last tick's stages took.
    times: StageTimes,
}

/// Wall time of each stage of a tick, for profiling. Not state: it never
/// feeds back into the simulation, and it isn't saved or hashed.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTimes {
    pub commands: Duration,
    pub self_update: Duration,
    pub read: Duration,
    pub merge: Duration,
    pub apply: Duration,
    pub housekeeping: Duration,
    pub hash: Duration,
}

impl StageTimes {
    pub fn total(&self) -> Duration {
        self.commands + self.self_update + self.read + self.merge + self.apply + self.housekeeping + self.hash
    }
}

/// Times consecutive stages.
struct Lap(Instant);

impl Lap {
    /// The time since the last lap.
    fn next(&mut self) -> Duration {
        let now = Instant::now();
        now - std::mem::replace(&mut self.0, now)
    }
}

impl World {
    pub fn new(config: WorldConfig, rules: Rules) -> World {
        let count = config.chunk_count();
        World {
            max_body_radius: rules.max_body_radius(),
            max_speed: rules.max_speed(),
            rules,
            tick: 0,
            chunks: (0..count).map(|_| Chunk::new()).collect(),
            indices: (0..count).map(|_| SpatialIndex::default()).collect(),
            ids: Ids::default(),
            terrain: {
                let size = config.size();
                let center = FxVec2::new(size.x.div_int(2), size.y.div_int(2));
                Terrain::flat(center, size.x.min(size.y).div_int(2))
            },
            settlement: None,
            timers: TimerWheel::default(),
            pending: Messages::default(),
            despawns: Vec::new(),
            spawns: Vec::new(),
            times: StageTimes::default(),
            config,
        }
    }

    pub fn config(&self) -> &WorldConfig {
        &self.config
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    /// A new world for a scene, sized to fit its arena, at tick 0.
    pub fn new_scene(seed: u64, content: &Content, setup: &crate::settlement::SceneSetup) -> Result<World, String> {
        let extent = setup.scenario.as_ref().map_or(setup.extent, |s| s.extent);
        let span = (extent.mul_int(2) + Fx::from_int(2)).ceil().floor_to_int() as u32;
        let chunks = span.div_ceil(CHUNK_TILES as u32).max(1);
        let config = WorldConfig { seed, chunks_x: chunks, chunks_y: chunks, max_movers: 0 };
        let mut world = World::new(config, content.rules.clone());
        let settlement = Settlement::from_scene(setup, &mut world.terrain, content)?;
        world.settlement = Some(Box::new(settlement));
        Ok(world)
    }

    /// A new world for a settlement scenario, sized to fit its arena, at tick 0.
    /// `obstacles` are the scene's blocking props, in arena coordinates.
    pub fn new_settlement(
        seed: u64,
        content: &Content,
        scenario: &content::scenario::Scenario,
        obstacles: &[Disc],
    ) -> Result<World, String> {
        let span = (scenario.extent.mul_int(2) + Fx::from_int(2)).ceil().floor_to_int() as u32;
        let chunks = span.div_ceil(CHUNK_TILES as u32).max(1);
        let config = WorldConfig { seed, chunks_x: chunks, chunks_y: chunks, max_movers: 0 };
        let mut world = World::new(config, content.rules.clone());
        let center = world.terrain.center;
        let placed: Vec<Disc> = obstacles
            .iter()
            .map(|o| Disc { center: FxVec2::new(center.x + o.center.x, center.y + o.center.y), radius: o.radius })
            .collect();
        let settlement = Settlement::from_scenario(scenario, &placed, &mut world.terrain, content)?;
        world.settlement = Some(Box::new(settlement));
        Ok(world)
    }

    pub fn settlement(&self) -> Option<&Settlement> {
        self.settlement.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn settlement_mut(&mut self) -> &mut Settlement {
        self.settlement.as_deref_mut().expect("a settlement world")
    }

    /// Replaces the terrain. For building a new world before tick 0; during
    /// play, terrain changes only through commands and messages.
    pub fn set_terrain(&mut self, terrain: Terrain) {
        self.terrain = terrain;
    }

    /// The next tick to run.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn mover_count(&self) -> usize {
        self.chunks.iter().map(|c| c.movers.len()).sum()
    }

    pub fn sleeping_timers(&self) -> usize {
        self.timers.len()
    }

    pub fn mover(&self, id: Id<Mover>) -> Option<MoverRow> {
        let location = self.ids.get(id)?;
        Some(self.chunks[location.chunk as usize].movers.row(location.slot as usize))
    }

    /// Runs one tick with this turn's commands and returns the state hash.
    pub fn step(&mut self, turn: &[Command]) -> TickHash {
        let mut lap = Lap(Instant::now());
        self.apply_commands(turn);
        let commands = lap.next();
        self.self_update();
        let self_update = lap.next();
        let outboxes = self.read_phase();
        let read = lap.next();
        let messages = self.merge(outboxes);
        let merge = lap.next();
        self.apply_messages(messages);
        let apply = lap.next();
        self.housekeeping();
        let housekeeping = lap.next();
        let hash = self.hash();
        self.tick += 1;
        self.times = StageTimes { commands, self_update, read, merge, apply, housekeeping, hash: lap.next() };
        hash
    }

    /// How long each stage of the last tick took.
    pub fn stage_times(&self) -> StageTimes {
        self.times
    }

    // Stage 1: apply inputs, in player order.
    fn apply_commands(&mut self, turn: &[Command]) {
        let mut order: Vec<usize> = (0..turn.len()).collect();
        order.sort_by_key(|&i| turn[i].player);
        for (n, i) in order.into_iter().enumerate() {
            let mut rng = Rng::new(self.config.seed, self.tick, n as u64, stream::COMMAND);
            // Players only drive their own avatar.
            // The memory trial acts on the first avatar, so only its owner may
            // record, deploy or clear.
            let memory = matches!(turn[i].kind, CommandKind::MemoryRecord | CommandKind::MemoryDeploy | CommandKind::MemoryClear);
            let driven = if memory {
                Some(self.settlement.as_ref().and_then(|s| s.player()))
            } else {
                turn[i].kind.avatar().map(Some)
            };
            if let Some(agent) = driven {
                let owner = agent
                    .and_then(|agent| self.settlement.as_ref().and_then(|s| s.agents.get(agent as usize)))
                    .and_then(|a| a.owner);
                if owner != Some(turn[i].player) {
                    continue;
                }
            }
            match turn[i].kind {
                CommandKind::SpawnMovers { center, count } => self.spawn_movers(center, count, &mut rng),
                CommandKind::Impulse { mover, velocity } => {
                    let Some(location) = self.ids.get(mover) else { continue };
                    let (chunk, slot) = (location.chunk as usize, location.slot as usize);
                    self.wake(chunk, slot);
                    let max_speed = self.def(self.chunks[chunk].movers.kind[slot]).max_speed;
                    self.chunks[chunk].movers.vel[slot] = clamp_speed(FxVec3::new(velocity.x, velocity.y, Fx::ZERO), max_speed);
                }
                CommandKind::Dig { tile } => {
                    self.terrain.dig(tile);
                }
                CommandKind::SetLink { link, enabled } => {
                    if let Some(s) = &mut self.settlement {
                        s.set_link(link as usize, enabled);
                    }
                }
                CommandKind::SetMachine { producer, enabled } => {
                    if let Some(s) = &mut self.settlement {
                        s.set_machine(producer as usize, enabled);
                    }
                }
                CommandKind::SetDefender { defender, enabled } => {
                    if let Some(s) = &mut self.settlement {
                        s.set_defender(defender as usize, enabled);
                    }
                }
                CommandKind::MoveAvatar { agent, ref intent } => {
                    if let Some(s) = &mut self.settlement {
                        s.move_avatar(agent as usize, intent.clone(), &self.terrain);
                    }
                }
                CommandKind::AimAvatar { agent, aim } => {
                    if let Some(s) = &mut self.settlement {
                        s.aim_avatar(agent as usize, aim);
                    }
                }
                CommandKind::Attack { agent, weapon, aim, muzzle } => {
                    if let Some(s) = &mut self.settlement {
                        s.attack(self.tick, agent as usize, weapon, aim, muzzle);
                    }
                }
                CommandKind::Transfer { agent, depot, item, units, deposit } => {
                    if let Some(s) = &mut self.settlement {
                        s.transfer(self.tick, agent as usize, depot as usize, item as usize, units, deposit);
                    }
                }
                CommandKind::CancelTransfer { agent } => {
                    if let Some(s) = &mut self.settlement {
                        s.cancel_transfer(agent as usize);
                    }
                }
                CommandKind::Equip { agent, weapon } => {
                    if let Some(s) = &mut self.settlement {
                        s.equip_weapon(agent as usize, weapon);
                    }
                }
                CommandKind::Jetpack { agent, enabled } => {
                    if let Some(s) = &mut self.settlement {
                        s.jetpack(agent as usize, enabled);
                    }
                }
                CommandKind::Jump { agent } => {
                    if let Some(s) = &mut self.settlement {
                        s.jump(agent as usize, self.tick);
                    }
                }
                CommandKind::Respawn { agent } => {
                    if let Some(s) = &mut self.settlement {
                        s.respawn(agent as usize, &mut self.terrain);
                    }
                }
                CommandKind::DigDown { agent } => {
                    if let Some(s) = &mut self.settlement {
                        s.dig(self.tick, agent as usize, &self.terrain);
                    }
                }
                CommandKind::ReleaseDirectControl { agent } => {
                    if let Some(s) = &mut self.settlement {
                        s.release_direct_control(agent as usize);
                    }
                }
                CommandKind::RepairCover { agent, asset } => {
                    if let Some(s) = &mut self.settlement {
                        if let Some(a) = s.agents.get_mut(agent as usize).filter(|a| a.controlled) {
                            a.avatar.repair = asset.filter(|&asset| self.terrain.destructibles.iter()
                                .any(|piece| piece.asset == asset && piece.repairable && piece.health < piece.max_health));
                        }
                    }
                }
                CommandKind::PlayPiano { agent, piano, pitch, velocity } => {
                    if let Some(s) = &mut self.settlement {
                        s.play_piano(agent as usize, piano, pitch, velocity);
                    }
                }
                CommandKind::ToggleVehicle { agent, vehicle } => {
                    if let Some(s) = &mut self.settlement {
                        s.toggle_vehicle(self.tick, agent as usize, vehicle, &self.terrain);
                    }
                }
                CommandKind::Pile { agent, action, pace, approach } => {
                    if let Some(s) = &mut self.settlement {
                        s.pile_action(self.tick, &self.terrain, agent as usize, action, pace, approach);
                    }
                }
                CommandKind::Dispense { dispenser, units } => {
                    if let Some(s) = &mut self.settlement {
                        s.dispense(dispenser, units);
                    }
                }
                CommandKind::AddStock { depot, item, units } => {
                    if let Some(s) = &mut self.settlement {
                        s.add_stock(depot as usize, item as usize, units);
                    }
                }
                CommandKind::MemoryRecord => {
                    if let Some(s) = &mut self.settlement {
                        s.memory_record();
                    }
                }
                CommandKind::MemoryDeploy => {
                    if let Some(s) = &mut self.settlement {
                        s.memory_deploy();
                    }
                }
                CommandKind::MemoryClear => {
                    if let Some(s) = &mut self.settlement {
                        s.memory_clear();
                    }
                }
                CommandKind::Name { ref name } => {
                    if let Some(s) = &mut self.settlement {
                        s.rename(turn[i].player, name);
                    }
                }
                CommandKind::Join => {
                    if let Some(s) = &mut self.settlement {
                        s.join(turn[i].player);
                    }
                }
            }
        }
    }

    fn spawn_movers(&mut self, center: FxVec2, count: u32, rng: &mut Rng) {
        let size = self.config.size();
        if center.x < Fx::ZERO || center.y < Fx::ZERO || center.x >= size.x || center.y >= size.y {
            return;
        }
        let spread = Fx::from_int(8);
        for _ in 0..count.min(MAX_SPAWN_COMMAND) {
            if self.ids.allocated() >= self.config.max_movers {
                return;
            }
            let pos = FxVec2::new(
                center.x + rng.range(-spread, spread),
                center.y + rng.range(-spread, spread),
            );
            let Some(kind) = self.pick_kind(rng) else { return };
            let def = self.def(kind).clone();
            let half = def.max_speed.div_int(2);
            let row = MoverRow {
                id: self.ids.alloc(),
                kind,
                pos: clamp_to_world(FxVec3::new(pos.x, pos.y, Fx::ZERO), size),
                vel: FxVec3::new(rng.range(-half, half), rng.range(-half, half), Fx::ZERO),
                radius: def.radius,
                hp: def.max_hp - rng.below(def.max_hp as u64 / 2) as i32,
                hunger: rng.below(HUNGRY_AT as u64) as i32,
                kills: 0,
                sleep_until: 0,
            };
            self.spawns.push(row);
        }
    }

    /// Picks a mover kind by spawn weight.
    fn pick_kind(&self, rng: &mut Rng) -> Option<u16> {
        let total: u64 = self.rules.movers.iter().map(|m| m.spawn_weight as u64).sum();
        if total == 0 {
            return None;
        }
        let mut roll = rng.below(total);
        for (kind, def) in self.rules.movers.iter().enumerate() {
            if roll < def.spawn_weight as u64 {
                return Some(kind as u16);
            }
            roll -= def.spawn_weight as u64;
        }
        unreachable!("roll is below the total weight")
    }

    fn def(&self, kind: u16) -> &MoverDef {
        &self.rules.movers[kind as usize]
    }

    // Stage 2: self update. Each mover changes only itself.
    fn self_update(&mut self) {
        if let Some(settlement) = &mut self.settlement {
            settlement.self_update(self.tick, &self.terrain);
        }
        let World { chunks, indices, config, tick, rules, max_body_radius, .. } = self;
        let (config, tick, indices, rules, max_body_radius) = (&*config, *tick, &*indices, &*rules, *max_body_radius);
        chunks.par_iter_mut().for_each(|chunk| {
            if chunk.awake == 0 {
                return;
            }
            chunk.dirty = true;
            let size = config.size();
            let movers = &mut chunk.movers;
            for slot in 0..movers.len() {
                if movers.sleep_until[slot] != 0 || movers.hp[slot] <= 0 {
                    continue;
                }
                let id = movers.id[slot];
                let mut rng = Rng::new(config.seed, tick, id.to_bits(), stream::SELF_UPDATE);
                if rng.chance(1, 400) {
                    let wake = tick + 30 + rng.below(60);
                    movers.sleep_until[slot] = wake;
                    chunk.awake -= 1;
                    chunk.new_sleepers.push((id, wake));
                    continue;
                }

                let pos = movers.pos[slot];
                let radius = movers.radius[slot];
                let max_speed = rules.movers[movers.kind[slot] as usize].max_speed;
                let push = separation(indices, config, max_body_radius, id, pos, radius);

                let mut vel = movers.vel[slot];
                if rng.chance(1, 30) {
                    let jitter = max_speed.div_int(5);
                    vel += FxVec3::new(rng.range(-jitter, jitter), rng.range(-jitter, jitter), Fx::ZERO);
                }
                vel = clamp_speed(vel, max_speed);
                let mut next = pos + vel + clamp_speed(push, max_speed);
                // Bounce off the world's edges.
                if next.x < Fx::ZERO || next.x >= size.x {
                    vel.x = -vel.x;
                }
                if next.y < Fx::ZERO || next.y >= size.y {
                    vel.y = -vel.y;
                }
                next = clamp_to_world(next, size);
                movers.pos[slot] = next;
                movers.vel[slot] = vel;
                movers.hunger[slot] += 1;
            }
        });
    }

    // Stage 3: read phase. Reads the whole world, writes only to its chunk's outbox.
    fn read_phase(&self) -> Vec<Messages> {
        let mut outboxes: Vec<Messages> = (0..self.chunks.len())
            .into_par_iter()
            .map(|chunk| self.read_chunk(chunk))
            .collect();
        // Settlement agents file their messages under the chunk they stand in,
        // so they take part in the shuffled chunk order like everyone else.
        if let Some(settlement) = &self.settlement {
            for (position, mut messages) in settlement.read(self.tick, &self.terrain) {
                let chunk = self.config.chunk_of(FxVec3::new(position.x, position.y, Fx::ZERO));
                outboxes[chunk].append(&mut messages);
            }
        }
        outboxes
    }

    fn read_chunk(&self, chunk_index: usize) -> Messages {
        let mut out = Messages::default();
        let chunk = &self.chunks[chunk_index];
        if chunk.awake == 0 {
            return out;
        }
        let movers = &chunk.movers;
        for slot in 0..movers.len() {
            if movers.sleep_until[slot] != 0 || movers.hp[slot] <= 0 {
                continue;
            }
            let id = movers.id[slot];
            let pos = movers.pos[slot];
            let radius = movers.radius[slot];
            let mut rng = Rng::new(self.config.seed, self.tick, id.to_bits(), stream::READ);

            let hunger = movers.hunger[slot];
            if hunger >= HUNGRY_AT && hunger % 10 == 0 {
                let amount = 1 + (radius.floor_to_int() as i32);
                out.eat.push(Eat { mover: id, chunk: chunk_index as u32, amount });
            }

            if rng.chance(1, 20) {
                if let Some(target) = self.nearest_target(id, pos, radius) {
                    out.damage.push(Damage { source: id, target, amount: 1 });
                }
            }

            if movers.kills[slot] >= 3 && rng.chance(1, 50) {
                let offset = FxVec2::from_angle(rng.range(Fx::ZERO, Fx::TAU)) * (radius.mul_int(3));
                let child = clamp_to_world(pos + FxVec3::new(offset.x, offset.y, Fx::ZERO), self.config.size());
                out.spawn.push(Spawn { parent: id, pos: child, kind: movers.kind[slot] });
            }
        }
        out
    }

    /// The closest live mover within reach, by current position. Ties go to the lower ID.
    fn nearest_target(&self, id: Id<Mover>, pos: FxVec3, radius: Fx) -> Option<Id<Mover>> {
        let search = radius + REACH + self.max_body_radius + self.max_speed;
        let mut best: Option<(Fx, Id<Mover>)> = None;
        for_each_near(&self.indices, &self.config, pos.xy(), search, |chunk, body| {
            if body.id == id {
                return;
            }
            let other = &self.chunks[chunk].movers;
            let slot = body.slot as usize;
            if other.hp[slot] <= 0 {
                return;
            }
            let gap = (other.pos[slot] - pos).xy().length() - radius - body.radius;
            if gap <= REACH && best.is_none_or(|b| (gap, body.id) < b) {
                best = Some((gap, body.id));
            }
        });
        best.map(|(_, target)| target)
    }

    // Stage 4: merge. Last tick's apply output first, then outboxes in a
    // shuffled chunk order seeded by the tick.
    fn merge(&mut self, mut outboxes: Vec<Messages>) -> Messages {
        let mut order: Vec<u32> = (0..outboxes.len() as u32).collect();
        Rng::new(self.config.seed, self.tick, 0, stream::CHUNK_ORDER).shuffle(&mut order);
        let mut messages = std::mem::take(&mut self.pending);
        for chunk in order {
            messages.append(&mut outboxes[chunk as usize]);
        }
        messages
    }

    // Stage 5: apply phase. Types in a fixed order; within a type, queue
    // order. The earlier message wins a conflict.
    fn apply_messages(&mut self, messages: Messages) {
        for message in messages.damage {
            let Some((chunk, slot)) = self.locate(message.target) else { continue };
            if self.chunks[chunk].movers.hp[slot] <= 0 {
                continue; // Already killed earlier in the queue.
            }
            self.wake(chunk, slot);
            let hp = &mut self.chunks[chunk].movers.hp[slot];
            *hp -= message.amount;
            if *hp <= 0 {
                self.despawns.push(message.target);
                self.pending.killed.push(Killed { mover: message.source });
            }
        }

        for message in messages.eat {
            let Some((chunk, slot)) = self.locate(message.mover) else { continue };
            let food = &mut self.chunks[message.chunk as usize].food;
            if *food < message.amount {
                self.pending.eat_failed.push(EatFailed { mover: message.mover });
                continue;
            }
            *food -= message.amount;
            self.chunks[message.chunk as usize].dirty = true;
            self.wake(chunk, slot);
            let max_hp = self.def(self.chunks[chunk].movers.kind[slot]).max_hp;
            let movers = &mut self.chunks[chunk].movers;
            movers.hunger[slot] = 0;
            movers.hp[slot] = (movers.hp[slot] + 1).min(max_hp);
        }

        for message in messages.spawn {
            let Some((chunk, slot)) = self.locate(message.parent) else { continue };
            if self.ids.allocated() >= self.config.max_movers {
                continue;
            }
            self.wake(chunk, slot);
            self.chunks[chunk].movers.kills[slot] = 0;
            let id = self.ids.alloc();
            let def = self.def(message.kind);
            let (radius, max_hp) = (def.radius, def.max_hp);
            let mut rng = Rng::new(self.config.seed, self.tick, id.to_bits(), stream::SPAWN);
            self.spawns.push(MoverRow {
                id,
                kind: message.kind,
                pos: message.pos,
                vel: FxVec3::ZERO,
                radius,
                hp: max_hp / 4 + 1 + rng.below(max_hp as u64 / 4 + 1) as i32,
                hunger: 0,
                kills: 0,
                sleep_until: 0,
            });
        }

        for message in messages.killed {
            let Some((chunk, slot)) = self.locate(message.mover) else { continue };
            self.wake(chunk, slot);
            self.chunks[chunk].movers.kills[slot] += 1;
        }

        for message in messages.eat_failed {
            let Some((chunk, slot)) = self.locate(message.mover) else { continue };
            self.wake(chunk, slot);
            // Wander off and try again in a while.
            let movers = &mut self.chunks[chunk].movers;
            movers.vel[slot] = -movers.vel[slot];
            movers.hunger[slot] = HUNGRY_AT - 30;
        }

        if let Some(settlement) = &mut self.settlement {
            let tick = self.tick;
            settlement.economy(tick);
            settlement.resolve_docks(messages.claim_dock);
            for message in messages.retry {
                settlement.retry(tick, message);
            }
            for message in messages.equip {
                settlement.equip(tick, message);
            }
            for message in messages.reserve {
                settlement.reserve(tick, message);
            }
            for message in messages.attach {
                settlement.attach(tick, &self.terrain, message);
            }
            for message in messages.load {
                settlement.load(tick, message);
            }
            for message in messages.unload {
                settlement.unload(tick, message);
            }
            for message in messages.manual_unit {
                settlement.manual_unit(tick, message);
            }
            for step in messages.pile_step {
                settlement.pile_step(tick, &self.terrain, step.agent);
            }
            for message in messages.strike {
                let agent = message.agent;
                let cover = message.cover;
                let cover_damage = if message.weapon == content::scenario::Weapon::Laser
                    && settlement.vehicle_of(agent).is_some_and(|(_, vehicle)| vehicle.phase.driving(agent)) { 2 } else { 1 };
                let hits = settlement.strike(tick, message);
                if let Some(index) = cover {
                    self.terrain.damage_cover(index as usize, cover_damage);
                }
                settlement.swing_resolved(agent, tick, hits);
            }
            for message in messages.projectile_hit {
                settlement.projectile_hit(tick, message);
            }
            for message in messages.projectile_blocked {
                let cover = message.cover;
                settlement.projectile_blocked(tick, message);
                if let Some(index) = cover {
                    self.terrain.damage_cover(index as usize, 1);
                }
            }
            for message in messages.projectile_expired {
                settlement.projectile_expired(message);
            }
            for message in messages.breach {
                settlement.breach(message);
            }
            for message in messages.fire {
                settlement.fire(tick, message);
            }
        }
        self.apply_repairs();
    }

    fn apply_repairs(&mut self) {
        let Some(settlement) = &self.settlement else { return };
        for index in 0..self.terrain.destructibles.len() {
            let piece = &self.terrain.destructibles[index];
            let held = settlement.agents.iter().any(|agent| agent.controlled
                && (agent.avatar.max_health == 0 || agent.avatar.health > 0)
                && agent.avatar.repair == Some(piece.asset)
                && self.terrain.asset_in_repair_reach(agent.position, piece.asset));
            self.terrain.repair_cover(index, held);
        }
    }

    // Stage 6: housekeeping.
    fn housekeeping(&mut self) {
        if let Some(settlement) = &mut self.settlement {
            settlement.combat_housekeeping(self.tick, &self.terrain);
            settlement.avatar_housekeeping(self.tick);
            settlement.dig_housekeeping(self.tick, &mut self.terrain);
            settlement.pile_housekeeping(self.tick);
            settlement.memory_housekeeping(&mut self.terrain);
        }
        self.despawn();
        self.move_between_chunks();
        self.insert_spawns();
        if self.tick % 8 == 0 {
            for chunk in &mut self.chunks {
                if chunk.awake > 0 && chunk.food < FOOD_MAX {
                    chunk.food += 1;
                    chunk.dirty = true;
                }
            }
        }
        self.update_timers();

        let config = &self.config;
        self.indices
            .par_iter_mut()
            .zip(self.chunks.par_iter_mut())
            .enumerate()
            .for_each(|(index, (spatial, chunk))| {
                if chunk.dirty {
                    spatial.rebuild(config.chunk_origin(index), &chunk.movers);
                    chunk.hash = chunk.compute_hash(index);
                    chunk.dirty = false;
                }
            });
    }

    fn despawn(&mut self) {
        for id in std::mem::take(&mut self.despawns) {
            let Some((chunk, slot)) = self.locate(id) else { continue };
            self.remove_row(chunk, slot);
            self.ids.free(id);
        }
    }

    /// Moves movers whose position left their chunk. Order: source chunk
    /// ascending, then slot descending, so pending slots stay valid.
    fn move_between_chunks(&mut self) {
        let mut moves = Vec::new();
        for (index, chunk) in self.chunks.iter().enumerate() {
            if !chunk.dirty {
                continue;
            }
            for slot in (0..chunk.movers.len()).rev() {
                if self.config.chunk_of(chunk.movers.pos[slot]) != index {
                    moves.push((index, slot));
                }
            }
        }
        for (chunk, slot) in moves {
            let row = self.remove_row(chunk, slot);
            self.insert_row(row);
        }
    }

    fn insert_spawns(&mut self) {
        for row in std::mem::take(&mut self.spawns) {
            self.insert_row(row);
        }
    }

    fn update_timers(&mut self) {
        for chunk in &mut self.chunks {
            for (id, wake) in chunk.new_sleepers.drain(..) {
                self.timers.schedule(wake, id);
            }
        }
        let mut due = Vec::new();
        self.timers.drain_due(self.tick + 1, &mut due);
        for id in due {
            let Some((chunk, slot)) = self.locate(id) else { continue };
            // Skip stale timers: the mover was woken early and may sleep again later.
            if self.chunks[chunk].movers.sleep_until[slot] == self.tick + 1 {
                self.wake(chunk, slot);
            }
        }
    }

    fn remove_row(&mut self, chunk: usize, slot: usize) -> MoverRow {
        let c = &mut self.chunks[chunk];
        c.dirty = true;
        let row = c.movers.swap_remove(slot);
        if row.awake() {
            c.awake -= 1;
        }
        if slot < c.movers.len() {
            let moved = c.movers.id[slot];
            self.ids.set_location(moved, Location { chunk: chunk as u32, slot: slot as u32 });
        }
        row
    }

    fn insert_row(&mut self, row: MoverRow) {
        let chunk = self.config.chunk_of(row.pos);
        let c = &mut self.chunks[chunk];
        c.dirty = true;
        if row.awake() {
            c.awake += 1;
        }
        let location = Location { chunk: chunk as u32, slot: c.movers.len() as u32 };
        self.ids.set_location(row.id, location);
        c.movers.push(row);
    }

    fn locate(&self, id: Id<Mover>) -> Option<(usize, usize)> {
        self.ids.get(id).map(|l| (l.chunk as usize, l.slot as usize))
    }

    fn wake(&mut self, chunk: usize, slot: usize) {
        let c = &mut self.chunks[chunk];
        c.dirty = true;
        if c.movers.sleep_until[slot] != 0 {
            c.movers.sleep_until[slot] = 0;
            c.awake += 1;
        }
    }

    fn hash(&self) -> TickHash {
        let chunks: Vec<u64> = self.chunks.iter().map(|c| c.hash).collect();
        let subsystems = vec![
            ("tick", hash_of(&self.tick)),
            ("chunks", hash_of(&chunks)),
            ("ids", hash_of(&self.ids)),
            ("terrain", hash_of(&self.terrain)),
            ("settlement", self.settlement.as_ref().map_or(0, |s| hash_of(s.as_ref()))),
            ("timers", hash_of(&self.timers)),
            ("pending", hash_of(&self.pending)),
        ];
        let mut total = StateHasher::default();
        for (_, hash) in &subsystems {
            total.write_u64(*hash);
        }
        TickHash { tick: self.tick, total: total.finish(), subsystems, chunks }
    }

    /// The complete state between ticks, for saves. Derived data (locations,
    /// awake counts, spatial indices, hashes) is left out and rebuilt on load.
    pub fn to_state(&self) -> WorldState {
        debug_assert!(self.despawns.is_empty() && self.spawns.is_empty());
        WorldState {
            config: self.config.clone(),
            tick: self.tick,
            ids: self.ids.to_state(),
            terrain: self.terrain.clone(),
            settlement: self.settlement.as_deref().cloned(),
            timers: self.timers.to_state(),
            pending: self.pending.clone(),
            chunks: self
                .chunks
                .iter()
                .map(|c| ChunkState {
                    food: c.food,
                    movers: (0..c.movers.len()).map(|slot| c.movers.row(slot)).collect(),
                })
                .collect(),
        }
    }

    /// Rebuilds a world from saved state. The result continues exactly as the
    /// saved world would have.
    pub fn from_state(state: WorldState, rules: Rules) -> Result<World, String> {
        if state.chunks.len() != state.config.chunk_count() {
            return Err(format!("{} chunks saved, config needs {}", state.chunks.len(), state.config.chunk_count()));
        }
        let timers = TimerWheel::from_state(state.timers).ok_or("malformed timer wheel")?;
        let mut world = World::new(state.config, rules);
        world.tick = state.tick;
        world.ids = Ids::from_state(&state.ids);
        world.terrain = state.terrain;
        world.settlement = state.settlement.map(Box::new);
        if let Some(settlement) = &mut world.settlement {
            settlement.set_characters(&world.rules.characters)?;
        }
        world.timers = timers;
        world.pending = state.pending;
        for (index, saved) in state.chunks.into_iter().enumerate() {
            world.chunks[index].food = saved.food;
            for row in saved.movers {
                if row.kind as usize >= world.rules.movers.len() {
                    return Err(format!("{:?} has unknown kind {}", row.id, row.kind));
                }
                if !world.ids.is_allocated(row.id) || world.ids.get(row.id).is_some() {
                    return Err(format!("{:?} is not allocated or appears twice", row.id));
                }
                if world.config.chunk_of(row.pos) != index {
                    return Err(format!("{:?} is saved in chunk {index} but lies outside it", row.id));
                }
                world.insert_row(row);
            }
        }
        if world.mover_count() != world.ids.allocated() as usize {
            return Err("allocated IDs do not match saved movers".into());
        }
        let config = &world.config;
        world.indices.par_iter_mut().zip(world.chunks.par_iter_mut()).enumerate().for_each(
            |(index, (spatial, chunk))| {
                spatial.rebuild(config.chunk_origin(index), &chunk.movers);
                chunk.hash = chunk.compute_hash(index);
                chunk.dirty = false;
            },
        );
        Ok(world)
    }

    /// Checks internal bookkeeping. For tests; panics on the first problem.
    pub fn check_invariants(&self) {
        let mut count = 0;
        for (index, chunk) in self.chunks.iter().enumerate() {
            let awake = chunk.movers.sleep_until.iter().filter(|&&s| s == 0).count() as u32;
            assert_eq!(chunk.awake, awake, "awake count of chunk {index}");
            for slot in 0..chunk.movers.len() {
                let id = chunk.movers.id[slot];
                let location = Location { chunk: index as u32, slot: slot as u32 };
                assert_eq!(self.ids.get(id), Some(location), "location of {id:?}");
                assert_eq!(self.config.chunk_of(chunk.movers.pos[slot]), index, "chunk of {id:?}");
                count += 1;
            }
        }
        assert_eq!(self.ids.allocated() as usize, count, "allocated IDs");
    }
}

/// Soft, mass-weighted push away from overlapping neighbors, read from the
/// spatial index (last tick's positions).
fn separation(
    indices: &[SpatialIndex],
    config: &WorldConfig,
    max_body_radius: Fx,
    id: Id<Mover>,
    pos: FxVec3,
    radius: Fx,
) -> FxVec3 {
    let mut push = FxVec2::ZERO;
    let mass = radius * radius;
    for_each_near(indices, config, pos.xy(), radius + max_body_radius, |_, body| {
        if body.id == id {
            return;
        }
        let delta = (pos - body.pos).xy();
        let reach = radius + body.radius;
        if delta.x.abs() >= reach || delta.y.abs() >= reach {
            return;
        }
        let distance = delta.length();
        if distance >= reach {
            return;
        }
        let direction = if distance == Fx::ZERO {
            // Exactly on top of each other: split by ID so both sides agree.
            FxVec2::new(if id < body.id { -Fx::ONE } else { Fx::ONE }, Fx::ZERO)
        } else {
            delta / distance
        };
        let other_mass = body.radius * body.radius;
        let share = other_mass / (mass + other_mass);
        push += direction * ((reach - distance) * share).div_int(2);
    });
    FxVec3::new(push.x, push.y, Fx::ZERO)
}

fn clamp_speed(vel: FxVec3, max_speed: Fx) -> FxVec3 {
    let speed = vel.length();
    if speed > max_speed {
        vel * (max_speed / speed)
    } else {
        vel
    }
}

fn clamp_to_world(pos: FxVec3, size: FxVec2) -> FxVec3 {
    let max_x = size.x - Fx::EPSILON;
    let max_y = size.y - Fx::EPSILON;
    FxVec3::new(pos.x.clamp(Fx::ZERO, max_x), pos.y.clamp(Fx::ZERO, max_y), pos.z)
}

/// A world plus the native thread pool that runs its parallel stages.
pub struct Simulation {
    world: World,
    #[cfg(not(target_arch = "wasm32"))]
    pool: rayon::ThreadPool,
}

impl Simulation {
    pub fn new(config: WorldConfig, rules: Rules, threads: usize) -> Simulation {
        Simulation::from_world(World::new(config, rules), threads)
    }

    /// Wraps a world, for example one just loaded from a save.
    pub fn from_world(world: World, threads: usize) -> Simulation {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = threads;
            return Simulation { world };
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("build simulation thread pool");
            Simulation { world, pool }
        }
    }

    pub fn into_world(self) -> World {
        self.world
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    #[cfg(test)]
    pub(crate) fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn step(&mut self, turn: &[Command]) -> TickHash {
        #[cfg(target_arch = "wasm32")]
        {
            return self.world.step(turn);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Simulation { world, pool } = self;
            pool.install(|| world.step(turn))
        }
    }
}

/// The saved form of a [`World`], between ticks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldState {
    pub config: WorldConfig,
    /// The next tick to run.
    pub tick: u64,
    pub ids: IdsState,
    pub terrain: Terrain,
    pub settlement: Option<Settlement>,
    pub timers: Vec<Vec<(u64, Id<Mover>)>>,
    /// Messages created during the last apply phase, handled next tick.
    pub pending: Messages,
    pub chunks: Vec<ChunkState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkState {
    pub food: i32,
    pub movers: Vec<MoverRow>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World").field("tick", &self.tick).field("movers", &self.mover_count()).finish_non_exhaustive()
    }
}

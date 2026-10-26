//! Messages: every interaction between entities, or with a shared resource,
//! is a message. There is one queue per message type, and types are applied in
//! the order they are declared in `message_queues!`.

use fx::{Fx, FxVec2, FxVec3};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};
use crate::id::Id;
use crate::mover::Mover;
use crate::settlement::{CarryJob, InventoryRef};

/// One mover attacks another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Damage {
    pub source: Id<Mover>,
    pub target: Id<Mover>,
    pub amount: i32,
}

/// A mover takes food from a chunk. Contested: when the food runs out, later
/// messages in the order are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eat {
    pub mover: Id<Mover>,
    pub chunk: u32,
    pub amount: i32,
}

/// A mover asks for a child. Rejected when the world is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spawn {
    pub parent: Id<Mover>,
    pub pos: FxVec3,
    pub kind: u16,
}

/// Sent during apply to a mover whose attack killed its target. Handled next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Killed {
    pub mover: Id<Mover>,
}

/// Sent during apply to a mover whose `Eat` was rejected. Handled next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EatFailed {
    pub mover: Id<Mover>,
}

/// How an agent wants a depot's dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockClaim {
    /// Using it (loading, unloading, attaching) or still leaving it.
    Holding,
    /// Heading to it, this far away.
    Approaching(Fx),
}

/// A settlement agent claims a depot's dock for next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimDock {
    pub depot: u16,
    pub agent: u16,
    pub claim: DockClaim,
}

/// An idle agent found no work; it waits before looking again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retry {
    pub agent: u16,
}

/// An agent asks to fetch its equipment, with the route there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equip {
    pub agent: u16,
    pub route: Vec<FxVec2>,
}

/// An agent asks for a haul job: up to `units` along `link`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reserve {
    pub agent: u16,
    pub link: u16,
    pub units: u32,
    pub route: Vec<FxVec2>,
}

/// An agent at the equipment dock takes its equipment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attach {
    pub agent: u16,
}

/// An agent at its pickup dock loads one unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Load {
    pub agent: u16,
}

/// An agent at its drop dock unloads one unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unload {
    pub agent: u16,
}

/// A player's attack resolves against these threats, in damage order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strike {
    pub agent: u16,
    pub weapon: content::scenario::Weapon,
    pub origin: FxVec2,
    pub forward: FxVec2,
    pub muzzle: [Fx; 3],
    pub targets: Vec<u64>,
    /// Where an arc coil's beam ends.
    pub beam_end: Option<FxVec2>,
    /// Index of the destructible piece stopped by a laser, if any.
    pub cover: Option<u16>,
    /// A laser beam stops at another live player, standing at `beam_end`.
    pub hit_player: bool,
    /// The tick the `Attack` that swung it applied on (`Swing::pressed`),
    /// which keys the strike's predicted events.
    pub started: u64,
}

/// A projectile reached a threat this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectileHit {
    pub projectile: u64,
    pub target: u64,
}

/// A projectile hit an obstacle this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectileBlocked {
    pub projectile: u64,
    pub at: FxVec2,
    pub cover: Option<u16>,
}

/// A projectile reached the end of its flight without hitting anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectileExpired {
    pub projectile: u64,
}

/// A threat reached the end of its route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breach {
    pub threat: u64,
}

/// A threat with an `attack` can start a swing at a hurtable player in its
/// reach, `distance` meters away. Stage 5 keeps the nearest player for each
/// threat, the lowest agent index on a tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartSwing {
    pub threat: u64,
    pub agent: u16,
    pub distance: Fx,
}

/// A threat's windup ends this tick. `hit` if its player was alive and in
/// reach then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwingLands {
    pub threat: u64,
    pub agent: u16,
    pub hit: bool,
}

impl StateHash for StartSwing {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.threat);
        h.write(&self.agent);
        h.write(&self.distance);
    }
}

impl StateHash for SwingLands {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.threat);
        h.write(&self.agent);
        h.write(&self.hit);
    }
}

/// A defender shoots at a threat's current position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fire {
    pub defender: u16,
    pub target: u64,
    pub to: FxVec2,
}

/// The player's pile job moves its next unit (or all of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PileStep {
    pub agent: u16,
}

impl StateHash for PileStep {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

/// A player's carry job (`Carry::job`) moves its next unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualUnit {
    pub agent: u16,
}

/// A player's action let go of the avatar's pile haul (`Action::Move`, an
/// `Attack` with an offered weapon, `Respawn`). Ends the haul if it is still
/// the one the action saw: a `Pile` command later in the same turn starts a
/// new one (`Hauler::job_serial`), which stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropHaul {
    pub agent: u16,
    pub serial: u64,
}

/// A player let go of direct control (`Action::Release`): the piano keys
/// their avatar held, up to `serial` (`Settlement::piano_serial` when the
/// action applied), are released. A key pressed later in the same turn stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropNotes {
    pub agent: u16,
    pub serial: u64,
}

/// A player respawned (`Action::Respawn`): the memory trial starts over and
/// its door closes again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartMemory {
    pub agent: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StockChange {
    /// A job the agent took on (`Action::Transfer`) holds its units on the
    /// other side: stock out of a depot, or room in it for a deposit.
    Reserve,
    /// A job that ended early (`Carry::drop_carry`) gives back what it held
    /// there.
    Release,
}

/// A carry job's units on its other side (`CarryJob::other`), held or given
/// back: `job.remaining` of them. The agent's own side changes where the
/// job is set or dropped, since that is the agent's own state. Contested: a
/// reservation the other side can no longer cover when it applies is
/// refused, and the agent's job rolls back (`Settlement::reserve_stock`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReserveStock {
    pub agent: u16,
    pub job: CarryJob,
    pub change: StockChange,
}

impl StateHash for DropHaul {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
        h.write(&self.serial);
    }
}

impl StateHash for DropNotes {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
        h.write(&self.serial);
    }
}

impl StateHash for RestartMemory {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

impl StateHash for ReserveStock {
    fn hash_into(&self, h: &mut StateHasher) {
        let job = &self.job;
        h.write(&self.agent);
        let (kind, index) = match job.other {
            InventoryRef::Depot(d) => (0, d),
            InventoryRef::Agent(a) => (1, a),
        };
        h.write_u64(kind);
        h.write(&index);
        h.write(&job.item);
        h.write_u64(job.direction as u64);
        h.write(&job.remaining);
        h.write(&job.next_tick);
        h.write_u64(self.change as u64);
    }
}

/// What players' actions do to the world (`settlement::action`): one queue
/// per message type, in apply order. Actions apply in stage 1, before the
/// chunks have outboxes, so their messages wait here rather than in
/// `Messages`, and stage 5 applies them first in the settlement's part, in
/// command order. Always empty between ticks, so neither saved nor hashed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionMessages {
    pub restart_memory: Vec<RestartMemory>,
    pub drop_haul: Vec<DropHaul>,
    pub drop_notes: Vec<DropNotes>,
    pub reserve_stock: Vec<ReserveStock>,
}

impl ActionMessages {
    pub fn is_empty(&self) -> bool {
        self.restart_memory.is_empty() && self.drop_haul.is_empty() && self.drop_notes.is_empty() && self.reserve_stock.is_empty()
    }
}

impl StateHash for ActionMessages {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.restart_memory);
        h.write(&self.drop_haul);
        h.write(&self.drop_notes);
        h.write(&self.reserve_stock);
    }
}

impl StateHash for Strike {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
        h.write_u64(self.weapon as u64);
        h.write(&self.origin);
        h.write(&self.forward);
        h.write(&self.muzzle[..]);
        h.write(&self.targets);
        match &self.beam_end {
            Some(end) => h.write(end),
            None => h.write_u64(u64::MAX),
        }
        h.write_u64(self.cover.map_or(u64::MAX, u64::from));
        h.write(&self.hit_player);
        h.write(&self.started);
    }
}

impl StateHash for ProjectileHit {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.projectile);
        h.write(&self.target);
    }
}

impl StateHash for ProjectileBlocked {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.projectile);
        h.write(&self.at);
        if let Some(index) = self.cover { h.write_u64(u64::from(index)); }
    }
}

impl StateHash for ProjectileExpired {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.projectile);
    }
}

impl StateHash for Breach {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.threat);
    }
}

impl StateHash for Fire {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.defender);
        h.write(&self.target);
        h.write(&self.to);
    }
}

impl StateHash for ManualUnit {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

impl StateHash for ClaimDock {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.depot);
        h.write(&self.agent);
        match self.claim {
            DockClaim::Holding => h.write_u64(0),
            DockClaim::Approaching(d) => {
                h.write_u64(1);
                h.write(&d);
            }
        }
    }
}

impl StateHash for Retry {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

impl StateHash for Equip {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
        h.write(&self.route);
    }
}

impl StateHash for Reserve {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
        h.write(&self.link);
        h.write(&self.units);
        h.write(&self.route);
    }
}

impl StateHash for Attach {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

impl StateHash for Load {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

impl StateHash for Unload {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.agent);
    }
}

impl StateHash for Damage {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.source);
        h.write(&self.target);
        h.write(&self.amount);
    }
}

impl StateHash for Eat {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.mover);
        h.write(&self.chunk);
        h.write(&self.amount);
    }
}

impl StateHash for Spawn {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.parent);
        h.write(&self.pos);
        h.write(&self.kind);
    }
}

impl StateHash for Killed {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.mover);
    }
}

impl StateHash for EatFailed {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.mover);
    }
}

macro_rules! message_queues {
    ($($field:ident: $ty:ty,)+) => {
        /// One queue per message type, in apply order.
        #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        pub struct Messages {
            $(pub $field: Vec<$ty>,)+
        }

        impl Messages {
            /// Moves every message in `other` to the end of the matching queue.
            pub fn append(&mut self, other: &mut Messages) {
                $(self.$field.append(&mut other.$field);)+
            }

            pub fn is_empty(&self) -> bool {
                true $(&& self.$field.is_empty())+
            }

            pub fn len(&self) -> usize {
                0 $(+ self.$field.len())+
            }
        }

        impl StateHash for Messages {
            fn hash_into(&self, h: &mut StateHasher) {
                $(h.write(&self.$field);)+
            }
        }
    };
}

message_queues! {
    damage: Damage,
    eat: Eat,
    spawn: Spawn,
    killed: Killed,
    eat_failed: EatFailed,
    // Settlement messages, after the economy step. Docks first, so the other
    // handlers see this tick's owners only from next tick on, like everyone.
    claim_dock: ClaimDock,
    retry: Retry,
    equip: Equip,
    reserve: Reserve,
    attach: Attach,
    load: Load,
    unload: Unload,
    manual_unit: ManualUnit,
    pile_step: PileStep,
    // Combat: strikes and projectile hits before breaches, so a killing
    // blow on the last tick saves the base; shots last. Threats' swings
    // after them too, so a kill or stagger on the tick a swing starts or
    // lands stops it.
    strike: Strike,
    projectile_hit: ProjectileHit,
    projectile_blocked: ProjectileBlocked,
    projectile_expired: ProjectileExpired,
    start_swing: StartSwing,
    swing_lands: SwingLands,
    breach: Breach,
    fire: Fire,
}

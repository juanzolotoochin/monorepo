//! Messages: every interaction between entities, or with a shared resource,
//! is a message. There is one queue per message type, and types are applied in
//! the order they are declared in `message_queues!`.

use fx::{Fx, FxVec2, FxVec3};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};
use crate::id::Id;
use crate::mover::Mover;

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
}

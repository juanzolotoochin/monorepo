//! Messages: every interaction between entities, or with a shared resource,
//! is a message. There is one queue per message type, and types are applied in
//! the order they are declared in `message_queues!`.

use fx::{Fx, FxVec3};

use crate::hash::{StateHash, StateHasher};
use crate::id::Id;
use crate::mover::Mover;

/// One mover attacks another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Damage {
    pub source: Id<Mover>,
    pub target: Id<Mover>,
    pub amount: i32,
}

/// A mover takes food from a chunk. Contested: when the food runs out, later
/// messages in the order are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eat {
    pub mover: Id<Mover>,
    pub chunk: u32,
    pub amount: i32,
}

/// A mover asks for a child. Rejected when the world is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub parent: Id<Mover>,
    pub pos: FxVec3,
    pub radius: Fx,
}

/// Sent during apply to a mover whose attack killed its target. Handled next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Killed {
    pub mover: Id<Mover>,
}

/// Sent during apply to a mover whose `Eat` was rejected. Handled next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EatFailed {
    pub mover: Id<Mover>,
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
        h.write(&self.radius);
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
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
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
}

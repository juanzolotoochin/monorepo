//! A timer wheel for waking sleeping entities.

use crate::hash::{StateHash, StateHasher};

const SLOTS: usize = 256;

/// Items scheduled by tick. Each slot holds every timer whose tick is congruent
/// to the slot index, so timers more than `SLOTS` ticks away stay in their slot
/// until their tick comes up. Order within a slot is insertion order.
pub struct TimerWheel<T> {
    slots: Vec<Vec<(u64, T)>>,
}

impl<T> Default for TimerWheel<T> {
    fn default() -> Self {
        TimerWheel { slots: (0..SLOTS).map(|_| Vec::new()).collect() }
    }
}

impl<T: Copy> TimerWheel<T> {
    pub fn schedule(&mut self, tick: u64, item: T) {
        self.slots[tick as usize % SLOTS].push((tick, item));
    }

    /// Removes the items due at `tick` and appends them to `due` in insertion order.
    pub fn drain_due(&mut self, tick: u64, due: &mut Vec<T>) {
        self.slots[tick as usize % SLOTS].retain(|&(at, item)| {
            if at == tick {
                due.push(item);
                false
            } else {
                true
            }
        });
    }

    /// Every pending timer as `(tick, item)`, slot by slot in insertion order.
    pub fn to_state(&self) -> Vec<Vec<(u64, T)>> {
        self.slots.clone()
    }

    pub fn from_state(slots: Vec<Vec<(u64, T)>>) -> Option<TimerWheel<T>> {
        let valid = slots.len() == SLOTS
            && slots.iter().enumerate().all(|(i, slot)| slot.iter().all(|&(tick, _)| tick as usize % SLOTS == i));
        valid.then_some(TimerWheel { slots })
    }

    pub fn len(&self) -> usize {
        self.slots.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Vec::is_empty)
    }
}

impl<T: StateHash> StateHash for TimerWheel<T> {
    fn hash_into(&self, h: &mut StateHasher) {
        for slot in &self.slots {
            h.write_u64(slot.len() as u64);
            for (tick, item) in slot {
                h.write_u64(*tick);
                item.hash_into(h);
            }
        }
    }
}

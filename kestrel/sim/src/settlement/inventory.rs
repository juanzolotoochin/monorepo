//! Countable goods held in one place: a depot, an agent's cargo, and later a
//! pile or the collector (`INVENTORY_HANDOFF.md`).
//!
//! An inventory counts units per item. It has one capacity, a total in units
//! of any item. Reservations are per item:
//!
//! - `reserved_out`: units promised to someone. They are still in `counts`.
//! - `reserved_in`: units on their way here. They are not in `counts` yet,
//!   but they hold space.
//!
//! A unit in the air belongs to neither end. Taking a unit ends its
//! reservation out; putting it ends its reservation in.

use serde::{Deserialize, Serialize};

use super::ItemIx;

/// Which items an inventory may hold at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kinds {
    /// Any mix of items.
    #[default]
    AnyMix,
    /// One item at a time. A second item is refused while any of the first
    /// is held or reserved. Piles use it.
    OneKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// Units on hand, per item.
    pub counts: Vec<u32>,
    /// Total units of any item. 0 holds nothing; `u32::MAX` has no limit.
    pub capacity: u32,
    /// Units promised to someone, per item. They are part of `counts`.
    pub reserved_out: Vec<u32>,
    /// Units on their way here, per item. They hold space.
    pub reserved_in: Vec<u32>,
    pub kinds: Kinds,
}

impl Inventory {
    /// An empty inventory for `items` items.
    pub fn new(items: usize, capacity: u32, kinds: Kinds) -> Inventory {
        Inventory { counts: vec![0; items], capacity, reserved_out: vec![0; items], reserved_in: vec![0; items], kinds }
    }

    /// Units of `item` on hand.
    pub fn count(&self, item: ItemIx) -> u32 {
        self.counts[item as usize]
    }

    /// Units on hand, of every item.
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// Units on their way here, of every item.
    pub fn total_in(&self) -> u32 {
        self.reserved_in.iter().sum()
    }

    /// Whether it holds any unit.
    pub fn holds_any(&self) -> bool {
        self.counts.iter().any(|&u| u > 0)
    }

    /// Units of `item` not promised to anyone.
    pub fn available(&self, item: ItemIx) -> u32 {
        self.counts[item as usize].saturating_sub(self.reserved_out[item as usize])
    }

    /// Space not taken by units on hand or on their way.
    pub fn free(&self) -> u32 {
        self.capacity.saturating_sub(self.total() + self.total_in())
    }

    /// The item held or reserved, if any. With `OneKind` there is at most one.
    pub fn kind(&self) -> Option<ItemIx> {
        (0..self.counts.len()).find(|&i| self.counts[i] + self.reserved_in[i] + self.reserved_out[i] > 0).map(|i| i as ItemIx)
    }

    /// Whether `item` may come in, by `kinds`.
    pub fn accepts(&self, item: ItemIx) -> bool {
        match self.kinds {
            Kinds::AnyMix => true,
            Kinds::OneKind => self.kind().is_none_or(|k| k == item),
        }
    }

    /// Promises `units` of `item` to someone. Refuses (and changes nothing)
    /// when fewer are available.
    pub fn reserve_out(&mut self, item: ItemIx, units: u32) -> bool {
        if units > self.available(item) {
            return false;
        }
        self.reserved_out[item as usize] += units;
        true
    }

    /// Holds space for `units` of `item` on their way here. Refuses (and
    /// changes nothing) when they don't fit, or are a second kind.
    pub fn reserve_in(&mut self, item: ItemIx, units: u32) -> bool {
        if units > self.free() || !self.accepts(item) {
            return false;
        }
        self.reserved_in[item as usize] += units;
        true
    }

    /// Ends a promise of `units` of `item` without moving them.
    pub fn release_out(&mut self, item: ItemIx, units: u32) {
        self.reserved_out[item as usize] -= units;
    }

    /// Gives back the space held for `units` of `item`.
    pub fn release_in(&mut self, item: ItemIx, units: u32) {
        self.reserved_in[item as usize] -= units;
    }

    /// Moves out `units` of `item` that were reserved out.
    pub fn take(&mut self, item: ItemIx, units: u32) {
        self.counts[item as usize] -= units;
        self.reserved_out[item as usize] -= units;
    }

    /// Moves in `units` of `item` that were reserved in.
    pub fn put(&mut self, item: ItemIx, units: u32) {
        self.counts[item as usize] += units;
        self.reserved_in[item as usize] -= units;
    }

    /// Adds `units` of `item` without a reservation: new units, such as a
    /// source's. Refuses (and changes nothing) when they don't fit, or are a
    /// second kind.
    pub fn add(&mut self, item: ItemIx, units: u32) -> bool {
        if units > self.free() || !self.accepts(item) {
            return false;
        }
        self.counts[item as usize] += units;
        true
    }

    /// Removes `units` of `item` without a reservation: units used up, or
    /// sent off at once. The caller checks they are available.
    pub fn remove(&mut self, item: ItemIx, units: u32) {
        debug_assert!(units <= self.available(item), "removing promised units");
        self.counts[item as usize] -= units;
    }

    /// Checks this inventory on its own: per-item lists of one length, units
    /// within capacity, promises within counts, and one kind for `OneKind`.
    pub fn validate(&self) -> Result<(), String> {
        let items = self.counts.len();
        if self.reserved_out.len() != items || self.reserved_in.len() != items {
            return Err("item lists differ in length".into());
        }
        if self.total() as u64 + self.total_in() as u64 > self.capacity as u64 {
            return Err("overbooked".into());
        }
        if (0..items).any(|i| self.reserved_out[i] > self.counts[i]) {
            return Err("stock overbooked".into());
        }
        if self.kinds == Kinds::OneKind {
            let used = (0..items).filter(|&i| self.counts[i] + self.reserved_in[i] + self.reserved_out[i] > 0).count();
            if used > 1 {
                return Err("holds more than one kind".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stocked(counts: &[u32], capacity: u32, kinds: Kinds) -> Inventory {
        let mut inv = Inventory::new(counts.len(), capacity, kinds);
        for (i, &units) in counts.iter().enumerate() {
            assert!(inv.add(i as ItemIx, units));
        }
        inv
    }

    #[test]
    fn new_is_empty() {
        let inv = Inventory::new(3, 10, Kinds::AnyMix);
        assert_eq!(inv.counts, vec![0; 3]);
        assert_eq!((inv.total(), inv.total_in(), inv.free()), (0, 0, 10));
        assert!(!inv.holds_any());
        assert_eq!(inv.kind(), None);
        assert_eq!(inv.validate(), Ok(()));
    }

    #[test]
    fn available_leaves_out_promised_units() {
        let mut inv = stocked(&[5, 2], 10, Kinds::AnyMix);
        assert!(inv.reserve_out(0, 3));
        assert_eq!((inv.available(0), inv.available(1), inv.count(0)), (2, 2, 5));
        assert!(!inv.reserve_out(0, 3), "only two are left");
        assert_eq!(inv.reserved_out[0], 3, "a refusal changes nothing");
        assert!(inv.reserve_out(0, 2));
        assert_eq!(inv.available(0), 0);
        inv.release_out(0, 4);
        assert_eq!(inv.available(0), 4);
        assert_eq!(inv.validate(), Ok(()));
    }

    #[test]
    fn free_counts_units_on_hand_and_on_their_way() {
        let mut inv = stocked(&[4, 0], 10, Kinds::AnyMix);
        assert_eq!(inv.free(), 6);
        assert!(inv.reserve_in(1, 5));
        assert_eq!(inv.free(), 1);
        assert!(!inv.reserve_in(0, 2), "two don't fit");
        assert_eq!(inv.reserved_in, vec![0, 5], "a refusal changes nothing");
        assert!(!inv.add(0, 2));
        assert!(inv.reserve_in(0, 1));
        assert_eq!(inv.free(), 0);
        inv.release_in(1, 5);
        assert_eq!(inv.free(), 5);
        assert_eq!(inv.validate(), Ok(()));
    }

    #[test]
    fn free_saturates_when_over_capacity() {
        // A capacity lowered below what is held leaves no space, not a wrap.
        let mut inv = stocked(&[6], 8, Kinds::AnyMix);
        inv.capacity = 4;
        assert_eq!(inv.free(), 0);
        assert_eq!(inv.validate(), Err("overbooked".into()));
    }

    #[test]
    fn take_and_put_move_reserved_units() {
        let mut from = stocked(&[3, 0], 10, Kinds::AnyMix);
        let mut to = Inventory::new(2, 4, Kinds::AnyMix);
        assert!(from.reserve_out(0, 2));
        assert!(to.reserve_in(0, 2));
        from.take(0, 1);
        // In the air: neither end holds it, but the target keeps its space.
        assert_eq!((from.count(0), from.reserved_out[0], from.available(0)), (2, 1, 1));
        assert_eq!((to.count(0), to.free()), (0, 2));
        to.put(0, 1);
        assert_eq!((to.count(0), to.reserved_in[0], to.free()), (1, 1, 2));
        from.take(0, 1);
        to.put(0, 1);
        assert_eq!((from.counts.clone(), to.counts.clone()), (vec![1, 0], vec![2, 0]));
        assert_eq!((from.reserved_out[0], to.reserved_in[0]), (0, 0));
        assert_eq!((from.validate(), to.validate()), (Ok(()), Ok(())));
    }

    #[test]
    fn add_and_remove_skip_reservations() {
        let mut inv = Inventory::new(2, 5, Kinds::AnyMix);
        assert!(inv.add(0, 3));
        assert!(inv.add(1, 2));
        assert!(!inv.add(1, 1), "full");
        inv.remove(0, 2);
        assert_eq!(inv.counts, vec![1, 2]);
        assert_eq!(inv.total(), 3);
        assert!(inv.holds_any());
    }

    #[test]
    fn zero_capacity_holds_nothing() {
        let mut inv = Inventory::new(1, 0, Kinds::AnyMix);
        assert_eq!(inv.free(), 0);
        assert!(!inv.add(0, 1));
        assert!(!inv.reserve_in(0, 1));
        assert!(inv.reserve_in(0, 0), "nothing always fits");
    }

    #[test]
    fn any_mix_holds_several_items() {
        let mut inv = stocked(&[1, 1, 0], 10, Kinds::AnyMix);
        assert!(inv.accepts(2));
        assert!(inv.reserve_in(2, 1));
        assert_eq!(inv.kind(), Some(0), "the first item held");
        assert_eq!(inv.validate(), Ok(()));
    }

    #[test]
    fn one_kind_refuses_a_second_item() {
        let mut inv = Inventory::new(3, 10, Kinds::OneKind);
        assert!(inv.accepts(1));
        assert!(inv.add(1, 2));
        assert_eq!(inv.kind(), Some(1));
        assert!(!inv.accepts(0));
        assert!(!inv.add(0, 1));
        assert!(!inv.reserve_in(2, 1));
        assert!(inv.reserve_in(1, 3), "more of the same is fine");
        assert_eq!(inv.validate(), Ok(()));
    }

    #[test]
    fn one_kind_counts_reservations_as_held() {
        // Space reserved for one item keeps out another, even with nothing held.
        let mut inv = Inventory::new(2, 10, Kinds::OneKind);
        assert!(inv.reserve_in(0, 1));
        assert!(!inv.add(1, 1));
        inv.put(0, 1);
        assert!(inv.reserve_out(0, 1));
        inv.take(0, 1);
        // Empty again: any item may come.
        assert_eq!(inv.kind(), None);
        assert!(inv.add(1, 1));
        assert_eq!(inv.validate(), Ok(()));
    }

    #[test]
    fn validate_finds_broken_inventories() {
        let mut inv = stocked(&[2, 0], 4, Kinds::AnyMix);
        inv.reserved_out[0] = 3;
        assert_eq!(inv.validate(), Err("stock overbooked".into()));
        inv.reserved_out[0] = 0;
        inv.reserved_in[1] = 3;
        assert_eq!(inv.validate(), Err("overbooked".into()));
        inv.reserved_in[1] = 1;
        assert_eq!(inv.validate(), Ok(()));
        inv.kinds = Kinds::OneKind;
        assert_eq!(inv.validate(), Err("holds more than one kind".into()));
        inv.reserved_in.pop();
        assert_eq!(inv.validate(), Err("item lists differ in length".into()));
    }
}

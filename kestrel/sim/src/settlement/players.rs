//! Players joining and leaving: each joiner gets an avatar at their slot's
//! spawn spot, and a player who leaves takes their avatar with them.
//!
//! Agents are referred to by index (`AgentIx`) in settlement state, so
//! removing one shifts the indices after it. `Settlement::leave` drops what
//! points at the leaving avatar and moves every other reference down
//! (`remap_agents`). Joins append, and nothing else adds agents, so the
//! scenario's agents (haulers and the first avatar) come first and never
//! move; only joined avatars after the removed one do.

use fx::{Fx, FxVec2};

use super::avatar::{AvatarState, Intent};
use super::{AgentIx, AgentState, Body, Carry, Inventory, InventoryRef, Kinds, Role, Settlement};
use crate::terrain::Terrain;
use crate::PlayerId;

/// Distance between neighboring spawn spots.
const SPOT_SPACING: Fx = Fx::from_ratio(6, 5);
/// Rings of spots around the spawn: 8 spots in the first, 16 in the second,
/// 24 in the third. Spots blocked by cover are skipped, so there are more
/// than the 16 players a LAN game may allow.
const SPOT_RINGS: i32 = 3;

/// Spawn spot offsets from the scene's spawn, in grid steps: nearest first,
/// then by position, so the order is the same on every peer.
fn spot_offsets() -> Vec<(i32, i32)> {
    let mut spots: Vec<(i32, i32)> = (-SPOT_RINGS..=SPOT_RINGS)
        .flat_map(|x| (-SPOT_RINGS..=SPOT_RINGS).map(move |y| (x, y)))
        .filter(|&offset| offset != (0, 0))
        .collect();
    spots.sort_by_key(|&(x, y)| (x * x + y * y, -x, y));
    spots
}

impl Settlement {
    /// Commands: gives a player who joined an avatar of their own, a copy
    /// of the scene's first avatar at a spawn spot beside it, by slot.
    /// Players who already have one keep it.
    pub fn join(&mut self, player: PlayerId, terrain: &Terrain) {
        if self.avatar_of(player).is_some() {
            return;
        }
        let Some(template) = self.player().map(|i| self.agents[i as usize].clone()) else { return };
        let position = self.spawn_spot(player, terrain, template.body.home, template.radius());
        let height = terrain.footprint(position, template.radius()).unwrap_or(self.spawn_height);
        // Only the definition carries over; the state starts fresh.
        self.agents.push(AgentState {
            id: format!("player-{}", player.0),
            character: template.character,
            body: Body {
                home: position,
                position,
                heading: Fx::PI,
                health: self.player_health,
                max_health: self.player_health,
                speed: template.body.speed,
                base_radius: template.body.base_radius,
                ..Body::default()
            },
            carry: Carry {
                inventory: Inventory::new(template.carry.inventory.counts.len(), template.carry.inventory.capacity, Kinds::AnyMix),
                transfer_ticks: template.carry.transfer_ticks,
                job: None,
                next: None,
            },
            role: Role::Avatar(AvatarState {
                owner: Some(player),
                height,
                weapon: self.weapons.first().copied().unwrap_or_default(),
                intent: Some(Intent::Stop),
                ..AvatarState::default()
            }),
        });
    }

    /// Agent `agent` of this settlement in `other` (another tick of the same
    /// world, such as the one before): avatars by owner, since a player
    /// leaving shifts the avatars after theirs, and other agents by index,
    /// since they never move.
    pub fn same_agent_in<'a>(&self, agent: AgentIx, other: &'a Settlement) -> Option<&'a AgentState> {
        match self.agents.get(agent as usize)?.avatar().and_then(|avatar| avatar.owner) {
            Some(owner) => other.agents.get(other.avatar_of(owner)? as usize),
            None => other.agents.get(agent as usize),
        }
    }

    /// Where `player`'s avatar starts and respawns: a spot around `spawn`
    /// (the first avatar's home, which a scene puts at its spawn point),
    /// picked by slot, that is free of obstacles, reachable on foot from
    /// `spawn`, and nobody else's home. `spawn` itself if no spot is.
    fn spawn_spot(&self, player: PlayerId, terrain: &Terrain, spawn: FxVec2, radius: Fx) -> FxVec2 {
        let spots = spot_offsets();
        let first = (player.0 as usize).saturating_sub(1) % spots.len();
        let taken = |p: FxVec2| self.agents.iter().any(|a| a.avatar().is_some() && a.body.home == p);
        (0..spots.len())
            .map(|i| spots[(first + i) % spots.len()])
            .map(|(x, y)| FxVec2::new(spawn.x + SPOT_SPACING * Fx::from_int(x), spawn.y + SPOT_SPACING * Fx::from_int(y)))
            .find(|&p| !taken(p) && terrain.clear(p, p, radius) && terrain.walkable(spawn, p, radius))
            .unwrap_or(spawn)
    }

    /// Commands: `player` left the game. Their avatar goes, with what it
    /// held:
    /// - its carry jobs end through `drop_carry`, as a cancel ends them, so
    ///   what they held on a depot, a pile or the collector and in `next` is
    ///   released (an empty drop pile then goes in housekeeping);
    /// - units flying to or from it land at once;
    /// - what its cargo then holds, minerals among the items, is written off
    ///   as `abandoned`;
    /// - its pile lab hauler (walk, notices, toss landings) goes.
    ///
    /// The scene's first avatar never leaves, since every joiner is a copy
    /// of it.
    pub fn leave(&mut self, player: PlayerId) {
        let Some(gone) = self.avatar_of(player) else { return };
        if Some(gone) == self.player() {
            return;
        }
        let releases = self.agents[gone as usize].carry.drop_carry(gone);
        self.reserve_stock(releases);
        let flying: Vec<u64> = self
            .transfers
            .values()
            .filter(|t| t.from == InventoryRef::Agent(gone) || t.to == InventoryRef::Agent(gone))
            .map(|t| t.id)
            .collect();
        for id in flying {
            let t = self.transfers.remove(&id).expect("listed transfer");
            self.land(t);
        }
        let a = &self.agents[gone as usize];
        let mut abandoned = a.carry.inventory.counts.clone();
        if let Some(item) = a.equipped_item() {
            abandoned[item as usize] += 1;
        }
        for (item, units) in abandoned.into_iter().enumerate() {
            self.accounts[item].abandoned += units as u64;
        }
        self.piano_notes.retain(|note| note.agent != gone);
        for t in &mut self.threats {
            if t.windup.is_some_and(|w| w.agent == gone) {
                t.windup = None;
            }
        }
        for owner in &mut self.dock_owner {
            if *owner == Some(gone) {
                *owner = None;
            }
        }
        if let Some(lab) = &mut self.piles {
            lab.haulers.remove(&gone);
        }
        self.agents.remove(gone as usize);
        self.remap_agents(gone);
    }

    /// After agent `gone` was removed: every stored index above it moves
    /// down by one. References to `gone` itself must be dropped first.
    fn remap_agents(&mut self, gone: AgentIx) {
        let shift = |ix: &mut AgentIx| {
            debug_assert_ne!(*ix, gone, "a reference to a removed agent");
            if *ix > gone {
                *ix -= 1;
            }
        };
        for owner in self.dock_owner.iter_mut().flatten() {
            shift(owner);
        }
        for t in self.transfers.values_mut() {
            for end in [&mut t.from, &mut t.to] {
                if let InventoryRef::Agent(ix) = end {
                    shift(ix);
                }
            }
        }
        for job in self.agents.iter_mut().flat_map(|a| [a.carry.job.as_mut(), a.carry.next.as_mut()]).flatten() {
            if let InventoryRef::Agent(ix) = &mut job.other {
                shift(ix);
            }
        }
        for windup in self.threats.iter_mut().filter_map(|t| t.windup.as_mut()) {
            shift(&mut windup.agent);
        }
        for note in &mut self.piano_notes {
            shift(&mut note.agent);
        }
        if let Some(lab) = &mut self.piles {
            lab.haulers = std::mem::take(&mut lab.haulers)
                .into_iter()
                .map(|(mut ix, hauler)| {
                    shift(&mut ix);
                    (ix, hauler)
                })
                .collect();
        }
    }
}

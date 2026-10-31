//! Buildings: gatherers players put up from parts, which turn out one item
//! while someone occupies them (`SACRED_TREE_HANDOFF.md`, D1).
//!
//! A building keeps its own table (`Settlement::buildings`), as vehicles and
//! threats do. The table only grows: an index is stable for the life of the
//! world. A building's kind is a row of `Settlement::building_kinds`, the
//! scenario's kinds resolved into ticks and item indices when it loads.
//!
//! Production runs in the economy step (`Settlement::building_production`):
//! a built building with an occupant works toward its next unit, and puts it
//! in its output when the work reaches its kind's period. An empty building,
//! or one a sprite has only claimed, does nothing (A1). An output is a pile
//! of the kind's product beside the building; production pauses while it
//! holds the occupant's cap.

use content::scenario::{self, Scenario};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::sprites::Hold;
use super::{AgentIx, DepotIx, Event, Inventory, InventoryRef, ItemIx, Kinds, Settlement};
use crate::terrain::{Disc, Terrain, CLEARANCE_MARGIN};

pub type BuildingIx = u16;
pub type BuildingKindIx = u16;

/// The clearance kept around a depot's dock and an output pile, as the
/// depot dock check in `Settlement::from_scenario` uses.
const DOCK_RADIUS: Fx = Fx::from_ratio(3, 20);
/// The widest body that stands at a slot spot, which must be clear for it.
const SLOT_RADIUS: Fx = Fx::from_ratio(3, 10);
/// How far past its footprint a building's slot spot is.
const SLOT_GAP: Fx = Fx::from_ratio(2, 5);

/// A building kind's rules, from the scenario: rules, not state, but copied
/// into the settlement as weapon tuning is, since the scenario isn't kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingKind {
    pub id: String,
    pub product: ItemIx,
    /// Footprint radius in meters.
    pub radius: Fx,
    /// Ticks of a sprite's work per unit.
    pub period: u64,
    /// Units the output holds before production pauses.
    pub cap: u32,
    /// The output's cap while an elder possesses the building.
    pub possessed_cap: u32,
    /// How many ticks of a sprite's work one tick of an elder's is worth.
    pub possessed_multiplier: Fx,
    /// Units a Burst adds at once.
    pub burst: u32,
    /// Ticks between Bursts.
    pub burst_cooldown: u64,
    /// Meters from the center to the output pile.
    pub output_distance: Fx,
    /// The materials of each part, in install order.
    pub parts: Vec<Vec<(ItemIx, u32)>>,
}

impl BuildingKind {
    /// The output's cap with `occupant` in the slot.
    pub fn cap_for(&self, occupant: Option<Occupant>) -> u32 {
        match occupant {
            Some(Occupant::Elder(_)) => self.possessed_cap,
            _ => self.cap,
        }
    }
}

/// How far a building has got.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// Being put up: `part` is the next part to install, and `materials` what
    /// has been delivered for it.
    Site { part: u8, materials: Inventory },
    Built,
}

/// Who works a building. A building has one slot (A1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Occupant {
    Sprite(AgentIx),
    /// A player's elder, possessing the building.
    Elder(AgentIx),
}

impl Occupant {
    pub fn agent(self) -> AgentIx {
        match self {
            Occupant::Sprite(a) | Occupant::Elder(a) => a,
        }
    }
}

/// A building's slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Slot {
    #[default]
    Empty,
    /// Held for a sprite on its way in (D2). It produces nothing yet.
    Claimed(AgentIx),
    Occupied(Occupant),
}

impl Slot {
    /// Who is inside, if anyone.
    pub fn occupant(self) -> Option<Occupant> {
        match self {
            Slot::Occupied(o) => Some(o),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingState {
    pub kind: BuildingKindIx,
    pub position: FxVec2,
    /// Where the output pile sits: `output_distance` from the center, on
    /// the side facing the Sacred Tree (`output_position`).
    pub output_position: FxVec2,
    pub stage: Stage,
    pub slot: Slot,
    /// Work toward the next unit, in ticks of a sprite's work. An elder adds
    /// the kind's multiplier each tick, so swapping occupants mid-unit keeps
    /// the work done.
    pub progress: Fx,
    /// The produced units waiting to be picked up: the kind's product only,
    /// up to the possessed cap (the most any occupant fills it to).
    pub output: Inventory,
}

impl BuildingState {
    pub fn built(&self) -> bool {
        self.stage == Stage::Built
    }

    /// Where the occupant stands (D2): just past the footprint, a quarter
    /// turn from the output pile, so the two never share a spot.
    pub fn slot_position(&self, kind: &BuildingKind) -> FxVec2 {
        let out = (self.output_position - self.position).normalize_or_zero();
        let out = if out == FxVec2::ZERO { FxVec2::new(Fx::ZERO, Fx::ONE) } else { out };
        self.position + FxVec2::new(-out.y, out.x) * (kind.radius + SLOT_GAP)
    }
}

/// Resolves the scenario's building kinds. `item` maps an item's id to its
/// index; the scenario is validated.
pub(super) fn kinds(scenario: &Scenario, item: impl Fn(&str) -> ItemIx) -> Vec<BuildingKind> {
    scenario
        .building_kinds
        .iter()
        .map(|k| BuildingKind {
            id: k.id.clone(),
            product: item(&k.product),
            radius: k.radius,
            period: scenario::ticks(k.period),
            cap: k.cap,
            possessed_cap: k.possessed_cap,
            possessed_multiplier: k.possessed_multiplier,
            burst: k.burst.units,
            burst_cooldown: scenario::ticks(k.burst.cooldown),
            output_distance: k.output_distance,
            parts: k.parts.iter().map(|p| p.materials.iter().map(|s| (item(&s.item), s.units)).collect()).collect(),
        })
        .collect()
}

/// Where a building's output pile goes: `distance` from its center, toward
/// the Sacred Tree. The tree is the midpoint of the scenario's depots (its
/// stock piles), or the arena's center when there are none. A building
/// standing right on that point puts it on its +y side.
pub(super) fn output_position(center: FxVec2, distance: Fx, depots: &[FxVec2], arena: FxVec2) -> FxVec2 {
    let tree = if depots.is_empty() {
        arena
    } else {
        let sum = depots.iter().fold(FxVec2::ZERO, |sum, &d| sum + d);
        sum.div_int(depots.len() as i64)
    };
    let toward = (tree - center).normalize_or_zero();
    let toward = if toward == FxVec2::ZERO { FxVec2::new(Fx::ZERO, Fx::ONE) } else { toward };
    center + toward * distance
}

/// Puts up the scenario's prebuilt buildings, built and empty, in scenario
/// order. Each footprint must be clear of the terrain (the scene's props,
/// machines, defenders and the buildings before it), of every depot's dock,
/// and inside the arena; it becomes an obstacle. Every output pile and slot
/// spot must then be clear and inside the arena too. `at` places a scenario
/// point.
pub(super) fn prebuilt(
    scenario: &Scenario,
    kinds: &[BuildingKind],
    depots: &[(DepotIx, FxVec2)],
    items: usize,
    terrain: &mut Terrain,
    at: impl Fn(scenario::Point) -> FxVec2,
) -> Result<Vec<BuildingState>, String> {
    let docks: Vec<FxVec2> = depots.iter().map(|&(_, p)| p).collect();
    let mut buildings = Vec::with_capacity(scenario.buildings.len());
    for (i, b) in scenario.buildings.iter().enumerate() {
        let k = kinds.iter().position(|k| k.id == b.kind).expect("validated building kind");
        let kind = &kinds[k];
        let center = at(b.position);
        let name = format!("building {i} ({})", b.kind);
        let reach = FxVec2::new(kind.radius, kind.radius);
        if !terrain.inside(center - reach) || !terrain.inside(center + reach) {
            return Err(format!("{name} at {:?} reaches outside the arena", b.position));
        }
        if !terrain.clear(center, center, kind.radius) {
            return Err(format!("{name} at {:?} overlaps an obstacle or another building", b.position));
        }
        let near = kind.radius + DOCK_RADIUS + CLEARANCE_MARGIN;
        if let Some(&(d, _)) = depots.iter().find(|&&(_, p)| p.distance(center) <= near) {
            return Err(format!("{name} at {:?} covers depot {}'s dock", b.position, scenario.depots[d as usize].id));
        }
        terrain.obstacles.push(Disc { center, radius: kind.radius });
        buildings.push(BuildingState {
            kind: k as BuildingKindIx,
            position: center,
            output_position: output_position(center, kind.output_distance, &docks, terrain.center),
            stage: Stage::Built,
            slot: Slot::Empty,
            progress: Fx::ZERO,
            output: Inventory::new(items, kind.possessed_cap, Kinds::OneKind),
        });
    }
    if !buildings.is_empty() {
        terrain.revision += 1;
    }
    for (i, b) in buildings.iter().enumerate() {
        if !terrain.clear(b.output_position, b.output_position, DOCK_RADIUS) {
            return Err(format!("building {i} ({})'s output pile is blocked", kinds[b.kind as usize].id));
        }
        let slot = b.slot_position(&kinds[b.kind as usize]);
        if !terrain.clear(slot, slot, SLOT_RADIUS) {
            return Err(format!("building {i} ({})'s slot spot is blocked", kinds[b.kind as usize].id));
        }
    }
    Ok(buildings)
}

impl Settlement {
    /// The kind of building `b`.
    pub fn building_kind(&self, b: BuildingIx) -> &BuildingKind {
        &self.building_kinds[self.buildings[b as usize].kind as usize]
    }

    /// Part of the economy step: every built, occupied building below its
    /// cap works one tick, and at its period puts a unit in its output. The
    /// cap counts every unit still in the output, those a bundle has
    /// reserved but not taken yet among them, and any on their way in; a
    /// unit a bundle took is in the air to the bundle and no longer counts.
    pub(super) fn building_production(&mut self) {
        for i in 0..self.buildings.len() {
            let b = &self.buildings[i];
            let kind = &self.building_kinds[b.kind as usize];
            let Some(occupant) = b.slot.occupant() else { continue };
            let held = b.output.total() + b.output.total_in();
            if !b.built() || held >= kind.cap_for(Some(occupant)) {
                continue;
            }
            let work = match occupant {
                Occupant::Sprite(_) => Fx::ONE,
                Occupant::Elder(_) => kind.possessed_multiplier,
            };
            let (product, period) = (kind.product, Fx::ONE.mul_int(kind.period as i64));
            let b = &mut self.buildings[i];
            b.progress = b.progress + work;
            if b.progress < period {
                continue;
            }
            // Any work past the period counts toward the next unit, so an
            // elder's rate holds when its multiplier doesn't divide it.
            b.progress = b.progress - period;
            let added = b.output.add(product, 1);
            debug_assert!(added, "below the cap, which is within the capacity");
            self.accounts[product as usize].produced += 1;
            self.events.push(Event::BuildingProduced { building: i as BuildingIx });
        }
    }

    /// Checks every building on its own: a known kind, a site's part in
    /// range, the output holding the kind's product only, its reservations
    /// against the bundles picking up from it (`expected_reservations`, as
    /// for piles), and nothing reserved on a site's materials (nothing
    /// delivers there yet). Then the slots against
    /// the agents, both ways: a claim or a sprite inside names a sprite that
    /// holds just that (`SpriteState::holds`), an elder names an avatar, and
    /// every sprite's hold is that building's slot.
    pub(super) fn validate_buildings(&self) -> Result<(), String> {
        for (i, b) in self.buildings.iter().enumerate() {
            let Some(kind) = self.building_kinds.get(b.kind as usize) else {
                return Err(format!("building {i} has unknown kind {}", b.kind));
            };
            let name = format!("building {i} ({})", kind.id);
            b.output.validate().map_err(|e| format!("{name} output {e}"))?;
            if b.output.kind().is_some_and(|item| item != kind.product) {
                return Err(format!("{name} output holds an item it doesn't make"));
            }
            if b.output.capacity != kind.possessed_cap {
                return Err(format!("{name} output has capacity {}, not its possessed cap", b.output.capacity));
            }
            let reserved = |inventory: &Inventory| inventory.total_in() > 0 || inventory.reserved_out.iter().any(|&u| u > 0);
            let building = i as BuildingIx;
            if (b.output.reserved_out.clone(), b.output.reserved_in.clone()) != self.expected_reservations(InventoryRef::Output(building)) {
                return Err(format!("{name} output reservations drifted"));
            }
            if let Stage::Site { part, materials } = &b.stage {
                if *part as usize >= kind.parts.len() {
                    return Err(format!("{name} is a site past its last part"));
                }
                materials.validate().map_err(|e| format!("{name} materials {e}"))?;
                if reserved(materials) {
                    return Err(format!("{name} materials reservations drifted"));
                }
            }
            let holds = |a: AgentIx| self.agents.get(a as usize).and_then(|a| a.sprite()).map(|sprite| sprite.holds);
            let agrees = match b.slot {
                Slot::Empty => true,
                Slot::Claimed(a) => holds(a) == Some(Some(Hold::Claimed(building))),
                Slot::Occupied(Occupant::Sprite(a)) => holds(a) == Some(Some(Hold::Inside(building))),
                Slot::Occupied(Occupant::Elder(a)) => self.agents.get(a as usize).is_some_and(|a| a.avatar().is_some()),
            };
            if !agrees {
                return Err(format!("{name} slot {:?} doesn't agree with its agent", b.slot));
            }
        }
        for (i, a) in self.agents.iter().enumerate() {
            let Some(hold) = a.sprite().and_then(|sprite| sprite.holds) else { continue };
            let me = i as AgentIx;
            let (building, slot) = match hold {
                Hold::Claimed(b) => (b, Slot::Claimed(me)),
                Hold::Inside(b) => (b, Slot::Occupied(Occupant::Sprite(me))),
            };
            if self.buildings.get(building as usize).map(|b| b.slot) != Some(slot) {
                return Err(format!("sprite {} holds {hold:?}, which its building's slot doesn't show", a.id));
            }
        }
        Ok(())
    }
}

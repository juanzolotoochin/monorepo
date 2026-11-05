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
//!
//! Players put new buildings up (D1, D4). `PlaceBlueprint` lays a site where
//! `placement` allows one, the check the scenario's prebuilt buildings pass
//! too. Deliveries fill the next part's materials (`InventoryRef::Site`, a
//! drop-off nothing is taken back from), and an install uses them up and
//! moves on to the next part. Installing the last makes the building
//! `Built`; it stays inert until someone occupies it.
//!
//! An elder possessing a building can Burst (D3): its kind's Burst units go
//! into the output at once, up to the possessed cap, and the elder waits out
//! the kind's cooldown before the next (`AvatarState::burst_ready`).
//!
//! A ballista (a kind with a `defence`, `SACRED_TREE_BALLISTA.md` B1, B2)
//! produces nothing. Its output is its ammo store instead: its ammo item
//! only, up to its ammo cap, which players deliver to as to a site and
//! nothing takes back out. Built, occupied and stocked, it shoots the
//! nearest threat in range through the helpers the Woodland Ballista uses
//! (`Settlement::pick_target`, `Settlement::launch_shot`), one unit a shot.
//! Empty, or with nobody inside, it never shoots. An elder possessing it
//! aims instead (B3): its avatar's aim is the reticle, an offset from the
//! ballista clamped to its range (`BuildingKind::reticle`), and `Attack`
//! shoots there (`Settlement::elder_fire`), at most once a period; the
//! ballista picks no targets of its own then.

use content::scenario::{self, Scenario};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::sprites::Hold;
use super::{AgentIx, DepotIx, Event, Inventory, InventoryRef, ItemIx, Kinds, Settlement};
use super::avatar::Seat;
use super::combat::Launcher;
use super::fire::{BurnDef, FireballDef};
use crate::messages::{Burst, BuildingFire, ElderFire, Install, Messages, PlaceBlueprint};
use crate::nav;
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
/// How far in front of an output pile, away from its building, a player
/// stands to pick up there, or to deliver to a site's materials there.
pub const STAND_GAP: Fx = Fx::from_ratio(2, 5);

/// A building kind's rules, from the scenario: rules, not state, but copied
/// into the settlement as weapon tuning is, since the scenario isn't kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingKind {
    pub id: String,
    /// What a gatherer makes; a ballista makes nothing.
    pub product: Option<ItemIx>,
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
    /// A ballista's shooting; `None` for a gatherer.
    pub defence: Option<Defence>,
    /// Hits from enemies' swings it takes before it breaks (B8).
    pub health: u32,
}

/// A ballista building's shooting (`scenario::Defence`), in ticks and item
/// indices: what a `DefenderState` holds, for a building.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defence {
    /// The one item its ammo store takes and it shoots.
    pub ammo: ItemIx,
    pub ammo_cap: u32,
    pub range: Fx,
    /// Ticks between shots.
    pub period: u64,
    pub damage: u32,
    /// Projectile speed, meters per second.
    pub speed: Fx,
    pub muzzle_height: Fx,
    pub projectile: scenario::ProjectileKind,
    /// What a possessing elder's Burst key throws (B4), if anything.
    pub fireball: Option<FireballDef>,
}

impl BuildingKind {
    /// The output's cap with `occupant` in the slot: a ballista's ammo
    /// store holds its ammo cap whoever is inside.
    pub fn cap_for(&self, occupant: Option<Occupant>) -> u32 {
        match (&self.defence, occupant) {
            (Some(d), _) => d.ammo_cap,
            (None, Some(Occupant::Elder(_))) => self.possessed_cap,
            (None, _) => self.cap,
        }
    }

    /// A possessing elder's reticle for an aim `offset` from the building
    /// (B3): the offset, cut to the ballista's range. `None` for a
    /// gatherer, or for no offset at all.
    pub fn reticle(&self, offset: FxVec2) -> Option<FxVec2> {
        let range = self.defence.as_ref()?.range;
        if offset == FxVec2::ZERO {
            return None;
        }
        Some(if offset.length() > range { offset.normalize_or_zero() * range } else { offset })
    }

    /// What the building's `output` holds at most: a gatherer's possessed
    /// cap (the most any occupant fills it to), a ballista's ammo cap.
    pub fn store_capacity(&self) -> u32 {
        self.defence.as_ref().map_or(self.possessed_cap, |d| d.ammo_cap)
    }

    /// What the building's `output` takes: a gatherer's one product, a
    /// ballista's ammo only.
    pub fn store_kinds(&self) -> Kinds {
        match &self.defence {
            Some(d) => Kinds::Only(d.ammo),
            None => Kinds::OneKind,
        }
    }
}

/// A ballista building's aim, as a `DefenderState` keeps it: when it may
/// shoot next, and where and when it last did (for the view). Stays at its
/// default for a gatherer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turret {
    /// The first tick it may shoot again.
    pub ready_at: u64,
    pub aim: Option<FxVec2>,
    pub fired_at: u64,
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
    /// up to the possessed cap (the most any occupant fills it to). A
    /// ballista's is its ammo store instead: its ammo only, up to the ammo
    /// cap, delivered to and never picked up from (B2); each shot spends
    /// one unit from it.
    pub output: Inventory,
    /// A ballista's aim and its next shot.
    pub turret: Turret,
    /// What is left of its kind's `health` (B8): an enemy's swing takes its
    /// damage, and at 0 it breaks (`Settlement::break_building`).
    pub health: u32,
}

impl BuildingState {
    pub fn built(&self) -> bool {
        self.stage == Stage::Built
    }

    /// A site's next part and the materials delivered for it; `None` once
    /// built.
    pub fn site(&self) -> Option<(u8, &Inventory)> {
        match &self.stage {
            Stage::Site { part, materials } => Some((*part, materials)),
            Stage::Built => None,
        }
    }

    /// What a site's next part still lacks on the ground, in the part's
    /// order: its materials less what has landed (units in the air still
    /// count as missing). Empty once the part is complete, and for a built
    /// building.
    pub fn needs(&self, kind: &BuildingKind) -> Vec<(ItemIx, u32)> {
        let Some((part, materials)) = self.site() else { return Vec::new() };
        let part = kind.parts.get(part as usize).map_or(&[][..], |p| &p[..]);
        part.iter().map(|&(item, units)| (item, units.saturating_sub(materials.count(item)))).filter(|&(_, units)| units > 0).collect()
    }

    /// Units of `item` a site's next part still takes: its need less what
    /// has landed and what is on its way. Zero for an item the part doesn't
    /// use, and for a built building.
    pub fn wanted(&self, kind: &BuildingKind, item: ItemIx) -> u32 {
        let Some((part, materials)) = self.site() else { return 0 };
        let need = kind.parts.get(part as usize).and_then(|p| p.iter().find(|&&(i, _)| i == item)).map_or(0, |&(_, units)| units);
        need.saturating_sub(materials.count(item) + materials.reserved_in[item as usize])
    }

    /// Whether a site's next part has all its materials on the ground, so
    /// it can be installed.
    pub fn ready(&self, kind: &BuildingKind) -> bool {
        self.site().is_some() && self.needs(kind).is_empty()
    }

    /// Where the occupant stands (D2): just past the footprint, a quarter
    /// turn from the output pile, so the two never share a spot.
    pub fn slot_position(&self, kind: &BuildingKind) -> FxVec2 {
        slot_position(self.position, self.output_position, kind)
    }

    /// A building of kind `k` at `center`, at `stage`, with its slot and
    /// output empty. A site's materials take any mix up to its biggest part.
    fn new(k: BuildingKindIx, kind: &BuildingKind, center: FxVec2, output: FxVec2, stage: Stage, items: usize) -> BuildingState {
        BuildingState {
            kind: k,
            position: center,
            output_position: output,
            stage,
            slot: Slot::Empty,
            progress: Fx::ZERO,
            output: Inventory::new(items, kind.store_capacity(), kind.store_kinds()),
            turret: Turret::default(),
            health: kind.health,
        }
    }
}

/// An empty site's materials for `kind`: any mix, up to the most units a
/// part takes. Only what the next part still needs comes in
/// (`BuildingState::wanted`), so the capacity is never the limit.
fn materials(kind: &BuildingKind, items: usize) -> Inventory {
    let most = kind.parts.iter().map(|p| p.iter().map(|&(_, units)| units).sum::<u32>()).max().unwrap_or(0);
    Inventory::new(items, most, Kinds::AnyMix)
}

/// Resolves the scenario's building kinds. `item` maps an item's id to its
/// index; the scenario is validated.
pub(super) fn kinds(scenario: &Scenario, item: impl Fn(&str) -> ItemIx) -> Vec<BuildingKind> {
    scenario
        .building_kinds
        .iter()
        .map(|k| BuildingKind {
            id: k.id.clone(),
            product: k.product.as_deref().map(&item),
            radius: k.radius,
            period: scenario::ticks(k.period),
            cap: k.cap,
            possessed_cap: k.possessed_cap,
            possessed_multiplier: k.possessed_multiplier,
            burst: k.burst.units,
            burst_cooldown: scenario::ticks(k.burst.cooldown),
            output_distance: k.output_distance,
            health: k.health,
            parts: k.parts.iter().map(|p| p.materials.iter().map(|s| (item(&s.item), s.units)).collect()).collect(),
            defence: k.defence.as_ref().map(|d| Defence {
                ammo: item(&d.ammo),
                ammo_cap: d.ammo_cap,
                range: d.range,
                period: scenario::ticks(d.period),
                damage: d.damage,
                speed: d.projectile_speed,
                muzzle_height: d.muzzle_height,
                projectile: d.projectile,
                fireball: d.fireball.as_ref().map(|f| FireballDef {
                    cooldown: scenario::ticks(f.cooldown),
                    speed: f.speed,
                    damage: f.damage,
                    radius: f.radius,
                    fire_radius: f.fire_radius,
                    fire_ticks: scenario::ticks(f.fire_seconds),
                    burn: BurnDef { damage: f.burn_damage, period: scenario::ticks(f.burn_period), linger: scenario::ticks(f.burn_linger) },
                }),
            }),
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

/// Why a building can't go where it was put (`placement`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementRefusal {
    /// The scenario has no such building kind.
    UnknownKind,
    /// The footprint reaches past the arena's edge.
    OutsideArena,
    /// The footprint overlaps an obstacle (the tree, a prop), another
    /// building or someone standing there.
    Blocked,
    /// The footprint covers a depot's dock (a tree pile).
    CoversDock(DepotIx),
    /// The footprint covers another building's output pile or slot spot.
    CoversSpot(BuildingIx),
    /// Its own output pile is blocked, or crowds a dock or another
    /// building's output pile or slot spot.
    OutputBlocked,
    /// Its own slot spot is blocked, or crowds a dock or another building's
    /// output pile or slot spot.
    SlotBlocked,
    /// Nobody could walk from the depots (the tree's piles) to its output
    /// pile or its slot spot.
    Unreachable,
}

impl PlacementRefusal {
    /// What a blueprint's ghost says: "BLOCKED", "TOO CLOSE TO A PILE".
    pub fn message(self) -> &'static str {
        match self {
            PlacementRefusal::UnknownKind => "UNKNOWN BUILDING",
            PlacementRefusal::OutsideArena => "OUT OF BOUNDS",
            PlacementRefusal::Blocked | PlacementRefusal::OutputBlocked | PlacementRefusal::SlotBlocked => "BLOCKED",
            PlacementRefusal::CoversDock(_) | PlacementRefusal::CoversSpot(_) => "TOO CLOSE TO A PILE",
            PlacementRefusal::Unreachable => "CAN'T BE REACHED",
        }
    }
}

/// What a placement is checked against.
#[derive(Clone, Copy)]
pub(super) struct Ground<'a> {
    pub kinds: &'a [BuildingKind],
    /// The depots' docks: no footprint covers one, and reachability starts
    /// there. They also mark the tree for the output pile's side.
    pub docks: &'a [FxVec2],
    /// The buildings standing; their footprints are already in the terrain.
    pub buildings: &'a [BuildingState],
    /// Bodies a footprint must not land on.
    pub bodies: &'a [Disc],
    pub terrain: &'a Terrain,
}

/// Whether a building of kind `k` can stand at `center`, and if so where its
/// output pile goes: one check for the scenario's prebuilt buildings and a
/// player's blueprint (`PlaceBlueprint`), in this order.
///
/// - The footprint is inside the arena, clear of the terrain (props, the
///   tree, the buildings before it) and the `bodies`, and off every dock and
///   the other buildings' output piles and slot spots, which are no
///   obstacles.
/// - Its own output pile and slot spot are clear, its footprint counted,
///   and off the docks and the other buildings' piles and slot spots.
/// - From every dock, a body as wide as one that stands at a slot spot can
///   walk to the slot spot (a sprite that claims it walks there) and to
///   where the elder stands at the output pile. With no docks there is
///   nothing to walk from, and this is skipped.
pub(super) fn placement(ground: &Ground, k: BuildingKindIx, center: FxVec2) -> Result<FxVec2, PlacementRefusal> {
    let Ground { kinds, docks, buildings, bodies, terrain } = *ground;
    let kind = kinds.get(k as usize).ok_or(PlacementRefusal::UnknownKind)?;
    let reach = FxVec2::new(kind.radius, kind.radius);
    if !terrain.inside(center - reach) || !terrain.inside(center + reach) {
        return Err(PlacementRefusal::OutsideArena);
    }
    let apart = |p: FxVec2, q: FxVec2, gap: Fx| p.distance(q) > gap + CLEARANCE_MARGIN;
    if !terrain.clear(center, center, kind.radius) || bodies.iter().any(|b| !apart(center, b.center, kind.radius + b.radius)) {
        return Err(PlacementRefusal::Blocked);
    }
    if let Some(d) = docks.iter().position(|&p| !apart(center, p, kind.radius + DOCK_RADIUS)) {
        return Err(PlacementRefusal::CoversDock(d as DepotIx));
    }
    // Each other building's output pile and slot spot, with the clearance
    // each keeps.
    let spots: Vec<(BuildingIx, FxVec2, Fx)> = buildings
        .iter()
        .enumerate()
        .flat_map(|(i, b)| [(i as BuildingIx, b.output_position, DOCK_RADIUS), (i as BuildingIx, b.slot_position(&kinds[b.kind as usize]), SLOT_RADIUS)])
        .collect();
    if let Some(&(b, _, _)) = spots.iter().find(|&&(_, p, r)| !apart(center, p, kind.radius + r)) {
        return Err(PlacementRefusal::CoversSpot(b));
    }
    let output = output_position(center, kind.output_distance, docks, terrain.center);
    let slot = slot_position(center, output, kind);
    let footprint = [Disc { center, radius: kind.radius }];
    for (spot, radius, refusal) in [(output, DOCK_RADIUS, PlacementRefusal::OutputBlocked), (slot, SLOT_RADIUS, PlacementRefusal::SlotBlocked)] {
        let crowded = spots.iter().any(|&(_, p, r)| !apart(spot, p, radius + r)) || docks.iter().any(|&p| !apart(spot, p, radius + DOCK_RADIUS));
        if crowded || !terrain.clear_with(spot, spot, radius, &footprint) {
            return Err(refusal);
        }
    }
    // The elder stands just in front of the pile (`interact::stand_spot`).
    let stand = output + (output - center).normalize_or_zero() * STAND_GAP;
    let walkable = |p: FxVec2, q: FxVec2| terrain.walkable(p, q, SLOT_RADIUS);
    for &from in docks {
        for to in [stand, slot] {
            if nav::path_with(terrain, from, to, SLOT_RADIUS, &footprint, &walkable).is_empty() {
                return Err(PlacementRefusal::Unreachable);
            }
        }
    }
    Ok(output)
}

/// Puts up the scenario's prebuilt buildings, built and empty, in scenario
/// order: each where `placement` allows it with the buildings before it
/// standing, and its footprint becomes an obstacle. `at` places a scenario
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
    let mut buildings: Vec<BuildingState> = Vec::with_capacity(scenario.buildings.len());
    for (i, b) in scenario.buildings.iter().enumerate() {
        let k = kinds.iter().position(|k| k.id == b.kind).expect("validated building kind") as BuildingKindIx;
        let center = at(b.position);
        let ground = Ground { kinds, docks: &docks, buildings: &buildings, bodies: &[], terrain };
        let output = placement(&ground, k, center).map_err(|refusal| {
            let why = match refusal {
                PlacementRefusal::UnknownKind => "has an unknown kind".to_string(),
                PlacementRefusal::OutsideArena => "reaches outside the arena".to_string(),
                PlacementRefusal::Blocked => "overlaps an obstacle or another building".to_string(),
                PlacementRefusal::CoversDock(d) => format!("covers depot {}'s dock", scenario.depots[depots[d as usize].0 as usize].id),
                PlacementRefusal::CoversSpot(other) => format!("covers building {other}'s output pile or slot spot"),
                PlacementRefusal::OutputBlocked => "has its output pile blocked".to_string(),
                PlacementRefusal::SlotBlocked => "has its slot spot blocked".to_string(),
                PlacementRefusal::Unreachable => "can't be reached on foot from the depots".to_string(),
            };
            format!("building {i} ({}) at {:?} {why}", b.kind, b.position)
        })?;
        terrain.obstacles.push(Disc { center, radius: kinds[k as usize].radius });
        buildings.push(BuildingState::new(k, &kinds[k as usize], center, output, Stage::Built, items));
    }
    if !buildings.is_empty() {
        terrain.revision += 1;
    }
    Ok(buildings)
}

/// Why a Burst didn't happen (`Settlement::bursts`). Stage 5 reports it as
/// `Event::BurstRefused`; `message` says it for the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BurstRefusal {
    /// Only an elder possessing a built building can Burst (D3).
    NotPossessing,
    /// The elder's last Burst is still cooling down, for `ticks` more.
    Cooling { ticks: u64 },
    /// The output already holds its possessed cap.
    OutputFull,
    /// The building possessed has no Burst (a ballista with no fireball).
    NoBurst,
    /// The elder's last fireball is still cooling down, for `ticks` more
    /// (B4): the Burst key in a ballista throws one.
    FireballCooling { ticks: u64 },
}

/// What the Burst key does now (`Settlement::burst_check`): in a gatherer,
/// a Burst of `units`; in a ballista, a fireball that lands at `to`
/// (world tiles), the reticle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ability {
    Burst { units: u32 },
    Fireball { to: FxVec2 },
}

impl BurstRefusal {
    /// The refusal in words, for the view: a cooldown in whole seconds,
    /// rounded up, so it never reads "0 s" while it still runs.
    pub fn message(self) -> String {
        match self {
            BurstRefusal::NotPossessing => "Possess a building to Burst".to_string(),
            BurstRefusal::Cooling { ticks } => format!("Burst ready in {} s", ticks.div_ceil(content::TICKS_PER_SECOND as u64)),
            BurstRefusal::OutputFull => "Output full".to_string(),
            BurstRefusal::NoBurst => "This building has no Burst".to_string(),
            BurstRefusal::FireballCooling { ticks } => format!("Fireball ready in {} s", ticks.div_ceil(content::TICKS_PER_SECOND as u64)),
        }
    }
}

/// Why a possessing elder's shot wouldn't go off now
/// (`Settlement::fire_check`). Stage 5 drops such a shot without a word;
/// the view says it (`message`) when the fire key is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FireRefusal {
    /// The elder possesses no ballista.
    NotInBallista,
    /// Its ammo store is empty.
    NoAmmo,
    /// It shot less than a period ago: `ticks` more.
    Reloading { ticks: u64 },
}

impl FireRefusal {
    /// The refusal in words, for the view.
    pub fn message(self) -> String {
        match self {
            FireRefusal::NotInBallista => "Possess a ballista to fire".to_string(),
            FireRefusal::NoAmmo => "No ammo: bring stone".to_string(),
            FireRefusal::Reloading { .. } => "Reloading".to_string(),
        }
    }
}

/// Whether `agent`, seated in `seat`, could shoot its ballista on `tick`
/// (B3): the building and its defence, or why not. An empty store reads
/// before a reload, so holding the fire key on an empty ballista says so.
fn fire_plan<'a>(agent: AgentIx, seat: Option<Seat>, tick: u64, buildings: &[BuildingState], kinds: &'a [BuildingKind]) -> Result<(BuildingIx, &'a Defence), FireRefusal> {
    let Some((ix, b)) = seat.and_then(Seat::building).and_then(|ix| Some((ix, buildings.get(ix as usize)?))) else {
        return Err(FireRefusal::NotInBallista);
    };
    let kind = &kinds[b.kind as usize];
    let Some(defence) = kind.defence.as_ref().filter(|_| b.built() && b.slot.occupant() == Some(Occupant::Elder(agent))) else {
        return Err(FireRefusal::NotInBallista);
    };
    if b.output.available(defence.ammo) == 0 {
        return Err(FireRefusal::NoAmmo);
    }
    if b.turret.ready_at > tick {
        return Err(FireRefusal::Reloading { ticks: b.turret.ready_at - tick });
    }
    Ok((ix, defence))
}

/// What the Burst key would do (`Settlement::bursts`): the building
/// `avatar` possesses, and the units that fit in its output, or, in a
/// ballista with a fireball (B4), where the fireball lands: the reticle
/// (`aim`, cut to range; half the range straight ahead before any aim). Or
/// why there is none. A cooldown running reads before a full output; a
/// Burst and a fireball each have their own.
fn burst_plan(avatar: &super::avatar::AvatarState, tick: u64, buildings: &[BuildingState], kinds: &[BuildingKind]) -> Result<(BuildingIx, Ability), BurstRefusal> {
    let Some((b, building)) = avatar.seat.and_then(Seat::building).and_then(|b| Some((b, buildings.get(b as usize)?))).filter(|(_, building)| building.built()) else {
        return Err(BurstRefusal::NotPossessing);
    };
    let kind = &kinds[building.kind as usize];
    if let Some(defence) = kind.defence.as_ref().filter(|d| d.fireball.is_some()) {
        if tick < avatar.fireball_ready {
            return Err(BurstRefusal::FireballCooling { ticks: avatar.fireball_ready - tick });
        }
        let ahead = FxVec2::new(Fx::ZERO, defence.range.div_int(2));
        let offset = avatar.aim.and_then(|aim| kind.reticle(aim)).unwrap_or(ahead);
        return Ok((b, Ability::Fireball { to: building.position + offset }));
    }
    let ready = avatar.burst_ready;
    if tick < ready {
        return Err(BurstRefusal::Cooling { ticks: ready - tick });
    }
    if kind.product.is_none() || kind.burst == 0 {
        return Err(BurstRefusal::NoBurst);
    }
    let held = building.output.total() + building.output.total_in();
    match kind.burst.min(kind.possessed_cap.saturating_sub(held)) {
        0 => Err(BurstRefusal::OutputFull),
        units => Ok((b, Ability::Burst { units })),
    }
}

/// A ballista's own shooting, when building `b` of `kind` can shoot on
/// `tick`: a ballista, built, with a sprite inside, ammo in its store and
/// its period over since the last shot. `None` for a gatherer, a site, an
/// empty, unstocked or possessed ballista (a possessing elder aims it
/// instead, `fire_plan`), or one still reloading.
fn ready_to_shoot<'a>(b: &BuildingState, kind: &'a BuildingKind, tick: u64) -> Option<&'a Defence> {
    let defence = kind.defence.as_ref()?;
    let stocked = b.output.available(defence.ammo) > 0;
    let staffed = matches!(b.slot.occupant(), Some(Occupant::Sprite(_)));
    (b.built() && staffed && stocked && b.turret.ready_at <= tick).then_some(defence)
}

/// Where a building's occupant stands (D2): just past the footprint, a
/// quarter turn from the output pile, so the two never share a spot.
fn slot_position(center: FxVec2, output: FxVec2, kind: &BuildingKind) -> FxVec2 {
    let out = (output - center).normalize_or_zero();
    let out = if out == FxVec2::ZERO { FxVec2::new(Fx::ZERO, Fx::ONE) } else { out };
    center + FxVec2::new(-out.y, out.x) * (kind.radius + SLOT_GAP)
}

/// The ways out of a building an elder leaving it takes, first to last:
/// behind the slot, behind the building, round the far side, then in front
/// of the slot. Each is a step (along the slot's side, along the output's
/// way); the spot is on the slot spot's ring that way (`beside`).
const LEAVING: [(i32, i32); 6] = [(1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (1, 1)];
/// The ways a sprite an elder put out goes: in front of the slot first, on
/// the side away from where the elder leaves, so its walk back to the slot
/// spot never passes the elder; then as the elder does.
const PUT_OUT: [(i32, i32); 6] = [(1, 1), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1)];

/// The clear spots beside building `b` where a body of `radius` can be put
/// (D3), the `ways` in order: an elder leaving it, or a sprite an elder put
/// out. They sit on the slot spot's ring, each a step from the slot spot and
/// from one another. A spot is left out unless it is inside the arena,
/// clear of the terrain (the building's own footprint among it), and off
/// the docks and every building's output pile and slot spot, as a placement
/// keeps them. Bodies don't count: the view doesn't read the agents, and a
/// body that stands there walks off.
fn beside(ground: &Ground, b: BuildingIx, radius: Fx, ways: &[(i32, i32)]) -> Vec<FxVec2> {
    let Ground { kinds, docks, buildings, terrain, .. } = *ground;
    let Some(building) = buildings.get(b as usize) else { return Vec::new() };
    let kind = &kinds[building.kind as usize];
    let out = (building.output_position - building.position).normalize_or_zero();
    let out = if out == FxVec2::ZERO { FxVec2::new(Fx::ZERO, Fx::ONE) } else { out };
    let side = FxVec2::new(-out.y, out.x);
    let ring = kind.radius + SLOT_GAP;
    let apart = |p: FxVec2, q: FxVec2, gap: Fx| p.distance(q) > gap + CLEARANCE_MARGIN;
    let spots: Vec<(FxVec2, Fx)> = buildings
        .iter()
        .flat_map(|o| [(o.output_position, DOCK_RADIUS), (o.slot_position(&kinds[o.kind as usize]), SLOT_RADIUS)])
        .chain(docks.iter().map(|&d| (d, DOCK_RADIUS)))
        .collect();
    ways.iter()
        .map(|&(along, ahead)| building.position + (side * Fx::from_int(along) + out * Fx::from_int(ahead)).normalize_or_zero() * ring)
        .filter(|&p| {
            let r = FxVec2::new(radius, radius);
            terrain.inside(p - r)
                && terrain.inside(p + r)
                && terrain.footprint(p, radius).is_some()
                && terrain.clear(p, p, radius)
                && spots.iter().all(|&(q, gap)| apart(p, q, radius + gap))
        })
        .collect()
}

/// Where an elder of `radius` leaving building `b` stands: the first clear
/// way out (`LEAVING`). `None` when every one is blocked; the elder then
/// stays in, as a driver does in a vehicle boxed in on every side.
pub(super) fn leave_spot(ground: &Ground, b: BuildingIx, radius: Fx) -> Option<FxVec2> {
    beside(ground, b, radius, &LEAVING).first().copied()
}

/// Where a sprite of `radius` that an elder of `elder_radius` put out of
/// building `b` stands: the first clear way (`PUT_OUT`) that isn't the
/// elder's leaving spot, so the two never land on one another, nor on the
/// slot spot the sprite walks back to (a walker whose goal sits under
/// another body holds back, `haulers::move_worker`). With no other spot,
/// the elder's.
pub(super) fn eject_spot(ground: &Ground, b: BuildingIx, radius: Fx, elder_radius: Fx) -> Option<FxVec2> {
    let elder = leave_spot(ground, b, elder_radius);
    let spots = beside(ground, b, radius, &PUT_OUT);
    spots.iter().copied().find(|&p| Some(p) != elder).or_else(|| spots.first().copied())
}

impl super::action::View<'_> {
    /// Where `me`, of `radius`, stands on leaving building `b` (`leave_spot`).
    pub(super) fn leave_spot(&self, b: BuildingIx, radius: Fx) -> Option<FxVec2> {
        let docks: Vec<FxVec2> = self.depots.iter().map(|d| d.position).collect();
        let ground = Ground { kinds: self.building_kinds, docks: &docks, buildings: self.buildings, bodies: &[], terrain: self.terrain };
        leave_spot(&ground, b, radius)
    }
}

impl Settlement {
    /// Where an elder of `radius` would stand on leaving building `b`
    /// (`leave_spot`): for scripts and the view.
    pub fn leave_spot(&self, b: BuildingIx, radius: Fx, terrain: &Terrain) -> Option<FxVec2> {
        let docks: Vec<FxVec2> = self.depots.iter().map(|d| d.position).collect();
        let ground = Ground { kinds: &self.building_kinds, docks: &docks, buildings: &self.buildings, bodies: &[], terrain };
        leave_spot(&ground, b, radius)
    }

    /// Where a sprite of `radius` put out of building `b` by an elder of
    /// `elder_radius` stands (`eject_spot`).
    pub(super) fn eject_spot(&self, b: BuildingIx, radius: Fx, elder_radius: Fx, terrain: &Terrain) -> Option<FxVec2> {
        let docks: Vec<FxVec2> = self.depots.iter().map(|d| d.position).collect();
        let ground = Ground { kinds: &self.building_kinds, docks: &docks, buildings: &self.buildings, bodies: &[], terrain };
        eject_spot(&ground, b, radius, elder_radius)
    }

    /// The kind of building `b`.
    pub fn building_kind(&self, b: BuildingIx) -> &BuildingKind {
        &self.building_kinds[self.buildings[b as usize].kind as usize]
    }

    /// Whether a blueprint of kind `kind` could go down at `position` now
    /// (`placement`, with every live agent's body in the way): what stage 5
    /// checks for `PlaceBlueprint`, and what the app's ghost shows, so the
    /// two never disagree. `Ok` holds where its output pile would go.
    pub fn placement_check(&self, kind: BuildingKindIx, position: FxVec2, terrain: &Terrain) -> Result<FxVec2, PlacementRefusal> {
        let docks: Vec<FxVec2> = self.depots.iter().map(|d| d.position).collect();
        let bodies: Vec<Disc> = self.agents.iter().filter(|a| a.body.alive()).map(|a| Disc { center: a.body.position, radius: a.radius() }).collect();
        let ground = Ground { kinds: &self.building_kinds, docks: &docks, buildings: &self.buildings, bodies: &bodies, terrain };
        placement(&ground, kind, position)
    }

    /// Where a building of kind `kind` at `position` would have its output
    /// pile and its slot spot, whether or not it may stand there: for the
    /// app's ghost.
    pub fn blueprint_spots(&self, kind: BuildingKindIx, position: FxVec2, terrain: &Terrain) -> Option<(FxVec2, FxVec2)> {
        let k = self.building_kinds.get(kind as usize)?;
        let docks: Vec<FxVec2> = self.depots.iter().map(|d| d.position).collect();
        let output = output_position(position, k.output_distance, &docks, terrain.center);
        Some((output, slot_position(position, output, k)))
    }

    /// Stage 5: this tick's blueprints, in command order, so of two that
    /// overlap the first stands. Each one `placement_check` allows becomes a
    /// site waiting for its first part's materials, its footprint an
    /// obstacle; the rest are refused with an event.
    /// Enemies then plan their routes afresh round it (`replan_routes`).
    pub(super) fn place_blueprints(&mut self, messages: Vec<PlaceBlueprint>, terrain: &mut Terrain) {
        let revision = terrain.revision;
        for PlaceBlueprint { kind, position } in messages {
            match self.placement_check(kind, position, terrain) {
                Ok(output) => {
                    let k = &self.building_kinds[kind as usize];
                    let stage = Stage::Site { part: 0, materials: materials(k, self.items.len()) };
                    terrain.obstacles.push(Disc { center: position, radius: k.radius });
                    terrain.revision += 1;
                    self.buildings.push(BuildingState::new(kind, k, position, output, stage, self.items.len()));
                    self.events.push(Event::BlueprintPlaced { building: (self.buildings.len() - 1) as BuildingIx });
                }
                Err(reason) => self.events.push(Event::BlueprintRefused { kind, reason }),
            }
        }
        if terrain.revision != revision {
            self.replan_routes(terrain);
        }
    }

    /// Stage 5: installs, in command order. A site whose next part has all
    /// its materials on the ground uses them up (the balances count them as
    /// consumed) and moves on to the next part; the last part makes it
    /// built, empty until someone occupies it. One that isn't ready (an
    /// install earlier in the order took the part) does nothing.
    pub(super) fn install(&mut self, messages: Vec<Install>) {
        for Install { building, .. } in messages {
            let Some(b) = self.buildings.get(building as usize) else { continue };
            let kind = &self.building_kinds[b.kind as usize];
            if !b.ready(kind) {
                continue;
            }
            let parts = kind.parts.len();
            let Stage::Site { part, materials } = &mut self.buildings[building as usize].stage else { unreachable!("ready means a site") };
            let installed = *part;
            for &(item, units) in &kind.parts[installed as usize] {
                materials.remove(item, units);
                self.accounts[item as usize].consumed += units as u64;
            }
            *part += 1;
            if *part as usize == parts {
                self.buildings[building as usize].stage = Stage::Built;
            }
            self.events.push(Event::PartInstalled { building, part: installed, built: installed as usize + 1 == parts });
        }
    }

    /// Stage 5: Bursts, in command order (D3). An elder still possessing a
    /// building, its cooldown over and the output not at its possessed cap,
    /// puts the kind's Burst units in the output (as many as fit; the
    /// balances count them as produced) and starts its cooldown. Otherwise
    /// the view hears why, and the cooldown doesn't start. In a ballista
    /// with a fireball, its own cooldown over, it throws the fireball at the
    /// reticle instead (B4, `throw_fireball`).
    pub(super) fn bursts(&mut self, messages: Vec<Burst>, tick: u64) {
        for Burst { agent } in messages {
            let Some(avatar) = self.agents.get(agent as usize).and_then(|a| a.avatar()) else { continue };
            match burst_plan(avatar, tick, &self.buildings, &self.building_kinds) {
                Ok((building, Ability::Fireball { to })) => {
                    let defence = self.building_kind(building).defence.as_ref().expect("a ballista");
                    let (def, height) = (defence.fireball.expect("a fireball"), defence.muzzle_height);
                    self.throw_fireball(tick, agent, building, to, def, height);
                }
                Ok((building, Ability::Burst { units })) => {
                    let kind = &self.building_kinds[self.buildings[building as usize].kind as usize];
                    let (product, cooldown) = (kind.product.expect("only a gatherer Bursts"), kind.burst_cooldown);
                    let added = self.buildings[building as usize].output.add(product, units);
                    debug_assert!(added, "below the possessed cap, which is the capacity");
                    self.accounts[product as usize].produced += units as u64;
                    if let Some(avatar) = self.agents[agent as usize].avatar_mut() {
                        avatar.burst_ready = tick + cooldown;
                    }
                    self.events.push(Event::BuildingBurst { building, units });
                }
                Err(reason) => self.events.push(Event::BurstRefused { agent, reason }),
            }
        }
    }

    /// What `agent`'s Burst key would do on `tick` (a Burst's units, or a
    /// fireball and where it lands), as stage 5 checks it (`bursts`): for
    /// the view's ready indicator and the control verb.
    pub fn burst_check(&self, agent: usize, tick: u64) -> Result<Ability, BurstRefusal> {
        let Some(avatar) = self.agents.get(agent).and_then(|a| a.avatar()) else { return Err(BurstRefusal::NotPossessing) };
        burst_plan(avatar, tick, &self.buildings, &self.building_kinds).map(|(_, ability)| ability)
    }

    /// Stage 3: every ballista that can shoot now (`ready_to_shoot`) aims at
    /// the nearest threat in range that the shots in flight won't kill
    /// (`pick_target`), as a defender does.
    pub(super) fn aim_buildings(&self, tick: u64, out: &mut Vec<(FxVec2, Messages)>) {
        for (i, b) in self.buildings.iter().enumerate() {
            let kind = &self.building_kinds[b.kind as usize];
            let Some(defence) = ready_to_shoot(b, kind, tick) else { continue };
            if let Some(t) = self.pick_target(b.position, defence.range) {
                let mut messages = Messages::default();
                messages.building_fire.push(BuildingFire { building: i as BuildingIx, target: t.id, to: t.position });
                out.push((b.position, messages));
            }
        }
    }

    /// Stage 5: a ballista's shot, if it can still shoot (`ready_to_shoot`):
    /// one unit of ammo leaves its store (spent, in the balances) and the
    /// shot flies (`launch_shot`), then it waits out its period.
    pub(crate) fn building_fire(&mut self, tick: u64, message: BuildingFire) {
        let Some(b) = self.buildings.get(message.building as usize) else { return };
        let kind = &self.building_kinds[b.kind as usize];
        let Some(defence) = ready_to_shoot(b, kind, tick) else { return };
        let launcher = Launcher { from: b.position, height: defence.muzzle_height, damage: defence.damage, speed: defence.speed, kind: defence.projectile, defender: None };
        let (ammo, period) = (defence.ammo, defence.period);
        let shot = Event::BuildingShot { building: message.building };
        self.launch_shot(tick, &launcher, InventoryRef::Output(message.building), ammo, Some(message.target), message.to, shot);
        let turret = &mut self.buildings[message.building as usize].turret;
        turret.aim = Some(message.to);
        turret.ready_at = tick + period;
        turret.fired_at = tick;
    }

    /// Stage 5: possessing elders' shots at their reticles (B3), in command
    /// order, each through the shared launch (`launch_shot`) if the elder
    /// can still shoot (`fire_plan`): one unit of ammo spent, the bolt aimed
    /// at no threat in particular (it hits whatever it meets on its way),
    /// and the ballista's period to wait. A second shot on the same tick, or
    /// one before the period is over, does nothing.
    pub(super) fn elder_fire(&mut self, messages: Vec<ElderFire>, tick: u64) {
        for ElderFire { agent, building, to } in messages {
            let seat = self.agents.get(agent as usize).and_then(|a| a.avatar()).and_then(|a| a.seat);
            let Ok((b, defence)) = fire_plan(agent, seat, tick, &self.buildings, &self.building_kinds) else { continue };
            if b != building {
                continue;
            }
            let from = self.buildings[b as usize].position;
            let launcher = Launcher { from, height: defence.muzzle_height, damage: defence.damage, speed: defence.speed, kind: defence.projectile, defender: None };
            let (ammo, period) = (defence.ammo, defence.period);
            self.launch_shot(tick, &launcher, InventoryRef::Output(b), ammo, None, to, Event::BuildingShot { building: b });
            let turret = &mut self.buildings[b as usize].turret;
            turret.aim = Some(to);
            turret.ready_at = tick + period;
            turret.fired_at = tick;
        }
    }

    /// Whether `agent` could shoot the ballista it possesses on `tick`, as
    /// stage 5 checks it (`elder_fire`): for the view's fire key.
    pub fn fire_check(&self, agent: usize, tick: u64) -> Result<BuildingIx, FireRefusal> {
        let seat = self.agents.get(agent).and_then(|a| a.avatar()).and_then(|a| a.seat);
        fire_plan(agent as AgentIx, seat, tick, &self.buildings, &self.building_kinds).map(|(b, _)| b)
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
            let Some(product) = kind.product else { continue };
            let held = b.output.total() + b.output.total_in();
            if !b.built() || held >= kind.cap_for(Some(occupant)) {
                continue;
            }
            let work = match occupant {
                Occupant::Sprite(_) => Fx::ONE,
                Occupant::Elder(_) => kind.possessed_multiplier,
            };
            let period = Fx::ONE.mul_int(kind.period as i64);
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

    /// Stage 5: an enemy's swing takes `damage` from building `b`'s health
    /// (B8). At 0 it breaks (`break_building`). Returns whether it broke.
    pub(super) fn hurt_building(&mut self, b: BuildingIx, damage: u32, terrain: &Terrain) -> bool {
        let building = &mut self.buildings[b as usize];
        building.health = building.health.saturating_sub(damage);
        if building.health > 0 {
            return false;
        }
        self.break_building(b, terrain);
        true
    }

    /// Breaks building `b` (B8): it becomes a site at its first part again,
    /// at full health, its parts lost (consumed when they went in). Nothing
    /// that touches it is left hanging:
    /// - units flying to or from it land now, as when a player leaves;
    /// - carry jobs on its output or its site end (`drop_carry`), giving
    ///   back what they hold;
    /// - its occupant is put out beside it: a sprite (its plan breaks, so it
    ///   replans, D2) or a possessing elder, whose reticle turns back into a
    ///   facing, as leaving does; a sprite on its way loses its claim;
    /// - what its output held (a ballista's ammo) and what was delivered to
    ///   it as a site spill onto ground piles beside it, one per item
    ///   (`spill`), so every unit is still stored and the balances hold.
    ///
    /// Its footprint stays an obstacle: the site stands where it stood.
    pub(super) fn break_building(&mut self, b: BuildingIx, terrain: &Terrain) {
        let holders = [InventoryRef::Output(b), InventoryRef::Site(b)];
        let flying: Vec<u64> = self.transfers.values().filter(|t| holders.contains(&t.from) || holders.contains(&t.to)).map(|t| t.id).collect();
        for id in flying {
            let t = self.transfers.remove(&id).expect("listed transfer");
            self.land(t);
        }
        for agent in 0..self.agents.len() {
            let carry = &mut self.agents[agent].carry;
            if carry.job.iter().chain(&carry.next).any(|job| holders.contains(&job.other)) {
                let releases = carry.drop_carry(agent as AgentIx);
                self.reserve_stock(releases);
            }
        }
        match self.buildings[b as usize].slot {
            Slot::Empty => {}
            Slot::Claimed(s) => self.release_slot(s),
            Slot::Occupied(Occupant::Sprite(s)) => {
                let radius = self.agents[s as usize].radius();
                let spot = self.eject_spot(b, radius, radius, terrain);
                self.release_slot(s);
                let body = &mut self.agents[s as usize].body;
                if let Some(spot) = spot {
                    body.position = spot;
                }
                body.route.clear();
                body.velocity = FxVec2::ZERO;
                body.push = FxVec2::ZERO;
            }
            Slot::Occupied(Occupant::Elder(e)) => {
                let radius = self.agents[e as usize].radius();
                let spot = self.leave_spot(b, radius, terrain);
                self.buildings[b as usize].slot = Slot::Empty;
                if let super::AgentState { body, role: super::Role::Avatar(avatar), .. } = &mut self.agents[e as usize] {
                    avatar.seat = None;
                    avatar.aim = avatar.aim.map(FxVec2::normalize_or_zero).filter(|aim| *aim != FxVec2::ZERO);
                    avatar.intent = Some(super::avatar::Intent::Stop);
                    if let Some(spot) = spot {
                        body.position = spot;
                        if let Some(ground) = terrain.footprint(spot, radius) {
                            avatar.height = ground;
                            avatar.flight.support = ground;
                        }
                    }
                }
            }
        }
        let items = self.items.len();
        let building = &mut self.buildings[b as usize];
        let kind = &self.building_kinds[building.kind as usize];
        let mut spilled = building.output.counts.clone();
        if let Stage::Site { materials, .. } = &building.stage {
            for (units, &more) in spilled.iter_mut().zip(&materials.counts) {
                *units += more;
            }
        }
        building.output = Inventory::new(items, kind.store_capacity(), kind.store_kinds());
        building.stage = Stage::Site { part: 0, materials: materials(kind, items) };
        building.progress = Fx::ZERO;
        building.turret = Turret::default();
        building.health = kind.health;
        let (center, radius) = (building.position, kind.radius);
        for (item, units) in spilled.into_iter().enumerate().filter(|&(_, units)| units > 0) {
            self.spill(item as ItemIx, units, center, radius, terrain);
        }
        self.events.push(Event::BuildingBroken { building: b });
    }

    /// Checks every building on its own: a known kind, a site's part in
    /// range, the output holding the kind's product only, its reservations
    /// against the bundles picking up from it (`expected_reservations`, as
    /// for piles), and a site's materials: their reservations against the
    /// deliveries on their way (likewise), nothing promised out (nothing is
    /// taken back), no more of an item than the next part takes, and an
    /// empty slot and output (nobody works a site). A built building has no
    /// delivery left on its way. Then the slots against
    /// the agents, both ways: a claim or a sprite inside names a sprite that
    /// holds just that (`SpriteState::holds`), an elder names an avatar
    /// seated there (`Seat::Building`), every sprite's hold is that
    /// building's slot, and every avatar seated in a building is its elder.
    pub(super) fn validate_buildings(&self) -> Result<(), String> {
        for (i, b) in self.buildings.iter().enumerate() {
            let Some(kind) = self.building_kinds.get(b.kind as usize) else {
                return Err(format!("building {i} has unknown kind {}", b.kind));
            };
            let name = format!("building {i} ({})", kind.id);
            b.output.validate().map_err(|e| format!("{name} output {e}"))?;
            if b.output.kind().is_some_and(|item| Some(item) != kind.product.or(kind.defence.as_ref().map(|d| d.ammo))) {
                return Err(format!("{name} output holds an item it doesn't make or shoot"));
            }
            if b.output.capacity != kind.store_capacity() || b.output.kinds != kind.store_kinds() {
                return Err(format!("{name} output has capacity {} or kinds {:?}, not its kind's", b.output.capacity, b.output.kinds));
            }
            if kind.defence.is_none() && b.turret != Turret::default() {
                return Err(format!("{name} is a gatherer with a turret's aim"));
            }
            if b.health == 0 || b.health > kind.health {
                return Err(format!("{name} has health {} of {}", b.health, kind.health));
            }
            let building = i as BuildingIx;
            if (b.output.reserved_out.clone(), b.output.reserved_in.clone()) != self.expected_reservations(InventoryRef::Output(building)) {
                return Err(format!("{name} output reservations drifted"));
            }
            let deliveries = self.expected_reservations(InventoryRef::Site(building));
            match &b.stage {
                Stage::Site { part, materials } => {
                    let Some(needs) = kind.parts.get(*part as usize) else {
                        return Err(format!("{name} is a site past its last part"));
                    };
                    materials.validate().map_err(|e| format!("{name} materials {e}"))?;
                    if materials.reserved_out.iter().any(|&u| u > 0) || (materials.reserved_out.clone(), materials.reserved_in.clone()) != deliveries {
                        return Err(format!("{name} materials reservations drifted"));
                    }
                    let need = |item: usize| needs.iter().find(|&&(i, _)| i as usize == item).map_or(0, |&(_, units)| units);
                    if (0..materials.counts.len()).any(|item| materials.counts[item] + materials.reserved_in[item] > need(item)) {
                        return Err(format!("{name} materials hold more than its next part takes"));
                    }
                    if b.slot != Slot::Empty || b.output.total() + b.output.total_in() > 0 {
                        return Err(format!("{name} is a site with someone in its slot or something in its output"));
                    }
                }
                Stage::Built => {
                    if deliveries.0.iter().chain(&deliveries.1).any(|&u| u > 0) {
                        return Err(format!("{name} is built with deliveries still on their way"));
                    }
                }
            }
            let holds = |a: AgentIx| self.agents.get(a as usize).and_then(|a| a.sprite()).map(|sprite| sprite.holds);
            let agrees = match b.slot {
                Slot::Empty => true,
                Slot::Claimed(a) => holds(a) == Some(Some(Hold::Claimed(building))),
                Slot::Occupied(Occupant::Sprite(a)) => holds(a) == Some(Some(Hold::Inside(building))),
                Slot::Occupied(Occupant::Elder(a)) => self.agents.get(a as usize).and_then(|a| a.avatar()).is_some_and(|p| p.seat == Some(super::Seat::Building(building))),
            };
            if !agrees {
                return Err(format!("{name} slot {:?} doesn't agree with its agent", b.slot));
            }
        }
        for (i, a) in self.agents.iter().enumerate() {
            let Some(hold) = a.sprite().and_then(|sprite| sprite.holds) else { continue };
            if !a.body.alive() {
                return Err(format!("sprite {} is dead and still holds {hold:?}", a.id));
            }
            let me = i as AgentIx;
            let (building, slot) = match hold {
                Hold::Claimed(b) => (b, Slot::Claimed(me)),
                Hold::Inside(b) => (b, Slot::Occupied(Occupant::Sprite(me))),
            };
            if self.buildings.get(building as usize).map(|b| b.slot) != Some(slot) {
                return Err(format!("sprite {} holds {hold:?}, which its building's slot doesn't show", a.id));
            }
        }
        for (i, a) in self.agents.iter().enumerate() {
            let Some(building) = a.avatar().and_then(|p| p.seat).and_then(super::Seat::building) else { continue };
            let slot = Slot::Occupied(Occupant::Elder(i as AgentIx));
            if self.buildings.get(building as usize).map(|b| (b.built(), b.slot)) != Some((true, slot)) {
                return Err(format!("avatar {} possesses building {building}, whose slot doesn't show it", a.id));
            }
        }
        Ok(())
    }
}

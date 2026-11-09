//! Miner sprites (M6.5 of the mining design, `SACRED_TREE_MINING.md`, M9):
//! a sprite with the Mine job digs exposed stone and carries it to the
//! tree's stone pile. Its plan has the sprites' shape (`sprites`): one
//! errand at a time, planned again when it is done or breaks. In order:
//!
//! - carrying something: take it to the tree's stone pile;
//! - stone lying on a ground pile near the tree (the elder's digs, its
//!   own, a dropped bundle): the nearest pile, to pick it up;
//! - else the nearest tile, by walk from the tree's stone pile, whose top
//!   layer is stone and that it can dig from a tile beside it within the
//!   M3 rule (`Terrain::dig_refusal`): claim it, walk there, dig it;
//! - else wait beside the tree's stone pile.
//!
//! Fetching piles first means a miner clears the stone its own dig drops,
//! and the elder's too, before it digs again: stone on the ground is worth
//! more than stone in it, and a miner that dug on past its drops would
//! leave piles by every pit.
//!
//! **The search** walks tile middles from the stone pile's tile, four ways,
//! over ground that isn't steep and between obstacles, out to
//! `SEARCH_TILES` from it (the veins are 18 to 37 m out), and keeps the
//! nearest `TARGETS` stone tiles the ground lets it dig. Each check clones
//! the terrain (`dig_refusal`), so the list is cached by terrain revision
//! (`Targets`): a waiting miner plans again every `RETRY_TICKS` and searches
//! only when the ground changed. What stands on a tile (a pile, a dock)
//! and other miners' claims change without the ground, so they are checked
//! on every plan, against the cached list. The walk is a stand-in for the
//! route: a goal nav finds no route to is given up until the ground changes
//! (`SpriteState::unreachable`).
//!
//! **In a volume** (the town's, M8.4 of the depths design, T3) a miner
//! mines walls: a level dig of the [`HEADROOM`] cells of the tile beside
//! the one it stands on, from its floor layer up (`Terrain::volume_dig`,
//! `DigDirection::Level`, `Step::DigWall`), never down or up, never under
//! itself. A wall is a face: a terrace step or a pit's side on the
//! surface, a tunnel's or the mountain cave's sides underground; flat open
//! ground has none, so with no wall worth digging a miner waits by the
//! stone pile as it does with no stone. The search walks floors in 3D
//! (`wall_search`): from the stone pile's top floor, four ways, to a
//! neighbour's floor within a layer of its own (`Volume::floor_near`, so a
//! step up or down a layer walks and a cliff is a wall), between
//! obstacles, out to `VOLUME_SEARCH_TILES` and at most `WALL_SEARCH_NODES`
//! floors (M8.1's idea: a node budget, deterministic). Beside every floor
//! it reaches it reads the wall's cells (`wall_worth`): their ore is its
//! worth (`STONE_WORTH`, `AMBER_WORTH`), plain earth none, and a wall worth
//! nothing is no target. It keeps the `TARGETS` best by walk less worth
//! (`TILES_PER_WORTH`): the nearest, as on a heightfield, a richer wall
//! winning a near tie, and amber counting for four tiles of stone. The
//! dig's stone and amber land on piles on the miner's floor
//! (`PileLab::mined_at`), which miners carry up to the tree: stone to the
//! stone pile, amber to the amber pile.
//!
//! **Light (M8.7, L3):** underground a miner walks and works only on lit
//! floors (open sky is lit), so the wall search walks lit floors only: a
//! wall is a target only from a lit stand that a lit way joins to the
//! stone pile. The floors it walked are the miner's *reach*
//! (`Targets::reaches`); a miner standing off it (in the dark, or in a lit
//! pool a dark way cuts off, where a lantern errand left it) is stranded
//! and walks back to the nearest floor in reach on its level, or home
//! (`retreat`, `sprites`). A pile is fetched only from a lit spot. The
//! search is cached by the terrain's revision and the light grid's.
//! Lantern errands are exempt (L3a of the depths design): a miner may walk
//! dark floors to put up a lantern, never to mine.
//!
//! [`HEADROOM`]: crate::volume::HEADROOM

use std::collections::{BTreeSet, VecDeque};

use content::character::SpriteMineDef;
use fx::{Fx, FxVec2};

use super::action::View;
use super::light::LightGrid;
use super::sprites::{SpriteState, Step};
use super::{AgentIx, Body, BuildingIx, Carry, DepotIx, ItemIx, Kinds, Settlement};
use crate::mine::{Veins, AMBER, AMBER_ID, STONE, STONE_ID};
use crate::nav::distance;
use crate::terrain::{Terrain, LAYER};
use crate::volume::{Volume, HEADROOM};

/// Route requests that found no route before a miner gives its goal up.
pub(super) const GIVE_UP_ASKS: u8 = 2;
/// How close to a pile a miner walking to it is there: within the dock's
/// reach (`avatar::DOCK_REACH`), so someone standing on the spot doesn't
/// hold it up.
pub(super) const REACH: Fx = Fx::from_ratio(3, 4);
/// How far, in tiles along x or y, the search walks from the stone pile.
pub const SEARCH_TILES: i32 = 48;
/// The stone tiles a search keeps, nearest first.
pub const TARGETS: usize = 8;
/// How far, in tiles along x or y, the wall search walks from the stone
/// pile in a volume: past the mountain cave's back wall (z -54, 56 tiles
/// north of the pile).
pub const VOLUME_SEARCH_TILES: i32 = 64;
/// The most floors a wall search visits: the whole box round the stone
/// pile ((2 * 64 + 1)² surface tiles), with room for tunnels and caves
/// under it. A bound in nodes, as `nav::PATH_BUDGET`, so it is the same on
/// every machine.
pub const WALL_SEARCH_NODES: usize = 24576;
/// A wall's worth per stone cell.
pub const STONE_WORTH: u32 = 1;
/// A wall's worth per amber cell: amber is rare, and lanterns need it.
pub const AMBER_WORTH: u32 = 4;
/// The tiles of walk a unit of a wall's worth makes up for: a wall's
/// score is its walk less this times its worth. Distance comes first (a
/// miner carries 5 units a trip, less than a wall of stone yields, so a
/// richer wall far off brings no more stone an hour), and ore breaks
/// near ties: a stone cell is worth 2 tiles, an amber cell 8.
pub const TILES_PER_WORTH: u32 = 2;
/// How far, in tiles along x or y, a stranded miner looks for a floor in
/// its reach on its level to walk back to (`retreat`); past it, home.
pub const RETREAT_TILES: i32 = 12;
/// How far beside the stone pile a miner with nothing to do waits.
const WAIT_GAP: Fx = Fx::from_ratio(6, 5);
/// How close to its waiting spot counts as there.
const AT_WAIT: Fx = Fx::from_ratio(1, 5);

/// What a sprite's brain reads beyond the `View`: the dig claims as of
/// last tick, the town's stone and its stone pile, its amber and amber
/// pile, and how the sprite mines.
#[derive(Clone, Copy)]
pub(super) struct Town<'a> {
    pub claims: &'a [((i32, i32), AgentIx)],
    pub stone: Option<(ItemIx, DepotIx)>,
    pub amber: Option<(ItemIx, DepotIx)>,
    pub mine: Option<SpriteMineDef>,
    /// How the enemies about stand to the sprite (`sprites::Alarm`).
    pub alarm: super::sprites::Alarm,
    /// The lantern sites' errands miners are on, as of last tick
    /// (`Errand`), so two don't fetch for the same need.
    pub errands: &'a [Errand],
}

/// A miner's errand for a lantern's site (M8.6): the site, and the item it
/// fetches or carries there, or `None` for an install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Errand {
    pub agent: AgentIx,
    pub site: BuildingIx,
    pub item: Option<ItemIx>,
}

/// The errand `sprite`'s plan is on from its current step, if any: a
/// fetch's site and its depot's item, a delivery's site and what it
/// carries, or an install's site.
pub(super) fn errand(agent: AgentIx, sprite: &SpriteState, carry: &Carry, depots: &[super::DepotState]) -> Option<Errand> {
    sprite.plan.iter().skip(sprite.step as usize).find_map(|step| match *step {
        Step::Fetch { depot, site } => {
            let item = depots.get(depot as usize).and_then(|d| match d.inventory.kinds {
                Kinds::Only(item) => Some(item),
                _ => None,
            });
            Some(Errand { agent, site, item })
        }
        Step::Supply(site) => Some(Errand { agent, site, item: carry.inventory.kind() }),
        Step::Install { site, .. } => Some(Errand { agent, site, item: None }),
        _ => None,
    })
}

/// The one item depot `depot` takes, if it takes only one (a tree pile).
pub(super) fn depot_item(view: &View, depot: DepotIx) -> Option<ItemIx> {
    match view.depots.get(depot as usize)?.inventory.kinds {
        Kinds::Only(item) => Some(item),
        _ => None,
    }
}

/// Units of `item` lantern site `site` still wants (`BuildingState::wanted`):
/// 0 for anything but a lantern's site.
pub(super) fn site_wants(view: &View, site: BuildingIx, item: ItemIx) -> u32 {
    view.buildings.get(site as usize).map_or(0, |b| {
        let kind = &view.building_kinds[b.kind as usize];
        if kind.is_lantern() { b.wanted(kind, item) } else { 0 }
    })
}

/// A lantern site's errand for a miner (M8.6 of the depths design, L1):
/// miners are the town's carriers, so they put lanterns up. For the
/// nearest lantern site (a tie to the lower index) that it hasn't given
/// up: carrying an item it wants, take it there; its part all there,
/// install it; else fetch an item it still wants from the tree's pile of
/// it, and take it there. A need another miner is already on (`errands`)
/// is left to it. Sites of other kinds stay the elder's (D4).
fn lantern_errand(body: &Body, carry: &Carry, view: &View, town: &Town, gave_up: &dyn Fn(FxVec2) -> bool) -> Option<Vec<Step>> {
    let me = view.me;
    let taken = |site: BuildingIx, item: Option<ItemIx>| town.errands.iter().any(|e| e.agent != me && e.site == site && e.item == item);
    let mut sites: Vec<(Fx, BuildingIx)> = view
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| b.site().is_some() && view.building_kinds[b.kind as usize].is_lantern() && !gave_up(b.stand_position()))
        .map(|(i, b)| (distance(body.position, b.position), i as BuildingIx))
        .collect();
    sites.sort();
    for (_, site) in sites {
        let b = &view.buildings[site as usize];
        let kind = &view.building_kinds[b.kind as usize];
        let stand = b.stand_position();
        if carry.carrying() {
            match carry.inventory.kind() {
                Some(item) if b.wanted(kind, item) > 0 && !taken(site, Some(item)) => return Some(vec![Step::GoTo(stand), Step::Supply(site)]),
                _ => continue,
            }
        }
        if let Some((part, _)) = b.site().filter(|_| b.ready(kind) && !taken(site, None)) {
            return Some(vec![Step::GoTo(stand), Step::Install { site, part }]);
        }
        for (item, _) in b.needs(kind) {
            if b.wanted(kind, item) == 0 || taken(site, Some(item)) {
                continue;
            }
            let depot = (0..view.depots.len() as DepotIx).find(|&d| depot_item(view, d) == Some(item) && view.depots[d as usize].inventory.available(item) > 0);
            if let Some(depot) = depot.filter(|&d| !gave_up(view.depots[d as usize].position)) {
                return Some(vec![Step::GoTo(view.depots[depot as usize].position), Step::Fetch { depot, site }, Step::GoTo(stand), Step::Supply(site)]);
            }
        }
    }
    None
}

/// A stone tile to dig, and the tile to dig it from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub tile: (i32, i32),
    pub stand: (i32, i32),
    /// The pit's depth when found: the dig takes out the layer under it.
    pub depth: u8,
    /// In a volume, the wall's bottom layer: the floor the miner stands on
    /// at `stand`, where the dig's [`HEADROOM`] cells start
    /// (`wall_search`).
    pub wall: Option<i32>,
    /// The wall's ore, by `STONE_WORTH` and `AMBER_WORTH` (a stone tile on
    /// a heightfield: 1).
    pub worth: u32,
    /// The tiles walked from the stone pile to `stand`.
    pub walk: u32,
}

/// The search's result for one terrain revision and light grid revision
/// (`search`): the targets, and in a volume the floors the search walked
/// (the miner's reach). Not state: a fresh copy (a loaded save) searches
/// the same ground again and finds the same, so any two compare equal.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    revision: Option<(u64, u64)>,
    list: Vec<Target>,
    reach: Option<BTreeSet<((i32, i32), i32)>>,
}

impl PartialEq for Targets {
    fn eq(&self, _: &Targets) -> bool {
        true
    }
}

impl Eq for Targets {}

impl Targets {
    /// The stone tiles to dig on `terrain`, searching again only when its
    /// revision or the light's changed.
    pub fn get(&mut self, terrain: &Terrain, light: &LightGrid, origin: (i32, i32), radius: Fx) -> &[Target] {
        self.refresh(terrain, light, origin, radius);
        &self.list
    }

    fn refresh(&mut self, terrain: &Terrain, light: &LightGrid, origin: (i32, i32), radius: Fx) {
        let revision = Some((terrain.revision, light.revision));
        if self.revision != revision {
            (self.list, self.reach) = searched(terrain, light, origin, radius);
            self.revision = revision;
        }
    }

    /// Whether the floor within a layer of `layer` of `tile` is in the reach of a search
    /// from `origin` (a volume's wall search walked it: lit, and joined to
    /// the stone pile by lit floors). `None` with no wall search (no volume,
    /// or no stone veins).
    pub fn reaches(&mut self, terrain: &Terrain, light: &LightGrid, origin: (i32, i32), radius: Fx, tile: (i32, i32), layer: i32) -> Option<bool> {
        self.refresh(terrain, light, origin, radius);
        self.reach.as_ref().map(|reach| (layer - 1..=layer + 1).any(|l| reach.contains(&(tile, l))))
    }

    /// Where a stranded miner on `tile`'s floor at `layer` walks back to
    /// (see the module doc): the middle of the nearest tile, within
    /// `RETREAT_TILES`, with a floor in reach within a layer of its own (the
    /// lower tile on a tie), skipping tiles in `skip`. As of the last
    /// search (`reaches` refreshes it).
    pub fn retreat(&self, tile: (i32, i32), layer: i32, skip: &[(i32, i32)]) -> Option<FxVec2> {
        let reach = self.reach.as_ref()?;
        let mut best: Option<(i32, (i32, i32))> = None;
        for dy in -RETREAT_TILES..=RETREAT_TILES {
            for dx in -RETREAT_TILES..=RETREAT_TILES {
                let t = (tile.0 + dx, tile.1 + dy);
                let key = (dx * dx + dy * dy, t);
                if best.is_some_and(|b| b <= key) || skip.contains(&t) {
                    continue;
                }
                if (layer - 1..=layer + 1).any(|l| reach.contains(&(t, l))) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, t)| Terrain::pit_center(t))
    }
}

/// The nearest stone tiles, by a walk from `origin` (see the module doc),
/// that a body of `radius` may dig from a tile beside them: at most
/// `TARGETS`, nearest first, in a fixed order on ties. In a volume, the
/// best walls instead (`wall_search`).
pub fn search(terrain: &Terrain, light: &LightGrid, origin: (i32, i32), radius: Fx) -> Vec<Target> {
    searched(terrain, light, origin, radius).0
}

/// [`search`], and in a volume the floors its wall search walked (the
/// reach, `Targets::reaches`).
#[allow(clippy::type_complexity)]
fn searched(terrain: &Terrain, light: &LightGrid, origin: (i32, i32), radius: Fx) -> (Vec<Target>, Option<BTreeSet<((i32, i32), i32)>>) {
    let Some(mine) = terrain.mine().filter(|m| m.spec.veins == Veins::Stone) else { return (Vec::new(), None) };
    if let Some(volume) = &terrain.volume {
        let (walls, reach) = wall_search(terrain, volume, light, origin, radius);
        return (walls, Some(reach));
    }
    let ground = |t: (i32, i32)| terrain.inside(Terrain::pit_center(t)) && terrain.tile_height(t).is_some();
    let mut seen = BTreeSet::from([origin]);
    let mut open = VecDeque::from([(origin, 0)]);
    let mut targets: Vec<Target> = Vec::new();
    while let Some((t, walk)) = open.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (t.0 + dx, t.1 + dy);
            if (n.0 - origin.0).abs() > SEARCH_TILES || (n.1 - origin.1).abs() > SEARCH_TILES || !ground(n) {
                continue;
            }
            if mine.ore(n, terrain.pit(n)) == Some(STONE) && !targets.iter().any(|x| x.tile == n) && terrain.dig_refusal(t, n, radius).is_none() {
                targets.push(Target { tile: n, stand: t, depth: terrain.dug(n), wall: None, worth: 1, walk });
                if targets.len() >= TARGETS {
                    return (targets, None);
                }
            }
            let (a, b) = (Terrain::pit_center(t), Terrain::pit_center(n));
            if !seen.contains(&n) && !terrain.steep(n) && terrain.clear(a, b, radius) {
                seen.insert(n);
                open.push_back((n, walk + 1));
            }
        }
    }
    (targets, None)
}

/// The worth of the wall a miner standing on a floor at layer `floor`
/// digs level in `tile` (see the module doc): its ore cells among the
/// [`HEADROOM`] from `floor` up, if `Terrain::volume_dig`'s rules for the
/// tile let a level dig take them (diggable, in no undiggable range,
/// something left under them). Nothing for a wall of plain earth, or no
/// wall (every cell empty: worth nothing).
pub fn wall_worth(terrain: &Terrain, volume: &Volume, tile: (i32, i32), floor: i32) -> Option<u32> {
    if !terrain.diggable || !terrain.inside(Terrain::pit_center(tile)) || terrain.is_undiggable(tile) {
        return None;
    }
    if volume.spans(tile).first().is_none_or(|s| s.bottom >= floor) {
        return None;
    }
    let worth = (floor..floor + HEADROOM)
        .filter(|&layer| volume.is_solid(tile, layer))
        .map(|layer| match volume.ore(tile, layer) {
            Some(STONE) => STONE_WORTH,
            Some(AMBER) => AMBER_WORTH,
            _ => 0,
        })
        .sum();
    (worth > 0).then_some(worth)
}

/// Whether any of the [`HEADROOM`] cells of `tile` from layer `bottom` up
/// is still solid: a wall dig with cells left (`Step::DigWall`).
pub fn wall_left(terrain: &Terrain, tile: (i32, i32), bottom: i32) -> bool {
    terrain.volume.as_ref().is_some_and(|volume| (bottom..bottom + HEADROOM).any(|layer| volume.is_solid(tile, layer)))
}

/// The best walls to mine in a volume, by a walk over lit floors from
/// `origin`'s top floor (see the module doc): at most `TARGETS`, best
/// first (`better`). Each tile is a target once, from the floor and stand
/// it is worth the most from. With them, every floor it walked: the reach.
#[allow(clippy::type_complexity)]
fn wall_search(terrain: &Terrain, volume: &Volume, light: &LightGrid, origin: (i32, i32), radius: Fx) -> (Vec<Target>, BTreeSet<((i32, i32), i32)>) {
    let Some(start) = volume.top(origin) else { return (Vec::new(), BTreeSet::new()) };
    let mut seen = BTreeSet::from([(origin, start)]);
    let mut open = VecDeque::from([(origin, start, 0u32)]);
    let mut walls: std::collections::BTreeMap<(i32, i32), Target> = std::collections::BTreeMap::new();
    let mut visited = 0;
    while let Some((t, floor, walk)) = open.pop_front() {
        visited += 1;
        if visited > WALL_SEARCH_NODES {
            break;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (t.0 + dx, t.1 + dy);
            if (n.0 - origin.0).abs() > VOLUME_SEARCH_TILES || (n.1 - origin.1).abs() > VOLUME_SEARCH_TILES || !terrain.inside(Terrain::pit_center(n)) {
                continue;
            }
            if let Some(worth) = wall_worth(terrain, volume, n, floor) {
                let wall = Target { tile: n, stand: t, depth: 0, wall: Some(floor), worth, walk };
                if walls.get(&n).is_none_or(|kept| better(&wall, kept)) {
                    walls.insert(n, wall);
                }
            }
            let Some(next) = volume.floor_near(n, floor) else { continue };
            // A miner walks lit floors only (L3).
            if !seen.contains(&(n, next)) && light.sprite_at(terrain, n, next) && terrain.clear(Terrain::pit_center(t), Terrain::pit_center(n), radius) {
                seen.insert((n, next));
                open.push_back((n, next, walk + 1));
            }
        }
    }
    let mut walls: Vec<Target> = walls.into_values().collect();
    walls.sort_by(|a, b| match (better(a, b), better(b, a)) {
        (true, _) => std::cmp::Ordering::Less,
        (_, true) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    walls.truncate(TARGETS);
    (walls, seen)
}

/// Whether wall `a` scores better than `b`: a shorter walk less its worth
/// (`TILES_PER_WORTH`), then a shorter walk, a lower tile and stand: a
/// total order, so the search's result is fixed.
fn better(a: &Target, b: &Target) -> bool {
    let score = |t: &Target| t.walk as i64 - (TILES_PER_WORTH * t.worth) as i64;
    (score(a), a.walk, a.tile, a.stand) < (score(b), b.walk, b.tile, b.stand)
}

/// What a sprite carrying something does first, whatever its job: take it
/// to the tree's pile for it (`depot_for`). Nothing when it carries
/// nothing, or there is no stone pile.
pub(super) fn deliver(carry: &Carry, view: &View, town: &Town) -> Vec<Step> {
    match depot_for(carry.inventory.kind(), town) {
        Some(depot) if carry.carrying() => vec![Step::GoTo(view.depots[depot as usize].position), Step::Deposit(depot)],
        _ => Vec::new(),
    }
}

/// The tree's pile that takes `item`: the amber pile for amber (M8.4),
/// else the stone pile, if the town has one.
fn depot_for(item: Option<ItemIx>, town: &Town) -> Option<DepotIx> {
    let stone = town.stone?.1;
    Some(town.amber.filter(|&(amber, _)| item == Some(amber)).map_or(stone, |(_, depot)| depot))
}

/// A miner's next errand (see the module doc). With no stone pile in the
/// scene, it goes home and stands there, as an idle sprite does.
pub(super) fn plan(sprite: &mut SpriteState, body: &Body, carry: &Carry, view: &View, town: &Town) -> Vec<Step> {
    let Some((stone, depot)) = town.stone else { return vec![Step::GoTo(body.home)] };
    let amber = town.amber.map(|(amber, _)| amber);
    if sprite.unreachable_at != view.terrain.revision {
        sprite.unreachable.clear();
        sprite.unreachable_at = view.terrain.revision;
    }
    let unreachable = sprite.unreachable.clone();
    let gave_up = |p: FxVec2| unreachable.contains(&Terrain::pit_tile(p));
    let dock = view.depots[depot as usize].position;
    let room_for = |item: Option<ItemIx>| depot_for(item, town).is_some_and(|d| view.depots[d as usize].inventory.free() > 0);
    let room = room_for(Some(stone));
    // A lantern's site first (M8.6): what it carries, if the site wants
    // it, its install, or a fetch from the tree's piles for it.
    if carry.inventory.capacity > 0 {
        if let Some(steps) = lantern_errand(body, carry, view, town, &gave_up) {
            return steps;
        }
    }
    if carry.carrying() {
        let to = depot_for(carry.inventory.kind(), town).map_or(dock, |d| view.depots[d as usize].position);
        if room_for(carry.inventory.kind()) && !gave_up(to) {
            return deliver(carry, view, town);
        }
        return wait(body, view, depot);
    }
    if carry.inventory.capacity == 0 || !(room || amber.is_some_and(|a| room_for(Some(a)))) {
        return wait(body, view, depot);
    }
    // Stone (and amber, in a town's volume) on the ground near the tree,
    // nearest the miner first.
    let tree = view.terrain.center;
    let reach = Fx::from_int(if view.terrain.volume.is_some() { VOLUME_SEARCH_TILES } else { SEARCH_TILES });
    let fetch = |item: Option<ItemIx>| item.is_some_and(|i| i == stone || Some(i) == amber) && room_for(item);
    let pile = view
        .piles
        .into_iter()
        .flat_map(|lab| lab.piles.iter())
        .filter(|(_, p)| p.source.is_none() && fetch(p.inventory.kind()) && p.inventory.kind().is_some_and(|i| p.inventory.available(i) > 0))
        .filter(|(_, p)| (p.position.x - tree.x).abs() <= reach && (p.position.y - tree.y).abs() <= reach)
        .filter_map(|(&id, p)| {
            let spot = stand_spot(view, p, body.position, dock, body.base_radius).filter(|&spot| !gave_up(spot))?;
            Some((distance(body.position, p.position), id, spot))
        })
        .min_by_key(|&(d, id, _)| (d, id));
    if let Some((_, id, spot)) = pile {
        return vec![Step::GoTo(spot), Step::Pickup(id)];
    }
    // Stone in the ground: the nearest tile (in a volume, the best wall)
    // no one else claimed and nothing stands on.
    if town.mine.is_some() && room {
        let me = view.me;
        let targets = sprite.targets.get(view.terrain, view.light, Terrain::pit_tile(dock), body.base_radius);
        let target = targets.iter().find(|t| {
            !town.claims.iter().any(|&(tile, a)| tile == t.tile && a != me) && view.dig_blocked(t.tile).is_none() && !unreachable.contains(&t.stand)
        });
        if let Some(t) = target {
            let dig = match t.wall {
                Some(layer) => Step::DigWall { tile: t.tile, layer },
                None => Step::Dig { tile: t.tile, depth: t.depth },
            };
            return vec![Step::ClaimTile(t.tile), Step::GoTo(Terrain::pit_center(t.stand)), dig];
        }
    }
    wait(body, view, depot)
}

/// Where a miner at `from` stands to pick up the pile at `pile`: as an
/// elder does (`interact::stand_spot`), on the side it comes from, or else
/// the tree's side or along x or y, the first on ground it can walk on (a
/// pile beside a deep pit may have a pit or steep ground on one side). In
/// a volume, where a body of `radius` stands on a floor within a layer of
/// the pile's (M8.4: a pile in a tunnel or the cave is picked up from its
/// floor, not from the mountain's top over it, nor from against a wall).
fn stand_spot(view: &View, pile: &super::piles::Pile, from: FxVec2, dock: FxVec2, radius: Fx) -> Option<FxVec2> {
    let terrain = view.terrain;
    let (pile, feet) = (pile.position, pile.height);
    let gap = super::piles::DROP_GAP;
    let sides = [(from - pile).normalize_or_zero(), (dock - pile).normalize_or_zero(), FxVec2::new(Fx::ONE, Fx::ZERO), FxVec2::new(-Fx::ONE, Fx::ZERO), FxVec2::new(Fx::ZERO, Fx::ONE), FxVec2::new(Fx::ZERO, -Fx::ONE)];
    sides.into_iter().filter(|&side| side != FxVec2::ZERO).map(|side| pile + side * gap).find(|&spot| {
        let tile = Terrain::pit_tile(spot);
        let ground = match &terrain.volume {
            // Lit (L3, M8.7): a miner picks a pile up in light only.
            Some(_) => terrain.footprint_at(spot, feet, radius).is_some_and(|h| (h - feet).abs() <= LAYER && view.light.sprite_lit(terrain, spot, h)),
            None => !terrain.steep(tile) && terrain.dug(tile) <= terrain.dug(Terrain::pit_tile(pile)) + 1,
        };
        terrain.inside(spot) && ground
    })
}

/// Nothing to do: walk to a spot beside the tree's stone pile, away from
/// the tree (one per sprite, so waiting miners don't stand on each other),
/// and wait there.
fn wait(body: &Body, view: &View, depot: DepotIx) -> Vec<Step> {
    let dock = view.depots[depot as usize].position;
    let away = (dock - view.terrain.center).normalize_or_zero();
    let away = if away == FxVec2::ZERO { FxVec2::new(Fx::ZERO, Fx::ONE) } else { away };
    let (sin, cos) = Fx::from_ratio(view.me as i64 % 5 - 2, 2).sin_cos();
    let spot = dock + FxVec2::new(away.x * cos - away.y * sin, away.x * sin + away.y * cos) * WAIT_GAP;
    if distance(body.position, spot) <= AT_WAIT {
        vec![Step::Wait]
    } else {
        vec![Step::GoTo(spot)]
    }
}

impl Settlement {
    /// The town's stone and the tree's stone pile (the depot that takes
    /// only stone), if it has them: where miners bring their stone.
    pub fn stone_depot(&self) -> Option<(ItemIx, DepotIx)> {
        let stone = self.item_index(STONE_ID)?;
        let depot = self.depots.iter().position(|d| d.inventory.kinds == Kinds::Only(stone))?;
        Some((stone, depot as DepotIx))
    }

    /// The town's amber and the tree's amber pile (the depot that takes
    /// only amber), if it has them: where miners bring their amber (M8.4).
    pub fn amber_depot(&self) -> Option<(ItemIx, DepotIx)> {
        let amber = self.item_index(AMBER_ID)?;
        let depot = self.depots.iter().position(|d| d.inventory.kinds == Kinds::Only(amber))?;
        Some((amber, depot as DepotIx))
    }
}

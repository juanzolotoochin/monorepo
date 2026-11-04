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

use std::collections::{BTreeSet, VecDeque};

use content::character::SpriteMineDef;
use fx::{Fx, FxVec2};

use super::action::View;
use super::sprites::{SpriteState, Step};
use super::{AgentIx, Body, Carry, DepotIx, ItemIx, Kinds, Settlement};
use crate::mine::{Veins, STONE_ID};
use crate::nav::distance;
use crate::terrain::Terrain;

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
/// How far beside the stone pile a miner with nothing to do waits.
const WAIT_GAP: Fx = Fx::from_ratio(6, 5);
/// How close to its waiting spot counts as there.
const AT_WAIT: Fx = Fx::from_ratio(1, 5);

/// What a sprite's brain reads beyond the `View`: the dig claims as of
/// last tick, the town's stone and its stone pile, and how the sprite mines.
#[derive(Clone, Copy)]
pub(super) struct Town<'a> {
    pub claims: &'a [((i32, i32), AgentIx)],
    pub stone: Option<(ItemIx, DepotIx)>,
    pub mine: Option<SpriteMineDef>,
    /// How the enemies about stand to the sprite (`sprites::Alarm`).
    pub alarm: super::sprites::Alarm,
}

/// A stone tile to dig, and the tile to dig it from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub tile: (i32, i32),
    pub stand: (i32, i32),
    /// The pit's depth when found: the dig takes out the layer under it.
    pub depth: u8,
}

/// The search's result for one terrain revision (`search`). Not state: a
/// fresh copy (a loaded save) searches the same ground again and finds the
/// same, so any two compare equal.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    revision: Option<u64>,
    list: Vec<Target>,
}

impl PartialEq for Targets {
    fn eq(&self, _: &Targets) -> bool {
        true
    }
}

impl Eq for Targets {}

impl Targets {
    /// The stone tiles to dig on `terrain`, searching again only when its
    /// revision changed.
    pub fn get(&mut self, terrain: &Terrain, origin: (i32, i32), radius: Fx) -> &[Target] {
        if self.revision != Some(terrain.revision) {
            self.list = search(terrain, origin, radius);
            self.revision = Some(terrain.revision);
        }
        &self.list
    }
}

/// The nearest stone tiles, by a walk from `origin` (see the module doc),
/// that a body of `radius` may dig from a tile beside them: at most
/// `TARGETS`, nearest first, in a fixed order on ties.
pub fn search(terrain: &Terrain, origin: (i32, i32), radius: Fx) -> Vec<Target> {
    let Some(mine) = terrain.mine().filter(|m| m.spec.veins == Veins::Stone) else { return Vec::new() };
    let ground = |t: (i32, i32)| terrain.inside(Terrain::pit_center(t)) && terrain.tile_height(t).is_some();
    let mut seen = BTreeSet::from([origin]);
    let mut open = VecDeque::from([origin]);
    let mut targets: Vec<Target> = Vec::new();
    while let Some(t) = open.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (t.0 + dx, t.1 + dy);
            if (n.0 - origin.0).abs() > SEARCH_TILES || (n.1 - origin.1).abs() > SEARCH_TILES || !ground(n) {
                continue;
            }
            let stone = mine.ore(n, terrain.pit(n)).is_some();
            if stone && !targets.iter().any(|x| x.tile == n) && terrain.dig_refusal(t, n, radius).is_none() {
                targets.push(Target { tile: n, stand: t, depth: terrain.pit(n) });
                if targets.len() >= TARGETS {
                    return targets;
                }
            }
            if !seen.contains(&n) && !terrain.steep(n) && terrain.clear(Terrain::pit_center(t), Terrain::pit_center(n), radius) {
                seen.insert(n);
                open.push_back(n);
            }
        }
    }
    targets
}

/// What a sprite carrying something does first, whatever its job: take it
/// to the tree's stone pile. Nothing when it carries nothing, or there is
/// no stone pile.
pub(super) fn deliver(carry: &Carry, view: &View, town: &Town) -> Vec<Step> {
    match town.stone {
        Some((_, depot)) if carry.carrying() => vec![Step::GoTo(view.depots[depot as usize].position), Step::Deposit(depot)],
        _ => Vec::new(),
    }
}

/// A miner's next errand (see the module doc). With no stone pile in the
/// scene, it goes home and stands there, as an idle sprite does.
pub(super) fn plan(sprite: &mut SpriteState, body: &Body, carry: &Carry, view: &View, town: &Town) -> Vec<Step> {
    let Some((stone, depot)) = town.stone else { return vec![Step::GoTo(body.home)] };
    if sprite.unreachable_at != view.terrain.revision {
        sprite.unreachable.clear();
        sprite.unreachable_at = view.terrain.revision;
    }
    let unreachable = sprite.unreachable.clone();
    let gave_up = |p: FxVec2| unreachable.contains(&Terrain::pit_tile(p));
    let dock = view.depots[depot as usize].position;
    let room = view.depots[depot as usize].inventory.free() > 0;
    if carry.carrying() {
        if room && !gave_up(dock) {
            return deliver(carry, view, town);
        }
        return wait(body, view, depot);
    }
    if !room || carry.inventory.capacity == 0 {
        return wait(body, view, depot);
    }
    // Stone on the ground near the tree, nearest the miner first.
    let tree = view.terrain.center;
    let reach = Fx::from_int(SEARCH_TILES);
    let pile = view
        .piles
        .into_iter()
        .flat_map(|lab| lab.piles.iter())
        .filter(|(_, p)| p.source.is_none() && p.inventory.kind() == Some(stone) && p.inventory.available(stone) > 0)
        .filter(|(_, p)| (p.position.x - tree.x).abs() <= reach && (p.position.y - tree.y).abs() <= reach)
        .filter_map(|(&id, p)| {
            let spot = stand_spot(view, p.position, body.position, dock).filter(|&spot| !gave_up(spot))?;
            Some((distance(body.position, p.position), id, spot))
        })
        .min_by_key(|&(d, id, _)| (d, id));
    if let Some((_, id, spot)) = pile {
        return vec![Step::GoTo(spot), Step::Pickup(id)];
    }
    // Stone in the ground: the nearest tile no one else claimed and
    // nothing stands on.
    if town.mine.is_some() {
        let me = view.me;
        let targets = sprite.targets.get(view.terrain, Terrain::pit_tile(dock), body.base_radius);
        let target = targets.iter().find(|t| {
            !town.claims.iter().any(|&(tile, a)| tile == t.tile && a != me) && view.dig_blocked(t.tile).is_none() && !unreachable.contains(&t.stand)
        });
        if let Some(t) = target {
            return vec![Step::ClaimTile(t.tile), Step::GoTo(Terrain::pit_center(t.stand)), Step::Dig { tile: t.tile, depth: t.depth }];
        }
    }
    wait(body, view, depot)
}

/// Where a miner at `from` stands to pick up the pile at `pile`: as an
/// elder does (`interact::stand_spot`), on the side it comes from, or else
/// the tree's side or along x or y, the first on ground it can walk on (a
/// pile beside a deep pit may have a pit or steep ground on one side).
fn stand_spot(view: &View, pile: FxVec2, from: FxVec2, dock: FxVec2) -> Option<FxVec2> {
    let terrain = view.terrain;
    let gap = super::piles::DROP_GAP;
    let sides = [(from - pile).normalize_or_zero(), (dock - pile).normalize_or_zero(), FxVec2::new(Fx::ONE, Fx::ZERO), FxVec2::new(-Fx::ONE, Fx::ZERO), FxVec2::new(Fx::ZERO, Fx::ONE), FxVec2::new(Fx::ZERO, -Fx::ONE)];
    sides.into_iter().filter(|&side| side != FxVec2::ZERO).map(|side| pile + side * gap).find(|&spot| {
        let tile = Terrain::pit_tile(spot);
        terrain.inside(spot) && !terrain.steep(tile) && terrain.pit(tile) <= terrain.pit(Terrain::pit_tile(pile)) + 1
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
}

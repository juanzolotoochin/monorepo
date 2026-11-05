//! Terrain: the arena's walkable ground and what blocks it.
//!
//! Scenes are arenas centered on `center` with half-width `extent`.
//! Movement is blocked by authored obstacles (discs: rocks, trees, machines)
//! and by rock voxels, 1 × 1 tile columns that can be dug out. Walkable
//! surfaces are raised boxes (platforms) that set the ground height. In
//! digging scenes, avatars dig pits into the ground, a layer at a time, and
//! mines generate their ground (terraces and ore) from a seed. Sloped
//! scenes join the tiles' heights into slopes, so pit edges become ramps.
//! Volume scenes keep their ground as solid and empty cells instead
//! ([`Volume`]), so floors can lie over each other.
//! Terrain is part of the saved state; navigation caches built from it are not.

use std::sync::{Arc, OnceLock};

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};
use crate::mine::{Hill, Mine, MineSpec, Veins};
use crate::volume::Volume;

/// Extra separation kept from obstacles, as in the original game (1.5 cm).
pub const CLEARANCE_MARGIN: Fx = Fx::from_bits(64_424_509); // 0.015

/// How much ground one dig removes.
pub const LAYER: Fx = Fx::HALF;
/// The deepest a pit goes, in layers, unless the scene sets its own
/// ([`Terrain::max_layers`]).
pub const MAX_LAYERS: u8 = 3;
/// In sloped scenes, the steepest walkable ground: a layer per tile.
pub const MAX_SLOPE: Fx = LAYER;
/// A search for a walk out of a pit that finds this many tiles stops:
/// that much open ground counts as out ([`Terrain::dig_refusal`]).
pub const WALK_OUT_TILES: usize = 256;

/// Why a dig in front of a digger doesn't start ([`Terrain::dig_refusal`],
/// `View::dig_in_front`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigRefusal {
    /// The scene can't be dug, or not there: outside the arena, a drop, or
    /// an [`Terrain::undiggable`] range.
    Undiggable,
    /// A digger digs the tile in front, never its own.
    OwnTile,
    /// The pit is as deep as the scene's pits go.
    Bottom,
    /// The dig would leave the tile more than a layer below the digger's.
    TooDeep,
    /// The digger's own tile would get too steep to walk on.
    Steepens,
    /// Someone in a tile near the pit would have no walk left to undug
    /// ground.
    Strands,
    /// A building, a site, the tree or a prop stands on the tile.
    Blocked,
    /// A pile, a tree pile's dock or a building's output pile is on it.
    Pile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disc {
    pub center: FxVec2,
    pub radius: Fx,
}

/// An individually removable piece of cover. Its index is stable for the
/// lifetime of the world, including saves and lockstep replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destructible {
    pub asset: u16,
    pub disc: Disc,
    pub health: u16,
    pub max_health: u16,
    pub repairable: bool,
    pub repair_progress: u16,
}

/// A raised walkable box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    pub min: FxVec2,
    pub max: FxVec2,
    pub height: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terrain {
    /// The arena's center in world tiles.
    pub center: FxVec2,
    /// Half-width of the square arena.
    pub extent: Fx,
    pub obstacles: Vec<Disc>,
    #[serde(default)]
    pub destructibles: Vec<Destructible>,
    pub surfaces: Vec<Surface>,
    /// Platform scenes: only the surfaces are ground; elsewhere is a drop.
    pub platforms_only: bool,
    /// Diggable rock columns by integer tile, sorted.
    pub rock: Vec<(i32, i32)>,
    /// Avatars can dig pits here.
    #[serde(default)]
    pub diggable: bool,
    /// Dug pits by [`Terrain::pit_tile`], with how many layers deep, sorted.
    #[serde(default)]
    pub pits: Vec<((i32, i32), u8)>,
    /// The ground slopes between tile corners instead of stepping at tile
    /// edges, and walks fail on slopes over [`MAX_SLOPE`] instead of steps
    /// over 0.25 m (see [`Terrain::ground`], [`Terrain::walkable`]).
    #[serde(default)]
    pub sloped: bool,
    /// The deepest a pit goes, in layers. Set it before
    /// [`Terrain::generate_mine`], which gives the mine as many.
    #[serde(default = "max_layers")]
    pub max_layers: u8,
    /// Tiles no one digs (a town's hill, M6.3): ranges of tiles, each its
    /// first and last tile on both axes.
    #[serde(default)]
    pub undiggable: Vec<((i32, i32), (i32, i32))>,
    /// Generated ground under the whole arena, if this is a mine.
    #[serde(default)]
    pub mine: Option<MineSpec>,
    /// The ground as solid and empty cells, if the scene has a volume (V11
    /// of the verticality design): then it is the ground, its top floor
    /// what [`Terrain::ground`] gives, and surfaces, mines and pits don't
    /// count. Edit it with [`Terrain::fill_cell`] and
    /// [`Terrain::clear_cell`], which bump the revision.
    #[serde(default)]
    pub volume: Option<Volume>,
    #[serde(skip)]
    generated: Generated,
    /// Bumped by every change, so cached paths know to rebuild.
    pub revision: u64,
}

/// The mine generated from [`Terrain::mine`] on first use. Not state: equal
/// to any other, so a fresh copy (say, a loaded save) compares the same.
#[derive(Clone, Default)]
struct Generated(OnceLock<Arc<Mine>>);

impl PartialEq for Generated {
    fn eq(&self, _: &Generated) -> bool {
        true
    }
}

impl Eq for Generated {}

impl std::fmt::Debug for Generated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Generated")
    }
}

impl Terrain {
    pub fn flat(center: FxVec2, extent: Fx) -> Terrain {
        Terrain {
            center,
            extent,
            obstacles: Vec::new(),
            destructibles: Vec::new(),
            surfaces: Vec::new(),
            platforms_only: false,
            rock: Vec::new(),
            diggable: false,
            pits: Vec::new(),
            sloped: false,
            max_layers: MAX_LAYERS,
            undiggable: Vec::new(),
            mine: None,
            volume: None,
            generated: Generated::default(),
            revision: 0,
        }
    }

    pub fn min(&self) -> FxVec2 {
        FxVec2::new(self.center.x - self.extent, self.center.y - self.extent)
    }

    pub fn max(&self) -> FxVec2 {
        FxVec2::new(self.center.x + self.extent, self.center.y + self.extent)
    }

    pub fn inside(&self, p: FxVec2) -> bool {
        let (min, max) = (self.min(), self.max());
        p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y
    }

    /// Ground height at `p`: the highest surface containing it; elsewhere
    /// zero, or nothing (a drop) in platform scenes. Pits sink it. In sloped
    /// scenes, the tile's four corners set it instead ([`Terrain::patch`]).
    /// In volume scenes, the top floor ([`Volume::ground`]).
    pub fn ground(&self, p: FxVec2) -> Option<Fx> {
        if let Some(volume) = &self.volume {
            return volume.ground(p);
        }
        if self.sloped && !self.level() {
            return self.patch(p).map(|patch| patch.height());
        }
        let top = self.top(p)?;
        if self.pits.is_empty() {
            return Some(top);
        }
        Some(top - LAYER.mul_int(self.pit(Self::pit_tile(p)) as i64))
    }

    /// [`Terrain::ground`] before digging.
    fn top(&self, p: FxVec2) -> Option<Fx> {
        let top = self
            .surfaces
            .iter()
            .filter(|s| p.x >= s.min.x && p.x <= s.max.x && p.y >= s.min.y && p.y <= s.max.y)
            .map(|s| s.height)
            .max();
        if self.platforms_only {
            top
        } else {
            let base = || self.mine().and_then(|m| m.height(Self::pit_tile(p))).unwrap_or(Fx::ZERO);
            Some(top.unwrap_or_else(base))
        }
    }

    /// The ground under a body at `p` whose feet are at height `z`: in
    /// volume scenes the floor it stands on ([`Volume::floor`]), which may
    /// be under another; elsewhere [`Terrain::ground`].
    pub fn floor(&self, p: FxVec2, z: Fx) -> Option<Fx> {
        match &self.volume {
            Some(volume) => volume.floor(p, z),
            None => self.ground(p),
        }
    }

    /// Whether the ground is zero everywhere, with no surfaces, drops, mine,
    /// pits or volume: sloped or not, it's the same ground.
    fn level(&self) -> bool {
        self.volume.is_none() && self.pits.is_empty() && self.mine.is_none() && self.surfaces.is_empty() && !self.platforms_only
    }

    /// A tile's height in sloped scenes: [`Terrain::top`] at its center, less
    /// its pit. Surfaces count by the tile's center, so they snap to tiles.
    /// In volume scenes, the column's top floor.
    pub fn tile_height(&self, tile: (i32, i32)) -> Option<Fx> {
        if let Some(volume) = &self.volume {
            return volume.top(tile).map(crate::volume::height);
        }
        let top = self.top(Self::pit_center(tile))?;
        Some(top - LAYER.mul_int(self.pit(tile) as i64))
    }

    /// The sloped ground over `p`'s tile: each corner takes the height of
    /// the lowest of the four tiles touching it (drops aside), so a dug
    /// tile's floor is flat and its neighbours slope down into it. Nothing
    /// if the tile itself is a drop. In volume scenes, the top floor's
    /// ([`Volume::patch`]).
    fn patch(&self, p: FxVec2) -> Option<Patch> {
        if let Some(volume) = &self.volume {
            return volume.top_patch(p);
        }
        let h = self.tile_height(Self::pit_tile(p))?;
        Some(Patch::over(p, h, |tile| self.tile_height(tile)))
    }

    /// The generated mine, if this terrain is one.
    pub fn mine(&self) -> Option<&Mine> {
        let spec = self.mine?;
        Some(self.generated.0.get_or_init(|| Arc::new(Mine::generate(spec))))
    }

    /// Makes the arena a mine generated from `seed`, its landing at `spawn`,
    /// with `veins` in its layers.
    pub fn generate_mine(&mut self, seed: u64, spawn: FxVec2, veins: Veins) {
        let min = Self::pit_tile(self.min());
        let size = (Self::pit_tile(self.max()).0 - min.0 + 1) as u32;
        self.mine = Some(MineSpec { seed, min, size, landing: Self::pit_tile(spawn), layers: self.max_layers, veins, hill: None });
        self.generated = Generated::default();
        self.revision += 1;
    }

    /// Raises `hill` in the middle of the mine, centered on the arena's
    /// center tile (its `center` is set to it), and makes it undiggable.
    pub fn raise_hill(&mut self, hill: Hill) -> Result<(), String> {
        hill.validate()?;
        let Some(spec) = &mut self.mine else { return Err("a hill needs a mine to stand in".into()) };
        let hill = Hill { center: Self::pit_tile(self.center), ..hill };
        spec.hill = Some(hill);
        self.undiggable.extend(hill.ranges());
        self.generated = Generated::default();
        self.revision += 1;
        Ok(())
    }

    /// [`Terrain::ground`], with drops at zero.
    pub fn height(&self, p: FxVec2) -> Fx {
        self.ground(p).unwrap_or(Fx::ZERO)
    }

    /// The highest ground under a body of `radius` at `p` (center and four
    /// sides), or nothing if any of it overhangs a drop.
    pub fn footprint(&self, p: FxVec2, radius: Fx) -> Option<Fx> {
        let offsets = [(Fx::ZERO, Fx::ZERO), (radius, Fx::ZERO), (-radius, Fx::ZERO), (Fx::ZERO, radius), (Fx::ZERO, -radius)];
        offsets.iter().try_fold(Fx::MIN, |top, &(dx, dy)| Some(top.max(self.ground(FxVec2::new(p.x + dx, p.y + dy))?)))
    }

    /// Whether a body can walk from `a` to `b` without a step over 0.25 m or a drop.
    /// In sloped scenes, without ground steeper than [`MAX_SLOPE`] or a
    /// drop under any of [`Terrain::footprint`]'s five points, every 0.1 m
    /// of the way from `a` on. A body that stands on steep ground (a dig
    /// beside it made it so) may still walk off it, as long as it doesn't
    /// climb faster than [`MAX_SLOPE`] while on it: leaving downhill or
    /// along the slope, never back onto steep ground once off it. So no one
    /// is left stuck, and no walk from gentle ground enters steep ground.
    pub fn walkable(&self, a: FxVec2, b: FxVec2, radius: Fx) -> bool {
        let steps = ((b - a).length() / Fx::from_ratio(1, 10)).ceil().floor_to_int().max(1);
        if self.sloped {
            if self.level() {
                return true;
            }
            let offsets = [(Fx::ZERO, Fx::ZERO), (radius, Fx::ZERO), (-radius, Fx::ZERO), (Fx::ZERO, radius), (Fx::ZERO, -radius)];
            // The most a step of the walk may climb while on steep ground.
            let climb = MAX_SLOPE * (b - a).length().div_int(steps);
            let (mut leaving, mut last) = (true, Fx::ZERO);
            for i in 0..=steps {
                let p = a + (b - a) * Fx::from_ratio(i, steps);
                let (mut steep, mut height) = (false, Fx::ZERO);
                for (k, &(dx, dy)) in offsets.iter().enumerate() {
                    let Some(patch) = self.patch(FxVec2::new(p.x + dx, p.y + dy)) else { return false };
                    steep |= patch.steep();
                    if k == 0 {
                        height = patch.height();
                    }
                }
                if steep {
                    if !leaving || (i > 0 && height - last > climb) {
                        return false;
                    }
                } else {
                    leaving = false;
                }
                last = height;
            }
            return true;
        }
        let Some(mut last) = self.footprint(a, radius) else { return false };
        for i in 1..=steps {
            let p = a + (b - a) * Fx::from_ratio(i, steps);
            let Some(next) = self.footprint(p, radius) else { return false };
            if (next - last).abs() > Fx::from_ratio(1, 4) {
                return false;
            }
            last = next;
        }
        true
    }

    /// The five points of [`Terrain::footprint`]: the center and four sides.
    fn footprint_points(p: FxVec2, radius: Fx) -> [FxVec2; 5] {
        [(Fx::ZERO, Fx::ZERO), (radius, Fx::ZERO), (-radius, Fx::ZERO), (Fx::ZERO, radius), (Fx::ZERO, -radius)].map(|(dx, dy)| FxVec2::new(p.x + dx, p.y + dy))
    }

    /// The ground a body of `radius` at `p` whose feet are at height `z`
    /// stands on: in volume scenes the highest of the floors under
    /// [`Terrain::footprint`]'s five points ([`Volume::floor`]), or nothing
    /// if any has none (a wall, or a passage without headroom, beside or
    /// under it); elsewhere [`Terrain::footprint`].
    pub fn footprint_at(&self, p: FxVec2, z: Fx, radius: Fx) -> Option<Fx> {
        let Some(volume) = &self.volume else { return self.footprint(p, radius) };
        Self::footprint_points(p, radius).iter().try_fold(Fx::MIN, |top, &q| Some(top.max(volume.floor(q, z)?)))
    }

    /// The lowest ceiling over a body of `radius` at `p` whose feet are at
    /// `z`: of each of the five points' [`Volume::ceiling_over`]. Nothing
    /// outside volume scenes or under open sky.
    pub fn ceiling_at(&self, p: FxVec2, z: Fx, radius: Fx) -> Option<Fx> {
        let volume = self.volume.as_ref()?;
        Self::footprint_points(p, radius).iter().filter_map(|&q| volume.ceiling_over(q, z)).min()
    }

    /// Whether a body of `radius` on foot, its feet at `z`, can walk from
    /// `a` to `b`. In volume scenes (V3 of the verticality design), every
    /// 0.1 m of the way each of its five points needs a floor
    /// ([`Terrain::footprint_at`]): at most a layer ([`STEP_UP`]) over its
    /// feet, so a step two layers up is a wall, and with headroom. Its feet
    /// follow the floor up and down ramps; past a drop of more than a layer
    /// they stay where they were, for the body to fall. Elsewhere
    /// [`Terrain::walkable`], which ignores `z`.
    ///
    /// [`STEP_UP`]: crate::volume::STEP_UP
    pub fn walkable_at(&self, a: FxVec2, b: FxVec2, z: Fx, radius: Fx) -> bool {
        if self.volume.is_none() {
            return self.walkable(a, b, radius);
        }
        let steps = ((b - a).length() / Fx::from_ratio(1, 10)).ceil().floor_to_int().max(1);
        let mut z = z;
        for i in 1..=steps {
            let p = a + (b - a) * Fx::from_ratio(i, steps);
            let Some(floor) = self.footprint_at(p, z, radius) else { return false };
            if floor >= z - LAYER {
                z = floor;
            }
        }
        true
    }

    /// Whether a flying body of `radius` with its feet at `z` can move from
    /// `a` to `b`. In volume scenes, each of its five points needs a floor
    /// no higher than its feet, and its head ([`BODY_HEIGHT`] up) under
    /// that floor's ceiling, every 0.05 m of the way; elsewhere
    /// [`Terrain::flyable`].
    ///
    /// [`BODY_HEIGHT`]: crate::volume::BODY_HEIGHT
    pub fn flyable_at(&self, a: FxVec2, b: FxVec2, z: Fx, radius: Fx) -> bool {
        let Some(volume) = &self.volume else { return self.flyable(a, b, z, radius) };
        let steps = ((b - a).length() / Fx::from_ratio(1, 20)).ceil().floor_to_int().max(1);
        let (feet, head) = (z + Fx::from_ratio(1, 50), z + crate::volume::BODY_HEIGHT);
        (1..=steps).all(|i| {
            let p = a + (b - a) * Fx::from_ratio(i, steps);
            Self::footprint_points(p, radius).iter().all(|&q| {
                volume.floor_at(q, z).is_some_and(|(floor, h)| {
                    h <= feet && volume.ceiling(Self::pit_tile(q), floor).is_none_or(|c| head <= crate::volume::height(c))
                })
            })
        })
    }

    /// Whether a flying body at height `z` can move from `a` to `b`: no ground
    /// along the way may rise above it.
    pub fn flyable(&self, a: FxVec2, b: FxVec2, z: Fx, radius: Fx) -> bool {
        let steps = ((b - a).length() / Fx::from_ratio(1, 20)).ceil().floor_to_int().max(1);
        let ceiling = z + Fx::from_ratio(1, 50);
        let offsets = [(Fx::ZERO, Fx::ZERO), (radius, Fx::ZERO), (-radius, Fx::ZERO), (Fx::ZERO, radius), (Fx::ZERO, -radius)];
        (0..=steps).all(|i| {
            let p = a + (b - a) * Fx::from_ratio(i, steps);
            offsets.iter().all(|&(dx, dy)| self.ground(FxVec2::new(p.x + dx, p.y + dy)).is_none_or(|y| y <= ceiling))
        })
    }

    /// The tile a pit at `p` would be dug in: tiles are 1 × 1 and centered
    /// on whole meters, as the deck's floor plates are.
    pub fn pit_tile(p: FxVec2) -> (i32, i32) {
        ((p.x + Fx::HALF).floor_to_int() as i32, (p.y + Fx::HALF).floor_to_int() as i32)
    }

    /// The center of a pit tile.
    pub fn pit_center(tile: (i32, i32)) -> FxVec2 {
        FxVec2::new(Fx::from_int(tile.0), Fx::from_int(tile.1))
    }

    /// How many layers deep the pit at `tile` is.
    pub fn pit(&self, tile: (i32, i32)) -> u8 {
        self.pits.binary_search_by_key(&tile, |&(t, _)| t).map_or(0, |at| self.pits[at].1)
    }

    /// Whether the sloped ground anywhere in `tile` is steeper than
    /// [`MAX_SLOPE`] along x or y: a tile edge whose corners differ by more
    /// than a layer. Never in unsloped scenes, whose steps
    /// [`Terrain::walkable`] checks instead.
    pub fn steep(&self, tile: (i32, i32)) -> bool {
        self.sloped && !self.level() && self.patch(Self::pit_center(tile)).is_some_and(|patch| {
            let [c00, c10, c01, c11] = patch.corners;
            [c10 - c00, c11 - c01, c01 - c00, c11 - c10].iter().any(|d| d.abs() > MAX_SLOPE)
        })
    }

    /// Whether the ground lets a digger of `radius` standing on tile `own`
    /// dig a layer out of `tile` (M3 of the mining design): a diggable tile
    /// in the arena, not as deep as pits go, left at most a layer below
    /// `own`, `own` no steeper for it, and no one stranded by it. What
    /// stands on the tile (buildings, piles) is the settlement's to check
    /// (`View::dig_in_front`).
    ///
    /// Stranded: from the middle of every tile, a body of `radius` walks to
    /// undug ground that isn't steep (`walks_out`). With no pits that
    /// holds; a dig changes the ground of the tiles touching it only, so
    /// only walks that cross those, or start beside them, change: checking
    /// the tiles within two of the dug one keeps it everywhere. The depth
    /// rule alone doesn't: deepening the last step of a stair from its
    /// bottom strands the bottom, and a deep pit beside a step drags its
    /// corners down until the step is too steep to climb.
    ///
    /// Since every corner of `own` is at most its height, and the dig
    /// lowers a corner to no less than a layer below it, a dig the depth
    /// rule allows never makes `own` steep that wasn't: that check is a
    /// guard for diggers that stand elsewhere than beside the tile.
    pub fn dig_refusal(&self, own: (i32, i32), tile: (i32, i32), radius: Fx) -> Option<DigRefusal> {
        if !self.diggable || !self.inside(Self::pit_center(tile)) || self.is_undiggable(tile) {
            return Some(DigRefusal::Undiggable);
        }
        if tile == own {
            return Some(DigRefusal::OwnTile);
        }
        if self.pit(tile) >= self.max_layers {
            return Some(DigRefusal::Bottom);
        }
        let (Some(here), Some(there)) = (self.tile_height(own), self.tile_height(tile)) else { return Some(DigRefusal::Undiggable) };
        // Dug, it would be a layer below `there`; it may go to a layer below `here`.
        if there < here {
            return Some(DigRefusal::TooDeep);
        }
        let mut dug = self.clone();
        dug.deepen(tile);
        if !self.steep(own) && dug.steep(own) {
            return Some(DigRefusal::Steepens);
        }
        if !dug.walks_out(tile, radius) {
            return Some(DigRefusal::Strands);
        }
        None
    }

    /// Whether a body of `radius` in the middle of any tile within two of
    /// `around` can walk to the middle of an undug tile that isn't steep:
    /// a search from tile middle to tile middle, along x, y and diagonals,
    /// where [`Terrain::walkable`] allows it (so a body on steep ground may
    /// walk off it). Obstacles aren't counted: this is about the ground.
    /// A search that finds [`WALK_OUT_TILES`] tiles counts as out.
    fn walks_out(&self, around: (i32, i32), radius: Fx) -> bool {
        let out = |t: (i32, i32)| self.pit(t) == 0 && !self.steep(t);
        let ground = |t: (i32, i32)| self.inside(Self::pit_center(t)) && self.tile_height(t).is_some();
        // Tiles found to walk out, so later searches stop at them.
        let mut free = std::collections::BTreeSet::new();
        for dy in -2..=2 {
            for dx in -2..=2 {
                let from = (around.0 + dx, around.1 + dy);
                if !ground(from) || out(from) {
                    continue;
                }
                let mut seen = std::collections::BTreeSet::from([from]);
                let mut open = std::collections::VecDeque::from([from]);
                let mut found = false;
                while let Some(t) = open.pop_front() {
                    if out(t) || free.contains(&t) || seen.len() >= WALK_OUT_TILES {
                        found = true;
                        break;
                    }
                    for (nx, ny) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                        let n = (t.0 + nx, t.1 + ny);
                        if !seen.contains(&n) && ground(n) && self.walkable(Self::pit_center(t), Self::pit_center(n), radius) {
                            seen.insert(n);
                            open.push_back(n);
                        }
                    }
                }
                if !found {
                    return false;
                }
                free.insert(from);
            }
        }
        true
    }

    /// The tile in front of a body at `p` facing `forward` (any length):
    /// the one beside its own along the axis it faces most, x on a tie.
    pub fn front_tile(p: FxVec2, forward: FxVec2) -> (i32, i32) {
        let (x, y) = Self::pit_tile(p);
        if forward.x.abs() >= forward.y.abs() {
            (x + if forward.x < Fx::ZERO { -1 } else { 1 }, y)
        } else {
            (x, y + if forward.y < Fx::ZERO { -1 } else { 1 })
        }
    }

    /// Whether `tile` is in one of the [`Terrain::undiggable`] ranges.
    pub fn is_undiggable(&self, tile: (i32, i32)) -> bool {
        self.undiggable.iter().any(|&(min, max)| tile.0 >= min.0 && tile.0 <= max.0 && tile.1 >= min.1 && tile.1 <= max.1)
    }

    /// Digs one more layer out of `tile`, down to [`Terrain::max_layers`].
    /// Returns whether it did.
    pub fn deepen(&mut self, tile: (i32, i32)) -> bool {
        match self.pits.binary_search_by_key(&tile, |&(t, _)| t) {
            Ok(at) if self.pits[at].1 < self.max_layers => self.pits[at].1 += 1,
            Ok(_) => return false,
            Err(at) => self.pits.insert(at, (tile, 1)),
        }
        self.revision += 1;
        true
    }

    /// Makes a cell of the volume solid. Returns whether it changed: never
    /// without a volume.
    pub fn fill_cell(&mut self, tile: (i32, i32), layer: i32) -> bool {
        let filled = self.volume.as_mut().is_some_and(|volume| volume.fill(tile, layer));
        if filled {
            self.revision += 1;
        }
        filled
    }

    /// Clears a cell of the volume, and its ore. Returns whether it changed:
    /// never without a volume.
    pub fn clear_cell(&mut self, tile: (i32, i32), layer: i32) -> bool {
        let cleared = self.volume.as_mut().is_some_and(|volume| volume.clear(tile, layer));
        if cleared {
            self.revision += 1;
        }
        cleared
    }

    pub fn is_rock(&self, tile: (i32, i32)) -> bool {
        self.rock.binary_search(&tile).is_ok()
    }

    pub fn add_rock(&mut self, tile: (i32, i32)) {
        if let Err(at) = self.rock.binary_search(&tile) {
            self.rock.insert(at, tile);
            self.revision += 1;
        }
    }

    /// Removes a rock column. Returns whether there was one.
    pub fn dig(&mut self, tile: (i32, i32)) -> bool {
        match self.rock.binary_search(&tile) {
            Ok(at) => {
                self.rock.remove(at);
                self.revision += 1;
                true
            }
            Err(_) => false,
        }
    }

    /// Whether a body of `radius` can move in a straight line from `a` to `b`.
    pub fn clear(&self, a: FxVec2, b: FxVec2, radius: Fx) -> bool {
        self.clear_with(a, b, radius, &[])
    }

    /// [`Terrain::clear`], also avoiding `extra` discs (other bodies).
    pub fn clear_with(&self, a: FxVec2, b: FxVec2, radius: Fx, extra: &[Disc]) -> bool {
        if !self.inside(a) || !self.inside(b) {
            return false;
        }
        let blocked = |o: &Disc| {
            let reach = o.radius + radius + CLEARANCE_MARGIN;
            segment_distance_squared(o.center, a, b) <= reach * reach
        };
        if self.blocking_obstacles().chain(extra.iter()).any(blocked) {
            return false;
        }
        if self.rock.is_empty() {
            return true;
        }
        // A rock column blocks like the disc that circumscribes it.
        let reach = Fx::from_ratio(71, 100) + radius + CLEARANCE_MARGIN;
        let x0 = (a.x.min(b.x) - reach).floor_to_int() as i32;
        let x1 = (a.x.max(b.x) + reach).floor_to_int() as i32;
        let y0 = (a.y.min(b.y) - reach).floor_to_int() as i32;
        let y1 = (a.y.max(b.y) + reach).floor_to_int() as i32;
        let start = self.rock.partition_point(|&t| t < (x0, i32::MIN));
        self.rock[start..]
            .iter()
            .take_while(|t| t.0 <= x1)
            .filter(|t| t.1 >= y0 && t.1 <= y1)
            .all(|&(x, y)| {
                let center = FxVec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF);
                segment_distance_squared(center, a, b) > reach * reach
            })
    }

    /// Every solid disc. Repairable cover leaves a blocking floor base.
    pub fn blocking_obstacles(&self) -> impl Iterator<Item = &Disc> {
        self.obstacles.iter().chain(self.destructibles.iter().filter(|p| p.health > 0 || p.repairable).map(|p| &p.disc))
    }

    pub fn asset_in_repair_reach(&self, position: FxVec2, asset: u16) -> bool {
        self.destructibles.iter().filter(|piece| piece.asset == asset && piece.repairable)
            .any(|piece| {
                let reach = piece.disc.radius + Fx::from_ratio(11, 10);
                (position - piece.disc.center).length_squared() <= reach * reach
            })
    }

    /// Select a damaged cover asset from any side of its full footprint.
    pub fn repairable_asset_near(&self, position: FxVec2) -> Option<u16> {
        self.destructibles.iter()
            .filter(|piece| piece.repairable && self.destructibles.iter()
                .any(|other| other.asset == piece.asset && other.health < other.max_health))
            .filter(|piece| self.asset_in_repair_reach(position, piece.asset))
            .map(|piece| ((position - piece.disc.center).length_squared(), piece.asset))
            .min()
            .map(|(_, asset)| asset)
    }

    /// Apply a weapon hit to a cover piece; crossing zero opens its footprint.
    pub fn damage_cover(&mut self, index: usize, damage: u16) -> bool {
        let Some(piece) = self.destructibles.get_mut(index) else { return false };
        if piece.health == 0 { return false; }
        piece.health = piece.health.saturating_sub(damage);
        piece.repair_progress = 0;
        if piece.health == 0 && !piece.repairable { self.revision += 1; }
        true
    }

    /// Progress one tick of nearby held repair. Each second restores one
    /// damage stage; releasing or leaving resets only the current stage.
    pub fn repair_cover(&mut self, index: usize, held: bool) {
        let Some(piece) = self.destructibles.get_mut(index) else { return };
        if !piece.repairable || piece.health >= piece.max_health || !held {
            piece.repair_progress = 0;
            return;
        }
        piece.repair_progress += 1;
        if piece.repair_progress >= content::TICKS_PER_SECOND as u16 {
            piece.health += 1;
            piece.repair_progress = 0;
        }
    }
}

impl StateHash for Terrain {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.center);
        h.write(&self.extent);
        h.write_u64(self.obstacles.len() as u64);
        for o in &self.obstacles {
            h.write(&o.center);
            h.write(&o.radius);
        }
        if !self.destructibles.is_empty() {
            h.write_u64(self.destructibles.len() as u64);
            for piece in &self.destructibles {
                h.write(&piece.disc.center);
                h.write_u64(piece.asset as u64);
                h.write(&piece.disc.radius);
                h.write_u64(piece.health as u64);
                h.write_u64(piece.max_health as u64);
                h.write_u64(piece.repairable as u64);
                h.write_u64(piece.repair_progress as u64);
            }
        }
        h.write_u64(self.surfaces.len() as u64);
        for s in &self.surfaces {
            h.write(&s.min);
            h.write(&s.max);
            h.write(&s.height);
        }
        h.write(&self.platforms_only);
        h.write_u64(self.rock.len() as u64);
        for &(x, y) in &self.rock {
            h.write_u64((x as u32 as u64) << 32 | y as u32 as u64);
        }
        h.write(&self.diggable);
        h.write_u64(self.pits.len() as u64);
        for &((x, y), depth) in &self.pits {
            h.write_u64((x as u32 as u64) << 32 | y as u32 as u64);
            h.write_u64(depth as u64);
        }
        if let Some(mine) = &self.mine {
            h.write_u64(mine.seed);
            h.write_u64((mine.min.0 as u32 as u64) << 32 | mine.min.1 as u32 as u64);
            h.write_u64(mine.size as u64);
            h.write_u64((mine.landing.0 as u32 as u64) << 32 | mine.landing.1 as u32 as u64);
            if mine.layers != MAX_LAYERS {
                h.write_u64(mine.layers as u64);
            }
            if mine.veins != Veins::Ore {
                h.write_u64(mine.veins as u64);
            }
            if let Some(hill) = &mine.hill {
                h.write_u64((hill.center.0 as u32 as u64) << 32 | hill.center.1 as u32 as u64);
                for v in [hill.plateau, hill.steps, hill.ramp_width, hill.ramp_length, hill.foot] {
                    h.write_u64(v as u64);
                }
            }
        }
        // Only when set, as below.
        if !self.undiggable.is_empty() {
            h.write_u64(self.undiggable.len() as u64);
            for &(min, max) in &self.undiggable {
                h.write_u64((min.0 as u32 as u64) << 32 | min.1 as u32 as u64);
                h.write_u64((max.0 as u32 as u64) << 32 | max.1 as u32 as u64);
            }
        }
        // Only when set, so terrains from before sloped scenes hash the same.
        if self.sloped || self.max_layers != MAX_LAYERS {
            h.write(&self.sloped);
            h.write_u64(self.max_layers as u64);
        }
        // Only when set, so terrains from before volumes hash the same.
        if let Some(volume) = &self.volume {
            h.write(volume);
        }
        h.write_u64(self.revision);
    }
}

fn max_layers() -> u8 {
    MAX_LAYERS
}

/// The sloped ground over one tile: its corner heights, and where in it a
/// point is.
pub(crate) struct Patch {
    /// At the tile's -x -y, +x -y, -x +y and +x +y corners.
    corners: [Fx; 4],
    /// The point, 0 to 1 across the tile along x and along y.
    u: Fx,
    v: Fx,
}

impl Patch {
    /// The ground over `p`'s tile by M6's corner rule, the tile `h` high and
    /// `around` giving each tile's height (nothing for a drop): each corner
    /// takes the lowest of the four tiles touching it, drops counting as
    /// `h`.
    pub(crate) fn over(p: FxVec2, h: Fx, around: impl Fn((i32, i32)) -> Option<Fx>) -> Patch {
        let (x, y) = Terrain::pit_tile(p);
        // The 3 × 3 tiles around it, row by row from -y.
        let mut heights = [[Some(h); 3]; 3];
        for (dy, row) in heights.iter_mut().enumerate() {
            for (dx, height) in row.iter_mut().enumerate() {
                if (dx, dy) != (1, 1) {
                    *height = around((x + dx as i32 - 1, y + dy as i32 - 1));
                }
            }
        }
        // The corner between rows `dy`, `dy + 1` and columns `dx`, `dx + 1`.
        let corner = |dx: usize, dy: usize| {
            [heights[dy][dx], heights[dy][dx + 1], heights[dy + 1][dx], heights[dy + 1][dx + 1]].into_iter().flatten().fold(h, Fx::min)
        };
        // Where `p` is in the tile, 0 to 1 from its -x -y corner.
        let u = p.x + Fx::HALF - Fx::from_int(x);
        let v = p.y + Fx::HALF - Fx::from_int(y);
        Patch { corners: [corner(0, 0), corner(1, 0), corner(0, 1), corner(1, 1)], u, v }
    }

    /// The height at the point, bilinear between the corners.
    pub(crate) fn height(&self) -> Fx {
        let [c00, c10, c01, c11] = self.corners;
        let near = c00 + (c10 - c00) * self.u;
        let far = c01 + (c11 - c01) * self.u;
        near + (far - near) * self.v
    }

    /// Whether the ground at the point rises or falls more than
    /// [`MAX_SLOPE`] per tile along x or along y. Along a diagonal it can
    /// be up to √2 steeper, as at the corner of a one-layer pit.
    fn steep(&self) -> bool {
        let [c00, c10, c01, c11] = self.corners;
        let along_x = (c10 - c00) + ((c11 - c01) - (c10 - c00)) * self.v;
        let along_y = (c01 - c00) + ((c11 - c10) - (c01 - c00)) * self.u;
        along_x.abs() > MAX_SLOPE || along_y.abs() > MAX_SLOPE
    }
}

/// Squared distance from `p` to the segment `a`–`b`.
pub fn segment_distance_squared(p: FxVec2, a: FxVec2, b: FxVec2) -> Fx {
    let ab = b - a;
    let ap = p - a;
    let len2 = ab.length_squared();
    let closest = if len2 == Fx::ZERO {
        a
    } else {
        let t = (ap.dot(ab) / len2).clamp(Fx::ZERO, Fx::ONE);
        a + ab * t
    };
    (p - closest).length_squared()
}

/// Where the segment `a`–`b` first touches the disc, as a fraction of the
/// segment in [0, 1], or `None`. Zero if `a` starts inside.
pub fn segment_disc(a: FxVec2, b: FxVec2, center: FxVec2, radius: Fx) -> Option<Fx> {
    let d = b - a;
    let f = a - center;
    let aa = d.length_squared();
    if aa == Fx::ZERO {
        return None;
    }
    let cc = f.length_squared() - radius * radius;
    if cc <= Fx::ZERO {
        return Some(Fx::ZERO);
    }
    let bb = f.dot(d).mul_int(2);
    let disc = bb * bb - (aa * cc).mul_int(4);
    if disc < Fx::ZERO {
        return None;
    }
    let t = (-bb - disc.sqrt()) / aa.mul_int(2);
    (t >= Fx::ZERO && t <= Fx::ONE).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> FxVec2 {
        FxVec2::new(Fx::from_bits((x * 4294967296.0) as i64), Fx::from_bits((y * 4294967296.0) as i64))
    }

    #[test]
    fn segments_and_discs() {
        let terrain = Terrain {
            obstacles: vec![Disc { center: v(0.0, 0.0), radius: Fx::HALF }],
            ..Terrain::flat(v(0.0, 0.0), Fx::from_int(8))
        };
        assert!(!terrain.clear(v(-2.0, 0.0), v(2.0, 0.0), Fx::from_ratio(1, 5)));
        assert!(terrain.clear(v(-2.0, 1.0), v(2.0, 1.0), Fx::from_ratio(1, 5)));
        assert!(!terrain.clear(v(-2.0, 0.0), v(9.0, 0.0), Fx::ZERO), "outside the arena");
        let hit = segment_disc(v(-2.0, 0.0), v(2.0, 0.0), v(0.0, 0.0), Fx::ONE).unwrap();
        assert!((hit.to_f64() - 0.25).abs() < 1e-6);
        assert_eq!(segment_disc(v(-2.0, 2.0), v(2.0, 2.0), v(0.0, 0.0), Fx::ONE), None);
    }

    #[test]
    fn rock_blocks_until_dug() {
        let mut terrain = Terrain::flat(v(0.0, 0.0), Fx::from_int(8));
        terrain.add_rock((0, 0));
        assert!(!terrain.clear(v(-2.0, 0.5), v(2.0, 0.5), Fx::from_ratio(1, 5)));
        let revision = terrain.revision;
        assert!(terrain.dig((0, 0)));
        assert!(!terrain.dig((0, 0)));
        assert!(terrain.revision > revision);
        assert!(terrain.clear(v(-2.0, 0.5), v(2.0, 0.5), Fx::from_ratio(1, 5)));
    }

    #[test]
    fn height_is_the_highest_surface() {
        let mut terrain = Terrain::flat(v(0.0, 0.0), Fx::from_int(8));
        terrain.surfaces.push(Surface { min: v(-1.0, -1.0), max: v(1.0, 1.0), height: Fx::ONE });
        terrain.surfaces.push(Surface { min: v(0.0, 0.0), max: v(2.0, 2.0), height: Fx::from_int(2) });
        assert_eq!(terrain.height(v(-0.5, -0.5)), Fx::ONE);
        assert_eq!(terrain.height(v(0.5, 0.5)), Fx::from_int(2));
        assert_eq!(terrain.height(v(5.0, 5.0)), Fx::ZERO);
    }

    #[test]
    fn pits_sink_the_ground_down_to_three_layers() {
        let mut terrain = Terrain::flat(v(0.0, 0.0), Fx::from_int(8));
        let tile = Terrain::pit_tile(v(2.3, -0.6));
        assert_eq!(tile, (2, -1));
        for depth in 1..=MAX_LAYERS {
            assert!(terrain.deepen(tile));
            assert_eq!(terrain.pit(tile), depth);
        }
        assert!(!terrain.deepen(tile), "no deeper than the last layer");
        assert_eq!(terrain.height(v(1.6, -1.4)), -LAYER.mul_int(3));
        assert_eq!(terrain.height(v(2.6, -1.4)), Fx::ZERO, "the next tile");
        // On foot, a pit is a drop too deep to walk into or out of.
        let r = Fx::from_ratio(8, 25);
        assert!(!terrain.walkable(v(0.0, -1.0), v(2.0, -1.0), r));
        assert!(!terrain.walkable(v(2.0, -1.0), v(4.0, -1.0), r));
        assert!(terrain.walkable(v(0.0, 1.0), v(4.0, 1.0), r));
    }

    fn sloped() -> Terrain {
        Terrain { sloped: true, max_layers: 6, ..Terrain::flat(v(0.0, 0.0), Fx::from_int(8)) }
    }

    /// Corners take the lowest of the tiles touching them: a dug tile's floor
    /// is flat, and its neighbours slope down into it a layer per tile,
    /// gently enough to walk.
    #[test]
    fn a_pit_has_a_flat_floor_and_its_neighbours_slope_into_it() {
        let mut terrain = sloped();
        terrain.deepen((0, 0));
        for p in [v(0.0, 0.0), v(-0.4, 0.3), v(0.45, -0.45)] {
            assert_eq!(terrain.height(p), -LAYER, "the floor at {p:?}");
        }
        // The side neighbour falls from its far edge to the pit's.
        assert_eq!(terrain.height(v(1.5, 0.0)), Fx::ZERO);
        assert_eq!(terrain.height(v(1.0, 0.0)), -LAYER / Fx::from_int(2));
        assert_eq!(terrain.height(v(0.5, 0.2)), -LAYER);
        // The diagonal one dips at the corner it shares.
        assert_eq!(terrain.height(v(0.5, 0.5)), -LAYER);
        assert_eq!(terrain.height(v(1.0, 1.0)), -LAYER / Fx::from_int(4));
        assert_eq!(terrain.height(v(1.5, 1.5)), Fx::ZERO);
        assert_eq!(terrain.height(v(2.5, 0.0)), Fx::ZERO, "two tiles away");
        assert!((-2..=2).all(|x| (-2..=2).all(|y| !terrain.steep((x, y)))));
        // Walked in and out from any side, and across.
        let r = Fx::from_ratio(8, 25);
        for to in [v(3.0, 0.0), v(-3.0, 0.0), v(0.0, 3.0), v(2.0, 2.0)] {
            assert!(terrain.walkable(v(0.0, 0.0), to, r));
            assert!(terrain.walkable(to, v(0.0, 0.0), r));
        }
        assert!(terrain.walkable(v(-3.0, 0.0), v(3.0, 0.0), r));
        // Unsloped, the same pit is a step too high.
        terrain.sloped = false;
        assert_eq!(terrain.height(v(1.0, 0.0)), Fx::ZERO);
        assert!(!terrain.walkable(v(0.0, 0.0), v(3.0, 0.0), r));
    }

    /// A tile whose neighbours are two layers apart slopes a meter across:
    /// too steep to walk on, along it or across it.
    #[test]
    fn a_tile_between_neighbours_two_layers_apart_is_too_steep() {
        let mut terrain = sloped();
        terrain.deepen((0, 0));
        terrain.deepen((0, 0));
        assert!(terrain.steep((1, 0)) && terrain.steep((0, 1)) && terrain.steep((-1, -1)));
        assert!(!terrain.steep((0, 0)), "the floor is flat");
        assert!(!terrain.steep((2, 0)), "the ground beyond is flat");
        let r = Fx::from_ratio(8, 25);
        assert!(!terrain.walkable(v(3.0, 0.0), v(0.0, 0.0), r), "no way in");
        assert!(!terrain.walkable(v(0.0, 0.0), v(3.0, 0.0), r), "no way out");
        assert!(!terrain.walkable(v(1.0, -2.0), v(1.0, 2.0), r), "nor along the slope");
        assert!(terrain.walkable(v(2.0, -2.0), v(2.0, 2.0), r), "the next tile over is fine");
        // The body's edge counts: its center beside the slope, it can't
        // lean over it.
        assert!(!terrain.walkable(v(1.7, -2.0), v(1.7, 2.0), r));
        // Each layer steps down as deep as the scene allows.
        for depth in 3..=6 {
            assert!(terrain.deepen((0, 0)));
            assert_eq!(terrain.height(v(0.0, 0.0)), -LAYER.mul_int(depth));
        }
        assert!(!terrain.deepen((0, 0)), "six layers is the scene's limit");
    }

    /// A route to the bottom of a deep pit goes around its walls and walks
    /// in by a ramp a layer above the floor.
    #[test]
    fn paths_go_around_a_deep_pit_and_in_by_its_ramp() {
        let mut terrain = sloped();
        let r = Fx::from_ratio(8, 25);
        terrain.deepen((0, 0));
        terrain.deepen((0, 0));
        assert!(crate::nav::path(&terrain, v(0.0, 4.0), v(0.0, 0.0), r).is_empty(), "no way in");
        // The ramp: one layer, to the -y side.
        terrain.deepen((0, -1));
        let route = crate::nav::path(&terrain, v(0.0, 4.0), v(0.0, 0.0), r);
        assert!(!route.is_empty(), "a way in");
        let mut from = v(0.0, 4.0);
        for &to in &route {
            assert!(terrain.walkable(from, to, r), "{from:?} to {to:?}");
            from = to;
        }
        let (length, _) = route.iter().fold((Fx::ZERO, v(0.0, 4.0)), |(length, at), &to| (length + crate::nav::distance(at, to), to));
        assert!(length > Fx::from_int(6), "around the pit ({length:?})");
        // The last leg comes up the ramp, from the -y side.
        let before = route[route.len() - 2];
        assert!(before.y < -Fx::HALF && before.x.abs() < Fx::ONE, "by the ramp: {route:?}");
        // Out again, the same way.
        assert!(!crate::nav::path(&terrain, v(0.0, 0.0), v(0.0, 4.0), r).is_empty());
    }
}

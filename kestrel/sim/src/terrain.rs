//! Terrain: the arena's walkable ground and what blocks it.
//!
//! Scenes are arenas centered on `center` with half-width `extent`.
//! Movement is blocked by authored obstacles (discs: rocks, trees, machines)
//! and by rock voxels, 1 × 1 tile columns that can be dug out. Walkable
//! surfaces are raised boxes (platforms) that set the ground height. In
//! digging scenes, avatars dig pits into the ground, a layer at a time, and
//! mines generate their ground (terraces and ore) from a seed.
//! Terrain is part of the saved state; navigation caches built from it are not.

use std::sync::{Arc, OnceLock};

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};
use crate::mine::{Mine, MineSpec};

/// Extra separation kept from obstacles, as in the original game (1.5 cm).
pub const CLEARANCE_MARGIN: Fx = Fx::from_bits(64_424_509); // 0.015

/// How much ground one dig removes.
pub const LAYER: Fx = Fx::HALF;
/// The deepest a pit goes, in layers.
pub const MAX_LAYERS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disc {
    pub center: FxVec2,
    pub radius: Fx,
}

/// An individually removable piece of cover. Its index is stable for the
/// lifetime of the world, including saves and lockstep replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destructible {
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
    /// Generated ground under the whole arena, if this is a mine.
    #[serde(default)]
    pub mine: Option<MineSpec>,
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
            mine: None,
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
    /// zero, or nothing (a drop) in platform scenes. Pits sink it.
    pub fn ground(&self, p: FxVec2) -> Option<Fx> {
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

    /// The generated mine, if this terrain is one.
    pub fn mine(&self) -> Option<&Mine> {
        let spec = self.mine?;
        Some(self.generated.0.get_or_init(|| Arc::new(Mine::generate(spec))))
    }

    /// Makes the arena a mine generated from `seed`, its landing at `spawn`.
    pub fn generate_mine(&mut self, seed: u64, spawn: FxVec2) {
        let min = Self::pit_tile(self.min());
        let size = (Self::pit_tile(self.max()).0 - min.0 + 1) as u32;
        self.mine = Some(MineSpec { seed, min, size, landing: Self::pit_tile(spawn) });
        self.generated = Generated::default();
        self.revision += 1;
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
    pub fn walkable(&self, a: FxVec2, b: FxVec2, radius: Fx) -> bool {
        let steps = ((b - a).length() / Fx::from_ratio(1, 10)).ceil().floor_to_int().max(1);
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

    /// Digs one more layer out of `tile`, down to [`MAX_LAYERS`]. Returns
    /// whether it did.
    pub fn deepen(&mut self, tile: (i32, i32)) -> bool {
        match self.pits.binary_search_by_key(&tile, |&(t, _)| t) {
            Ok(at) if self.pits[at].1 < MAX_LAYERS => self.pits[at].1 += 1,
            Ok(_) => return false,
            Err(at) => self.pits.insert(at, (tile, 1)),
        }
        self.revision += 1;
        true
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
        }
        h.write_u64(self.revision);
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
}

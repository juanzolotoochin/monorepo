//! Terrain: the arena's walkable ground and what blocks it.
//!
//! Scenes are arenas centered on `center` with half-width `extent`.
//! Movement is blocked by authored obstacles (discs: rocks, trees, machines)
//! and by rock voxels, 1 × 1 tile columns that can be dug out. Walkable
//! surfaces are raised boxes (platforms) that set the ground height.
//! Terrain is part of the saved state; navigation caches built from it are not.

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};

/// Extra separation kept from obstacles, as in the original game (1.5 cm).
pub const CLEARANCE_MARGIN: Fx = Fx::from_bits(64_424_509); // 0.015

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disc {
    pub center: FxVec2,
    pub radius: Fx,
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
    pub surfaces: Vec<Surface>,
    /// Diggable rock columns by integer tile, sorted.
    pub rock: Vec<(i32, i32)>,
    /// Bumped by every change, so cached paths know to rebuild.
    pub revision: u64,
}

impl Terrain {
    pub fn flat(center: FxVec2, extent: Fx) -> Terrain {
        Terrain { center, extent, obstacles: Vec::new(), surfaces: Vec::new(), rock: Vec::new(), revision: 0 }
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

    /// Ground height at `p`: the highest surface containing it, or zero.
    pub fn height(&self, p: FxVec2) -> Fx {
        self.surfaces
            .iter()
            .filter(|s| p.x >= s.min.x && p.x <= s.max.x && p.y >= s.min.y && p.y <= s.max.y)
            .map(|s| s.height)
            .max()
            .unwrap_or(Fx::ZERO)
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
        if self.obstacles.iter().chain(extra).any(blocked) {
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
        h.write_u64(self.surfaces.len() as u64);
        for s in &self.surfaces {
            h.write(&s.min);
            h.write(&s.max);
            h.write(&s.height);
        }
        h.write_u64(self.rock.len() as u64);
        for &(x, y) in &self.rock {
            h.write_u64((x as u32 as u64) << 32 | y as u32 as u64);
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
}

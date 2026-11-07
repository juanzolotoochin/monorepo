//! The volume's ground (V9 of the verticality design,
//! `kestrel/VERTICALITY.md`): a volume scene's chunk of 16 × 16 columns,
//! drawn from the simulation's cells (`sim::volume`).
//!
//! - Tops: the top of every span (a run of solid cells with empty above),
//!   sloped by the volume's corner rule (`Volume::patch`: each corner the
//!   lowest of the four tiles touching it, a neighbour counting by its
//!   floor within a layer), so a drawn floor is the floor the simulation
//!   walks on, ramps and dug stairs included.
//! - Ceilings: the underside of every span, flat, facing down.
//! - Walls: on each tile side, the part of the column's solid profile (each
//!   span from its flat bottom up to its sloped top along that side) that
//!   the neighbour's profile doesn't cover. So there is no face between two
//!   solids, and a wall reaches exactly up to the sloped top over it: no
//!   cracks where a ramp meets a blocky wall.
//!
//! Cut faces (walls and ceilings) are drawn darker than tops and carry
//! `UV_0.x = 1` (tops 0), for a shader to tell them apart (the cutaway
//! doesn't need it); all use the volume ground's one vertex-coloured
//! material (`cutaway::CutawayMaterial`).

use bevy::prelude::*;
use fx::FxVec2;
use sim::terrain::Terrain;
use sim::volume::{height, Volume};

use crate::digging::{corner_at, mix, slab, surface, MeshBuilder, CHUNK, ROCK, SIDES};

/// Cut faces (walls and ceilings) are this much darker than tops.
const CUT: f32 = 0.82;
/// Deep stone and high stone, for walls by layer.
const HIGH_STONE: Vec3 = Vec3::new(0.37, 0.28, 0.22);

/// The bottom of layer `layer`, in meters.
fn h(layer: i32) -> f32 {
    height(layer).to_f32()
}

/// Stone by layer: rock deep down, warmer up the mountain.
fn stone(layer: i32) -> Vec3 {
    ROCK.lerp(HIGH_STONE, ((layer + 8) as f32 / 24.0).clamp(0.0, 1.0))
}

/// The drawn corners of the top of a span ending under layer `top` in
/// `tile`'s column, at its -x -y, +x -y, -x +y and +x +y corners: M6's
/// corner rule as `Volume::patch` has it, each corner the lowest of the
/// four tiles touching it, a neighbour by its floor within a layer of `top`
/// (`Volume::floor_near`), none counting as level.
pub(crate) fn top_corners(volume: &Volume, tile: (i32, i32), top: i32) -> [f32; 4] {
    let own = h(top);
    let around = |dx: i32, dy: i32| if (dx, dy) == (0, 0) { Some(own) } else { volume.floor_near((tile.0 + dx, tile.1 + dy), top).map(h) };
    let corner = |cx: i32, cy: i32| [(cx - 1, cy - 1), (cx, cy - 1), (cx - 1, cy), (cx, cy)].into_iter().filter_map(|(dx, dy)| around(dx, dy)).fold(own, f32::min);
    [corner(0, 0), corner(1, 0), corner(0, 1), corner(1, 1)]
}

/// The part of a convex polygon where `inside` is at least zero (`inside`
/// linear in the point, so the cut is exact).
fn clip(points: &[Vec3], inside: impl Fn(Vec3) -> f32) -> Vec<Vec3> {
    let mut kept = Vec::with_capacity(points.len() + 2);
    for (n, &p) in points.iter().enumerate() {
        let q = points[(n + 1) % points.len()];
        let (dp, dq) = (inside(p), inside(q));
        if dp >= 0.0 {
            kept.push(p);
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            kept.push(p.lerp(q, dp / (dp - dq)));
        }
    }
    kept
}

/// Twice a polygon's area.
fn area(points: &[Vec3]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    points.windows(2).skip(1).map(|w| (w[0] - points[0]).cross(w[1] - points[0])).sum::<Vec3>().length()
}

/// A column's solid runs along one side of its tile: each span's bottom,
/// and its sloped top at the side's two corners (`corners`, in
/// [`top_corners`]' order).
fn profile(volume: &Volume, tile: (i32, i32), corners: [usize; 2]) -> Vec<(f32, f32, f32)> {
    volume
        .spans(tile)
        .iter()
        .map(|s| {
            let c = top_corners(volume, tile, s.top);
            (h(s.bottom), c[corners[0]], c[corners[1]])
        })
        .collect()
}

/// A volume scene's chunk `key` (16 × 16 arena tiles from the arena's
/// corner, the arena `extent` tiles round its `center` tile): tops,
/// ceilings and walls, as the module says.
pub(crate) fn chunk_mesh(volume: &Volume, center: IVec2, extent: i32, key: IVec2) -> Mesh {
    chunk_mesh_with(volume, center, extent, key, None)
}

/// The colour a mineral of a volume tints its cells: a town's stone grey
/// (`mine::STONE`), else the minerals' colours.
fn ore_color(mineral: u8) -> Vec3 {
    if mineral == sim::mine::STONE {
        crate::digging::STONE_TINT
    } else {
        Vec3::from_array(crate::piles::mineral_color(crate::piles::mineral(mineral)))
    }
}

/// [`chunk_mesh`], with open-sky tops coloured as a mine's terraces are
/// when `terraces` is the layer of the mine's lowest ground (a town's
/// volume, M8.3 of the depths design: its datum is the hill's top, the
/// ground round it lower), so the town looks as its heightfield did:
/// by terrace steps up from there, the ore at the top tinting it lightly.
pub(crate) fn chunk_mesh_with(volume: &Volume, center: IVec2, extent: i32, key: IVec2, terraces: Option<i32>) -> Mesh {
    let tile = |a: IVec2| ((a + center).x, (a + center).y);
    let ore = |a: IVec2, k: i32| volume.ore(tile(a), k).map(ore_color);
    let mut mesh = MeshBuilder::default();
    let first = key * CHUNK - IVec2::splat(extent);
    for y in first.y..(first.y + CHUNK).min(extent + 1) {
        for x in first.x..(first.x + CHUNK).min(extent + 1) {
            let a = IVec2::new(x, y);
            let spans = volume.spans(tile(a));
            if spans.is_empty() {
                continue;
            }
            let shade = if (x + y) & 1 == 0 { 1.0 } else { 0.94 };
            let (x0, x1, z0, z1) = (x as f32 - 0.5, x as f32 + 0.5, y as f32 - 0.5, y as f32 + 0.5);
            let tops: Vec<[f32; 4]> = spans.iter().map(|s| top_corners(volume, tile(a), s.top)).collect();
            for (n, (span, top)) in spans.iter().zip(&tops).enumerate() {
                // The top: under open sky, ground coloured by height as
                // terraces are; under a ceiling (a tunnel's or cave's floor),
                // the stone it is cut in.
                mesh.cut = false;
                let open = n + 1 == spans.len();
                let base = match terraces {
                    Some(lowest) if open => surface(((span.top - lowest) * 2).clamp(0, 12) as u8),
                    _ if open => surface((span.top.max(0) * 3 / 4).min(12) as u8),
                    _ => stone(span.top - 1) * 1.12,
                };
                let tint = if terraces.is_some() && open { 0.3 } else { 0.55 };
                let color = mix(base, ore(a, span.top - 1), tint) * shade;
                let p: [Vec3; 4] = std::array::from_fn(|i| corner_at(a, i, top[i]));
                // Split along the diagonal whose ends are closer in height,
                // as the sloped ground does.
                if (top[0] - top[3]).abs() <= (top[1] - top[2]).abs() {
                    mesh.triangle([p[0], p[1], p[3]], [color; 3]);
                    mesh.triangle([p[0], p[3], p[2]], [color; 3]);
                } else {
                    mesh.triangle([p[0], p[1], p[2]], [color; 3]);
                    mesh.triangle([p[1], p[3], p[2]], [color; 3]);
                }
                // The underside: a cave's or tunnel's ceiling.
                mesh.cut = true;
                let b = h(span.bottom);
                mesh.quad(
                    [Vec3::new(x0, b, z0), Vec3::new(x1, b, z0), Vec3::new(x1, b, z1), Vec3::new(x0, b, z1)],
                    Vec3::NEG_Y,
                    mix(stone(span.bottom), ore(a, span.bottom), 0.6) * CUT * 0.85,
                );
            }
            mesh.cut = true;
            for (side, [i, j], [ti, tj]) in SIDES {
                let theirs = profile(volume, tile(a + side), [ti, tj]);
                let normal = Vec3::new(side.x as f32, 0.0, side.y as f32);
                let (pi, pj) = (corner_at(a, i, 0.0), corner_at(a, j, 0.0));
                // 0 at corner `i`, 1 at corner `j`.
                let along = |p: Vec3| (p - pi).dot(pj - pi);
                for (span, top) in spans.iter().zip(&tops) {
                    let b = h(span.bottom);
                    let mut pieces = vec![vec![corner_at(a, i, b), corner_at(a, j, b), corner_at(a, j, top[j]), corner_at(a, i, top[i])]];
                    // Take out what each of the neighbour's spans covers: keep
                    // what is under its bottom and over its sloped top.
                    for &(bottom, top_i, top_j) in &theirs {
                        pieces = pieces
                            .into_iter()
                            .flat_map(|q| [clip(&q, |p| bottom - p.y), clip(&q, |p| p.y - (top_i + (top_j - top_i) * along(p)))])
                            .filter(|q| area(q) > 1e-6)
                            .collect();
                    }
                    for piece in pieces {
                        let (low, high) = piece.iter().fold((f32::MAX, f32::MIN), |(l, u), p| (l.min(p.y), u.max(p.y)));
                        // Banded by layer, coloured by the cell (or its ore).
                        for k in (low / 0.5).floor() as i32..(high / 0.5).ceil() as i32 {
                            let tone = if k & 1 == 0 { 1.0 } else { 0.9 };
                            mesh.polygon(&slab(&piece, h(k), h(k + 1)), normal, mix(stone(k), ore(a, k), 0.7) * tone * CUT);
                        }
                    }
                }
            }
        }
    }
    mesh.build()
}

/// The arena tiles (within `extent` of the arena's `center` tile) whose
/// cells or ore differ between two volumes.
pub(crate) fn changed_tiles(before: &Volume, after: &Volume, center: IVec2, extent: i32) -> Vec<IVec2> {
    // A merge of both volumes' sorted columns, then of their sorted ore: a
    // town's volume has some 66 000 columns, so no lookups per tile.
    let mut changed = std::collections::BTreeSet::new();
    let mut note = |tile: (i32, i32)| {
        let a = IVec2::new(tile.0, tile.1) - center;
        if a.x.abs() <= extent && a.y.abs() <= extent {
            changed.insert((a.y, a.x));
        }
    };
    let (mut old, mut new) = (before.columns().peekable(), after.columns().peekable());
    loop {
        match (old.peek(), new.peek()) {
            (None, None) => break,
            (Some(&(a, spans_a)), Some(&(b, spans_b))) if a == b => {
                if spans_a != spans_b {
                    note(a);
                }
                old.next();
                new.next();
            }
            (Some(&(a, _)), Some(&(b, _))) if a < b => {
                note(a);
                old.next();
            }
            (Some(&(a, _)), None) => {
                note(a);
                old.next();
            }
            (_, Some(&(b, _))) => {
                note(b);
                new.next();
            }
        }
    }
    let (mut old, mut new) = (before.ore_cells().peekable(), after.ore_cells().peekable());
    loop {
        match (old.peek(), new.peek()) {
            (None, None) => break,
            (Some(&(a, ma)), Some(&(b, mb))) if a == b => {
                if ma != mb {
                    note(a.0);
                }
                old.next();
                new.next();
            }
            (Some(&(a, _)), Some(&(b, _))) if a < b => {
                note(a.0);
                old.next();
            }
            (Some(&(a, _)), None) => {
                note(a.0);
                old.next();
            }
            (_, Some(&(b, _))) => {
                note(b.0);
                new.next();
            }
        }
    }
    changed.into_iter().map(|(y, x)| IVec2::new(x, y)).collect()
}

/// The lowest cell of `tile` solid in `before` and empty in `after`, if
/// any: where a dig's burst flies from.
pub(crate) fn lowest_cleared(before: &Volume, after: &Volume, tile: (i32, i32)) -> Option<i32> {
    before.spans(tile).iter().flat_map(|s| s.bottom..s.top).find(|&k| !after.is_solid(tile, k))
}

/// The height of the solid ground at view point `p` if `p` is in it: in a
/// solid cell and under its span's drawn top. `to_sim` maps view points to
/// the simulation's.
fn solid_top(volume: &Volume, p: Vec3, to_sim: &impl Fn(Vec3) -> FxVec2) -> Option<f32> {
    let s = to_sim(p);
    let tile = Terrain::pit_tile(s);
    let k = (p.y / 0.5).floor() as i32;
    let span = volume.spans(tile).iter().find(|s| s.bottom <= k && k < s.top)?;
    let [c00, c10, c01, c11] = top_corners(volume, tile, span.top);
    let (u, v) = (s.x.to_f32() + 0.5 - tile.0 as f32, s.y.to_f32() + 0.5 - tile.1 as f32);
    let near = c00 + (c10 - c00) * u;
    let far = c01 + (c11 - c01) * u;
    let top = near + (far - near) * v;
    (p.y <= top).then_some(top)
}

/// Where a ray (from the camera, through the cursor) first meets a volume's
/// ground, from over its highest column down: on a top, the point; on a
/// wall, the top over it.
pub(crate) fn ray_hit(volume: &Volume, ray: Ray3d, to_sim: impl Fn(Vec3) -> FxVec2) -> Option<Vec3> {
    let highest = volume.columns().filter_map(|(_, spans)| spans.last()).map(|s| h(s.top)).fold(0.0, f32::max) + 0.5;
    let lowest = volume.columns().filter_map(|(_, spans)| spans.first()).map(|s| h(s.bottom)).fold(0.0, f32::min) - 0.5;
    let plane = InfinitePlane3d::new(Vec3::Y);
    let start = ray.intersect_plane(Vec3::Y * highest, plane).unwrap_or(0.0);
    let end = ray.intersect_plane(Vec3::Y * lowest, plane)?;
    let steps = ((end - start) / 0.05).ceil().max(1.0) as usize;
    let mut previous = start;
    for n in 1..=steps {
        let t = start + (end - start) * n as f32 / steps as f32;
        if solid_top(volume, ray.get_point(t), &to_sim).is_some() {
            let (mut above, mut below) = (previous, t);
            for _ in 0..12 {
                let middle = (above + below) / 2.0;
                if solid_top(volume, ray.get_point(middle), &to_sim).is_some() {
                    below = middle;
                } else {
                    above = middle;
                }
            }
            let p = ray.get_point(below);
            return solid_top(volume, p, &to_sim).map(|top| p.with_y(top));
        }
        previous = t;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::{Indices, VertexAttributeValues};
    use fx::Fx;

    /// A mesh's triangles: corners and normal.
    fn triangles(mesh: &Mesh) -> Vec<([Vec3; 3], Vec3)> {
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("positions") };
        let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!("normals") };
        let Some(Indices::U32(indices)) = mesh.indices() else { panic!("indices") };
        indices
            .chunks(3)
            .map(|t| {
                let p = |i: u32| Vec3::from_array(positions[i as usize]);
                ([p(t[0]), p(t[1]), p(t[2])], Vec3::from_array(normals[t[0] as usize]))
            })
            .collect()
    }

    /// The mesh's area facing each way: +x, -x, +z, -z, up and down (tops
    /// count as up however they slope).
    fn areas(mesh: &Mesh) -> [f32; 6] {
        let mut out = [0.0; 6];
        for (t, normal) in triangles(mesh) {
            let cross = (t[1] - t[0]).cross(t[2] - t[0]);
            // Wound to face its normal.
            assert!(cross.dot(normal) > 0.0, "a face inside out: {t:?} {normal}");
            let a = cross.length() / 2.0;
            let i = if normal.y > 0.01 {
                4
            } else if normal.y < -0.01 {
                5
            } else if normal.x > 0.5 {
                0
            } else if normal.x < -0.5 {
                1
            } else if normal.z > 0.5 {
                2
            } else {
                3
            };
            out[i] += a;
        }
        out
    }

    fn assert_areas(mesh: &Mesh, expected: [f32; 6]) {
        let got = areas(mesh);
        for (g, e) in got.iter().zip(expected) {
            assert!((g - e).abs() < 1e-4, "areas {got:?}, expected {expected:?}");
        }
    }

    /// Chunk (0, 0) of a 16-tile arena round tile (0, 0) holds tiles -16 to -1:
    /// meshes here use an extent of 8, so chunk 0 holds tiles -8 to 7.
    fn mesh(volume: &Volume) -> Mesh {
        chunk_mesh(volume, IVec2::ZERO, 8, IVec2::ZERO)
    }

    #[test]
    fn a_single_cell_has_six_faces() {
        let mut volume = Volume::default();
        volume.fill((0, 0), 0);
        let m = mesh(&volume);
        assert_areas(&m, [0.5, 0.5, 0.5, 0.5, 1.0, 1.0]);
        // Its top is at the cell's top, level.
        assert!(triangles(&m).iter().filter(|(_, n)| n.y > 0.0).all(|(t, _)| t.iter().all(|p| p.y == 0.5)));
    }

    #[test]
    fn two_solids_side_by_side_share_no_face() {
        let mut volume = Volume::default();
        volume.fill((0, 0), 0);
        volume.fill((1, 0), 0);
        let m = mesh(&volume);
        // Each side's walls are only the outside ones: nothing on x = 0.5.
        assert_areas(&m, [0.5, 0.5, 1.0, 1.0, 2.0, 2.0]);
        assert!(triangles(&m).iter().all(|(t, n)| !(n.y == 0.0 && t.iter().all(|p| p.x == 0.5))), "a face between two solids");
        // One on the other: no top or ceiling between them.
        let mut stacked = Volume::default();
        stacked.fill((0, 0), 0);
        stacked.fill((0, 0), 1);
        assert_areas(&mesh(&stacked), [1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
    }

    /// A block 3 tiles wide with a tunnel along x through its middle row: a
    /// floor and a ceiling in the tunnel, and walls on both its sides.
    #[test]
    fn a_tunnel_has_floor_walls_and_ceiling() {
        let mut volume = Volume::default();
        volume.fill_box((-3, -1), (3, 1), -2, 8);
        volume.clear_box((-3, 0), (3, 0), 0, 4);
        let tris = triangles(&mesh(&volume));
        let inside = |p: &Vec3| p.x.abs() <= 3.5;
        // The floor at 0 m and the ceiling at 2 m over the middle row.
        let floor = tris.iter().filter(|(t, n)| n.y > 0.0 && t.iter().all(|p| p.y == 0.0 && p.z.abs() <= 0.5 && inside(p))).count();
        let ceiling = tris.iter().filter(|(t, n)| n.y < 0.0 && t.iter().all(|p| p.y == 2.0 && p.z.abs() <= 0.5)).count();
        // The walls face into the tunnel from z = ±0.5, 0 m to 2 m.
        let wall = |z: f32, nz: f32| tris.iter().filter(|(t, n)| n.z == nz && t.iter().all(|p| p.z == z && (0.0..=2.0).contains(&p.y))).map(|(t, _)| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0).sum::<f32>();
        assert_eq!(floor, 14, "two triangles a tile");
        assert_eq!(ceiling, 14);
        assert!((wall(0.5, -1.0) - 7.0 * 2.0).abs() < 1e-4, "the south wall");
        assert!((wall(-0.5, 1.0) - 7.0 * 2.0).abs() < 1e-4, "the north wall");
    }

    /// A ramp a layer a tile, climbing a tunnel and onto a hill: every drawn
    /// top's corners are the floor the simulation walks on there.
    #[test]
    fn sloped_floors_are_the_simulations() {
        let mut volume = Volume::default();
        volume.fill_box((-8, -8), (7, 7), -4, 0);
        for x in 0..6 {
            volume.fill_box((x, -2), (x, 2), 0, x + 1);
        }
        // A tunnel through the ramp's top, and a cave under a slab.
        volume.fill_box((-6, -6), (-2, -2), 0, 12);
        volume.clear_box((-5, -5), (-3, -3), 0, 5);
        volume.clear_box((0, -1), (5, 1), 2, 7);
        volume.fill_box((2, 0), (3, 0), 2, 3);
        let m = mesh(&volume);
        let mut checked = 0;
        let mut sloped = 0;
        for (t, n) in triangles(&m).into_iter().filter(|(_, n)| n.y > 0.0) {
            let middle = (t[0] + t[1] + t[2]) / 3.0;
            for p in t {
                // Nudged into the triangle, so the corner is this tile's.
                let q = p + (middle - p) * 0.01;
                let sim = volume.floor(FxVec2::new(Fx::quantize(q.x), Fx::quantize(q.z)), Fx::quantize(p.y)).map(|h| h.to_f32());
                let tile = Terrain::pit_tile(FxVec2::new(Fx::quantize(q.x), Fx::quantize(q.z)));
                let is_floor = volume.floors(tile).any(|f| (h(f) - p.y).abs() <= 0.5 && p.y <= h(f));
                if is_floor {
                    assert!(sim.is_some_and(|s| (s - p.y).abs() < 0.02), "at {p}: drawn {}, simulated {sim:?}", p.y);
                    checked += 1;
                }
            }
            if n.y < 0.999 {
                sloped += 1;
            }
        }
        assert!(checked > 200 && sloped > 10, "{checked} corners, {sloped} sloped");
    }

    /// The drawn ground is closed: its faces' area vectors sum to zero, so
    /// no wall stands up past a ramp (a fin) and none stops short of one (a
    /// crack), over ramps beside walls, a tunnel, a cave and a dug stair.
    #[test]
    fn the_surface_is_closed() {
        let mut volume = Volume::default();
        volume.fill_box((-7, -7), (7, 7), -4, 0);
        for x in 0..5 {
            volume.fill_box((x, -2), (x, 2), 0, x + 1);
        }
        volume.fill_box((0, 3), (4, 3), 0, 8);
        volume.fill_box((-6, -6), (-1, -2), 0, 12);
        volume.clear_box((-6, -5), (-1, -4), 0, 5);
        volume.clear_box((-4, -4), (-4, -4), -1, 3);
        // A stair a layer a step down, in the open.
        for (n, x) in (-6..=-3).enumerate() {
            volume.clear_box((x, 4), (x, 5), -(n as i32), 0);
        }
        let mut sum = Vec3::ZERO;
        for (t, _) in triangles(&mesh(&volume)) {
            sum += (t[1] - t[0]).cross(t[2] - t[0]);
        }
        assert!(sum.length() < 1e-3, "open by {sum}");
    }

    /// A dig re-meshes its own chunk and those its corner rule reaches:
    /// every other chunk's mesh is unchanged.
    #[test]
    fn a_dig_remeshes_only_the_dirty_chunks() {
        let extent = 24;
        let mut before = Volume::default();
        before.fill_box((-24, -24), (24, 24), -8, 4);
        let positions = |v: &Volume, key: IVec2| {
            let m = chunk_mesh(v, IVec2::ZERO, extent, key);
            let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION).cloned() else { panic!() };
            p
        };
        // Arena tile -24 is chunk 0's first; -9 its last, -8 chunk 1's first.
        for dug in [IVec2::new(-20, -20), IVec2::new(-9, -20), IVec2::new(-10, -10)] {
            let mut after = before.clone();
            for k in 2..4 {
                after.clear((dug.x, dug.y), k);
            }
            let changed = changed_tiles(&before, &after, IVec2::ZERO, extent);
            assert_eq!(changed, vec![dug]);
            assert_eq!(lowest_cleared(&before, &after, (dug.x, dug.y)), Some(2));
            let dirty = crate::digging::dirty_chunks(changed, extent, true);
            assert!(dirty.contains(&(dug + IVec2::splat(extent)).div_euclid(IVec2::splat(CHUNK)).to_array()));
            for y in 0..4 {
                for x in 0..4 {
                    let key = IVec2::new(x, y);
                    if !dirty.contains(&key.to_array()) {
                        assert_eq!(positions(&before, key), positions(&after, key), "dig at {dug} changed clean chunk {key}");
                    }
                }
            }
            let expected = match dug.x {
                -20 => 1,
                -9 => 2,
                _ => 4,
            };
            assert_eq!(dirty.len(), expected, "dig at {dug}: {dirty:?}");
        }
        // Ore moving counts too; nothing changing is no tile.
        let mut ore = before.clone();
        ore.set_ore((3, 3), 1, Some(1));
        assert_eq!(changed_tiles(&before, &ore, IVec2::ZERO, extent), vec![IVec2::new(3, 3)]);
        assert!(changed_tiles(&before, &before.clone(), IVec2::ZERO, extent).is_empty());
    }

    /// The cursor's ray finds the top of a hill higher than the old 6 m
    /// search start, a tunnel's floor through its open end, and a wall's top.
    #[test]
    fn the_ray_hits_the_volumes_ground() {
        let mut volume = Volume::default();
        volume.fill_box((-8, -8), (7, 7), -4, 0);
        volume.fill_box((0, -3), (5, 3), 0, 16);
        let to_sim = |p: Vec3| FxVec2::new(Fx::quantize(p.x), Fx::quantize(p.z));
        let down = |x: f32, z: f32| Ray3d::new(Vec3::new(x, 30.0, z), Dir3::NEG_Y);
        assert_eq!(ray_hit(&volume, down(2.0, 0.0), to_sim).map(|p| p.y), Some(8.0));
        assert_eq!(ray_hit(&volume, down(-3.0, 0.0), to_sim).map(|p| p.y), Some(0.0));
        // Sideways into the hill's wall at 4 m: the top over it.
        let side = Ray3d::new(Vec3::new(-6.0, 4.0, 0.0), Dir3::X);
        let hit = ray_hit(&volume, Ray3d::new(side.origin, Dir3::new(Vec3::new(1.0, -0.01, 0.0)).unwrap()), to_sim).unwrap();
        assert!((hit.x + 0.5).abs() < 0.06 && hit.y == 8.0, "{hit}");
    }

    /// The Sacred Tree's ground as a volume (M8.3 of the depths design):
    /// its 257 x 257 columns, 17 x 17 chunks, mesh at startup in well under
    /// two seconds, and diffing two of its volumes after a dig finds only
    /// the dug tile.
    #[test]
    fn the_towns_volume_meshes_quickly() {
        let mut terrain = Terrain::flat(FxVec2::ZERO, Fx::from_int(128));
        terrain.max_layers = 16;
        terrain.generate_mine(7, FxVec2::new(Fx::ZERO, Fx::from_ratio(17, 5)), sim::mine::Veins::Stone);
        let hill = sim::mine::Hill { center: (0, 0), plateau: 15, steps: 8, ramp_width: 7, ramp_length: 8, foot: 10 };
        terrain.raise_hill(hill).unwrap();
        terrain.volume_from_mine(None).unwrap();
        let volume = terrain.volume.clone().unwrap();
        let extent = 128;
        let count = (2 * extent + 1 + CHUNK - 1) / CHUNK;
        let started = std::time::Instant::now();
        let mut triangles = 0;
        for y in 0..count {
            for x in 0..count {
                let mesh = chunk_mesh_with(&volume, IVec2::ZERO, extent, IVec2::new(x, y), Some(-4));
                triangles += mesh.indices().map_or(0, |i| i.len() / 3);
            }
        }
        let took = started.elapsed();
        eprintln!("the town's {} chunks: {triangles} triangles in {took:?}", count * count);
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
        let mut dug = volume.clone();
        let top = dug.top((20, -30)).unwrap();
        assert!(dug.clear((20, -30), top - 1));
        let started = std::time::Instant::now();
        assert_eq!(changed_tiles(&volume, &dug, IVec2::ZERO, extent), vec![IVec2::new(20, -30)]);
        eprintln!("a dig's diff over the town in {:?}", started.elapsed());
    }
}

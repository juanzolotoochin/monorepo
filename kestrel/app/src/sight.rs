//! Line of sight on the player's level (the "seeing inside" prototype,
//! `todo/verticality.md`): which tiles round the player they can see from
//! head height, as a 0..1 value per tile, for the cutaway shader to light
//! what is in sight and dim what isn't while the player is inside.
//!
//! View only and pure: it reads the volume, keeps nothing the simulation
//! sees. A tile is clear when its cell at the eye layer (`EYE` layers over
//! the player's floor, 1.5 m to 2 m over the feet) is empty, so a step or a
//! low ledge doesn't block the view but a wall does. From the eye tile's
//! middle (and four points round it, `EYE_SAMPLES`) a ray goes to the
//! middle and four corners of every tile in a square `WINDOW` round it
//! (`TARGET_SAMPLES`); a ray reaches its tile if every tile it crosses on
//! the way is clear. The target itself needn't be. A tile's value is the
//! share of its rays that reach it, so the view out of a mouth is a wedge
//! whose sides are a soft band; a solid tile is also as visible as the
//! clear tiles beside it (its face toward a floor in sight); then each tile takes at least `SPREAD` of the
//! brightest of its eight neighbours (a one-tile outward glow, never
//! darkening what is in sight). Recomputed only when its `Key` changes:
//! the eye tile, the floor layer, or the terrain's revision.

/// The window's side, in tiles: the whole Tunnel Lab (64 m) round the eye.
pub const WINDOW: i32 = 64;
/// Layers over the floor of the cell a ray must find empty: layer 3 is
/// 1.5 m to 2 m over the feet, the head.
pub const EYE: i32 = 3;
/// Where rays start, in tiles from the eye tile's middle.
pub const EYE_SAMPLES: [(f32, f32); 5] = [(0.0, 0.0), (-0.3, -0.3), (0.3, -0.3), (-0.3, 0.3), (0.3, 0.3)];
/// Where rays end, in tiles from a target tile's middle.
pub const TARGET_SAMPLES: [(f32, f32); 5] = [(0.0, 0.0), (-0.4, -0.4), (0.4, -0.4), (-0.4, 0.4), (0.4, 0.4)];
/// How much of its brightest neighbour a tile gets (the soft edge's glow).
pub const SPREAD: f32 = 0.5;

/// What a visibility grid was computed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// The eye's tile (`Terrain::pit_tile`).
    pub tile: (i32, i32),
    /// The floor layer the player stands on.
    pub floor: i32,
    /// `Terrain::revision` when computed.
    pub revision: u64,
}

/// Per-tile visibility over a square window.
#[derive(Clone, Debug, PartialEq)]
pub struct Sight {
    pub key: Key,
    /// The window's lowest tile.
    pub origin: (i32, i32),
    /// Its side in tiles.
    pub size: i32,
    /// Row by row (y, then x): 0 out of sight, 1 fully in sight.
    pub values: Vec<f32>,
}

impl Sight {
    /// The visibility of `tile`: 0 outside the window.
    pub fn at(&self, tile: (i32, i32)) -> f32 {
        let (x, y) = (tile.0 - self.origin.0, tile.1 - self.origin.1);
        if x < 0 || y < 0 || x >= self.size || y >= self.size {
            return 0.0;
        }
        self.values[(y * self.size + x) as usize]
    }

    /// The values as bytes (an R8 texture's rows).
    pub fn bytes(&self) -> Vec<u8> {
        self.values.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect()
    }

    /// Whether a grid computed for `key` would differ: the eye moved a
    /// tile or a level, or the ground changed.
    pub fn stale(this: Option<&Sight>, key: Key) -> bool {
        this.is_none_or(|sight| sight.key != key)
    }
}

/// Whether `tile` blocks the view at the eye over floor layer `floor`.
pub fn blocks(volume: &sim::volume::Volume, tile: (i32, i32), floor: i32) -> bool {
    volume.is_solid(tile, floor + EYE)
}

/// The visibility round `key.tile` over a `size` window, with `opaque`
/// telling which tiles block a ray.
pub fn compute(key: Key, size: i32, opaque: impl Fn((i32, i32)) -> bool) -> Sight {
    let origin = (key.tile.0 - size / 2, key.tile.1 - size / 2);
    let n = size as usize;
    let index = |x: i32, y: i32| (y as usize) * n + x as usize;
    let mut solid = vec![false; n * n];
    for y in 0..size {
        for x in 0..size {
            solid[index(x, y)] = opaque((origin.0 + x, origin.1 + y));
        }
    }
    // Window-relative tile coordinates; tile t spans [t, t + 1).
    let eye = ((key.tile.0 - origin.0) as f32 + 0.5, (key.tile.1 - origin.1) as f32 + 0.5);
    let clear = |x: i32, y: i32| x < 0 || y < 0 || x >= size || y >= size || !solid[index(x, y)];
    let mut raw = vec![0.0; n * n];
    let rays = (EYE_SAMPLES.len() * TARGET_SAMPLES.len()) as f32;
    for y in 0..size {
        for x in 0..size {
            let mut seen = 0;
            for &(ex, ey) in &EYE_SAMPLES {
                let from = (eye.0 + ex, eye.1 + ey);
                for &(tx, ty) in &TARGET_SAMPLES {
                    let to = (x as f32 + 0.5 + tx, y as f32 + 0.5 + ty);
                    if reaches(from, to, &clear) {
                        seen += 1;
                    }
                }
            }
            raw[index(x, y)] = seen as f32 / rays;
        }
    }
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && x < size && y < size;
    // A solid tile is as visible as the clearest clear tile beside it: its
    // face toward that floor is.
    let mut lit = raw.clone();
    for y in 0..size {
        for x in 0..size {
            if !solid[index(x, y)] {
                continue;
            }
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if inside(nx, ny) && !solid[index(nx, ny)] {
                    lit[index(x, y)] = lit[index(x, y)].max(raw[index(nx, ny)]);
                }
            }
        }
    }
    let mut values = lit.clone();
    for y in 0..size {
        for x in 0..size {
            let mut brightest: f32 = 0.0;
            for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                if inside(x + dx, y + dy) {
                    brightest = brightest.max(lit[index(x + dx, y + dy)]);
                }
            }
            let v = &mut values[index(x, y)];
            *v = v.max(brightest * SPREAD);
        }
    }
    Sight { key, origin, size, values }
}

/// Whether the segment `from`..`to` crosses only clear tiles before the
/// tile holding `to` (a grid walk, Amanatides and Woo).
fn reaches(from: (f32, f32), to: (f32, f32), clear: &impl Fn(i32, i32) -> bool) -> bool {
    let (mut x, mut y) = (from.0.floor() as i32, from.1.floor() as i32);
    let end = (to.0.floor() as i32, to.1.floor() as i32);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let step_x = if dx > 0.0 { 1 } else { -1 };
    let step_y = if dy > 0.0 { 1 } else { -1 };
    let delta_x = if dx != 0.0 { (1.0 / dx).abs() } else { f32::INFINITY };
    let delta_y = if dy != 0.0 { (1.0 / dy).abs() } else { f32::INFINITY };
    let mut max_x = if dx > 0.0 { (x as f32 + 1.0 - from.0) * delta_x } else if dx < 0.0 { (from.0 - x as f32) * delta_x } else { f32::INFINITY };
    let mut max_y = if dy > 0.0 { (y as f32 + 1.0 - from.1) * delta_y } else if dy < 0.0 { (from.1 - y as f32) * delta_y } else { f32::INFINITY };
    // At most one step per tile boundary crossed.
    let mut left = (end.0 - x).abs() + (end.1 - y).abs();
    while (x, y) != end && left > 0 {
        if (x, y) != (from.0.floor() as i32, from.1.floor() as i32) && !clear(x, y) {
            return false;
        }
        if max_x < max_y {
            max_x += delta_x;
            x += step_x;
        } else {
            max_y += delta_y;
            y += step_y;
        }
        left -= 1;
    }
    true
}

/// How fast the inside factor moves: from 0 to 1 in `FADE` seconds.
pub const FADE: f32 = 0.3;

/// The inside factor one frame of `dt` seconds on, toward 1 under a
/// ceiling and 0 in the open, at a constant rate (0 to 1 in [`FADE`]).
pub fn ease(factor: f32, inside: bool, dt: f32) -> f32 {
    let target = if inside { 1.0 } else { 0.0 };
    let step = dt / FADE;
    if factor < target { (factor + step).min(target) } else { (factor - step).max(target) }
}

/// The factor as drawn: smoothed at both ends, so the fade starts and
/// ends softly.
pub fn shown(factor: f32) -> f32 {
    let f = factor.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// A map from rows of text, row 0 at y = 0: `#` blocks, anything else
    /// is clear. `@` is the eye.
    struct Map {
        walls: BTreeSet<(i32, i32)>,
        eye: (i32, i32),
    }

    fn map(rows: &[&str]) -> Map {
        let mut walls = BTreeSet::new();
        let mut eye = (0, 0);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                match c {
                    '#' => {
                        walls.insert((x as i32, y as i32));
                    }
                    '@' => eye = (x as i32, y as i32),
                    _ => {}
                }
            }
        }
        Map { walls, eye }
    }

    fn sight(map: &Map, size: i32) -> Sight {
        let key = Key { tile: map.eye, floor: 0, revision: 0 };
        compute(key, size, |t| map.walls.contains(&t))
    }

    #[test]
    fn a_straight_tunnel_is_visible_end_to_end() {
        let m = map(&[
            "##############################",
            "#@...........................#",
            "##############################",
        ]);
        let s = sight(&m, 64);
        for x in 1..29 {
            assert_eq!(s.at((x, 1)), 1.0, "tunnel floor at x {x}");
        }
        // Its walls and the far end wall, beside floor in sight, show.
        assert!(s.at((15, 0)) > 0.5 && s.at((15, 2)) > 0.5 && s.at((29, 1)) > 0.5);
    }

    #[test]
    fn a_room_round_a_corner_is_not_visible() {
        let m = map(&[
            "#############",
            "#@......#####",
            "#######.#####",
            "#######.#####",
            "#######.....#",
            "#######.....#",
            "#######.....#",
            "#############",
        ]);
        let s = sight(&m, 64);
        // Down the corridor's first leg, and into the bend.
        assert_eq!(s.at((5, 1)), 1.0);
        assert!(s.at((7, 1)) > 0.9);
        // The room's far side, round the corner: dark.
        for (x, y) in [(10, 5), (11, 5), (11, 6), (10, 6), (11, 4)] {
            assert_eq!(s.at((x, y)), 0.0, "room tile ({x}, {y})");
        }
    }

    /// A tunnel opening east onto open ground: the view out of its mouth.
    fn mouth() -> Map {
        let mut rows = vec![String::new(); 41];
        for (y, row) in rows.iter_mut().enumerate() {
            for x in 0..60 {
                let tunnel = (19..=21).contains(&y);
                let c = if x < 20 && !tunnel {
                    '#'
                } else if x == 2 && y == 20 {
                    '@'
                } else if x < 1 {
                    '#'
                } else {
                    '.'
                };
                row.push(c);
            }
        }
        let rows: Vec<&str> = rows.iter().map(|r| r.as_str()).collect();
        map(&rows)
    }

    fn lit_width(s: &Sight, x: i32) -> usize {
        (0..41).filter(|&y| s.at((x, y)) > 0.5).count()
    }

    #[test]
    fn the_view_out_of_a_mouth_widens_with_distance_and_has_a_soft_edge() {
        let m = mouth();
        let s = sight(&m, 128);
        let (near, far) = (lit_width(&s, 24), lit_width(&s, 40));
        assert!(near >= 3, "the mouth's width just outside: {near}");
        assert!(far > near + 2, "the wedge widens: {near} then {far}");
        // Out of the wedge's sides, dark.
        assert_eq!(s.at((40, 2)), 0.0);
        assert_eq!(s.at((40, 38)), 0.0);
        // Its sides are a band of part values, not a hard step.
        let soft = (0..41).filter(|&y| (0.05..0.95).contains(&s.at((40, y)))).count();
        assert!(soft >= 2, "soft tiles at x 40: {soft}");
    }

    #[test]
    fn solid_walls_bordering_visible_floor_are_visible() {
        let m = map(&[
            "#########",
            "#.......#",
            "#...@...#",
            "#.......#",
            "#########",
        ]);
        let s = sight(&m, 32);
        for x in 1..8 {
            assert!(s.at((x, 0)) > 0.9 && s.at((x, 4)) > 0.9, "the room's wall at x {x}");
        }
        // Behind a wall, the next solid tile only glows (`SPREAD`).
        let m = map(&["##########", "#@.......#", "##########", "##########"]);
        let s = sight(&m, 32);
        assert!(s.at((5, 2)) > 0.9 && s.at((5, 3)) <= SPREAD + 1e-6);
    }

    #[test]
    fn a_grid_is_recomputed_only_when_its_key_changes() {
        let m = map(&["#####", "#.@.#", "#####"]);
        let key = Key { tile: m.eye, floor: 0, revision: 7 };
        let s = compute(key, 16, |t| m.walls.contains(&t));
        assert!(!Sight::stale(Some(&s), key));
        assert!(Sight::stale(None, key));
        assert!(Sight::stale(Some(&s), Key { tile: (3, 1), ..key }));
        assert!(Sight::stale(Some(&s), Key { floor: 1, ..key }));
        assert!(Sight::stale(Some(&s), Key { revision: 8, ..key }));
    }

    #[test]
    fn the_eye_band_is_the_head_not_the_feet() {
        let mut volume = sim::volume::Volume::default();
        // A step a layer up (layer 0) doesn't block; a wall to the head does.
        volume.fill((1, 0), 0);
        volume.fill_box((2, 0), (2, 0), 0, EYE + 1);
        assert!(!blocks(&volume, (1, 0), 0));
        assert!(blocks(&volume, (2, 0), 0));
        // From a floor two layers up, the same wall's top is under the eye.
        assert!(!blocks(&volume, (2, 0), 2));
    }

    /// The Tunnel Lab's volume, in scene tiles.
    fn tunnel_lab() -> sim::volume::Volume {
        let pack = voxel_assets::ScenePack::decode(tunnel_lab::PACK_BYTES).expect("the Tunnel Lab pack");
        pack.scene.volume.as_ref().expect("a volume").build()
    }

    /// Rows of the grid round `tile` as text, for failure messages.
    fn picture(s: &Sight, from: (i32, i32), to: (i32, i32)) -> String {
        (from.1..=to.1)
            .map(|y| (from.0..=to.0).map(|x| [' ', '.', ':', '#'][(s.at((x, y)) * 3.0).round() as usize]).collect::<String>())
            .collect::<Vec<_>>()
            .join("
")
    }

    #[test]
    fn in_the_tunnel_lab_the_tunnel_and_its_mouths_are_in_sight() {
        let volume = tunnel_lab();
        let key = Key { tile: (12, 0), floor: 0, revision: 0 };
        let s = compute(key, WINDOW, |t| blocks(&volume, t, 0));
        let shown = picture(&s, (-6, -12), (32, 12));
        for x in 4..=24 {
            assert_eq!(s.at((x, 0)), 1.0, "the tunnel at x {x}:
{shown}");
        }
        // Out of both mouths, straight on; not into the mountain's middle.
        assert!(s.at((0, 0)) > 0.9 && s.at((28, 0)) > 0.9, "the mouths:
{shown}");
        assert_eq!(s.at((12, 6)), 0.0, "inside the mountain:
{shown}");
        assert_eq!(s.at((0, 10)), 0.0, "round the mountain:
{shown}");
    }

    #[test]
    fn the_inside_factor_eases_over_the_fade() {
        let dt = 1.0 / 60.0;
        let mut f = 0.0;
        for _ in 0..9 {
            f = ease(f, true, dt);
        }
        assert!((f - 0.5).abs() < 0.01, "half way at 0.15 s: {f}");
        for _ in 0..20 {
            f = ease(f, true, dt);
        }
        assert_eq!(f, 1.0, "all the way in by 0.3 s and held");
        f = ease(f, false, dt);
        assert!(f < 1.0 && f > 0.9);
        for _ in 0..30 {
            f = ease(f, false, dt);
        }
        assert_eq!(f, 0.0);
        // Drawn with soft ends: slow at 0 and 1, the same half way.
        assert!(shown(0.1) < 0.1 && shown(0.9) > 0.9);
        assert!((shown(0.5) - 0.5).abs() < 1e-6);
        // A long frame doesn't overshoot.
        assert_eq!(ease(0.2, true, 1.0), 1.0);
    }
}

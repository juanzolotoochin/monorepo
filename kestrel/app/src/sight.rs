//! Line of sight on the player's level (the "seeing inside" prototype,
//! `todo/verticality.md`): which tiles round the player they can see from
//! head height, as a 0..1 value per tile, for the cutaway shader to light
//! what is in sight and dim what isn't while the player is inside.
//!
//! View only and pure: it reads the volume, keeps nothing the simulation
//! sees. A tile is clear when its cell at the eye layer (`EYE` layers over
//! the player's floor, 1.5 m to 2 m over the feet) or the one under it is
//! empty, so a step, a low ledge or the next step down a stair doesn't
//! block the view but a wall does. From the eye tile's
//! middle (and four points round it, `EYE_SAMPLES`) a ray goes to the
//! middle and four corners of every tile in a square `WINDOW` round it
//! (`TARGET_SAMPLES`); a ray reaches its tile if every tile it crosses on
//! the way is clear. The target itself needn't be. A tile's value is the
//! share of its rays that reach it, so the view out of a mouth is a wedge
//! whose sides are a soft band; a solid tile is also as visible as the
//! clear tiles beside it (its face toward a floor in sight); then each tile takes at least `SPREAD` of the
//! brightest of its eight neighbours (a one-tile outward glow, never
//! darkening what is in sight). Recomputed only when its `Key` changes:
//! the eye tile, the floor layer, the terrain's revision or the light's.
//!
//! M8.5 (T7 of the depths design) adds three things per tile, for the
//! shader to draw rooms as thin-walled boxes in a void:
//! - **near** (`Sight::near`): how close the tile is to floor in sight: the
//!   best ray share of the clear tiles in its 3 x 3 (itself and its eight
//!   neighbours), over `NEAR_FULL`. Floor in sight and the one-tile rim of
//!   rock round it are 1; rock and ground further off are 0 and drawn as
//!   near-black void ([`void`]).
//! - **open** (`Sight::open`): 1 where nothing solid is over the eye layer
//!   (sky over it), 0 under a roof or in rock. The shader lights roofed
//!   ground by the elder's lamp, an ambient and the light grid, not the sun.
//! - **light** (`Sight::light`): 0..1 per tile from lanterns, all zero
//!   until M8.6's light grid fills it (`Sight::set_light`); the shader adds
//!   it to roofed ground (`cutaway::underground`).
//!
//! And whether the eye is **deep** inside (`Sight::deep`): under a roof the
//! view goes inside only when the roofed floor in sight round the eye is a
//! room (`ROOF_AREA` tiles within `ROOF_TILES`) or no open sky is in sight
//! within `SKY_TILES` tiles. A stair dug down from the surface is one tile
//! wide with sky a step or two behind, so it stays outside (with a notch
//! cut over the head, `cutaway`) until the sky is three tiles back; a cave
//! mouth or a wide tunnel goes inside at once.

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
/// The ray share at which a clear tile counts fully as floor in sight for
/// [`Sight::near`]: a tile at a wedge's soft side reached by a few rays is
/// partly near.
pub const NEAR_FULL: f32 = 0.2;
/// The ray share at which a clear tile counts as in sight for `deep`.
pub const SEEN: f32 = 0.5;
/// The square round the eye (this many tiles each way) whose roofed floor
/// in sight is counted for `deep`...
pub const ROOF_TILES: i32 = 2;
/// ...and how many such tiles make a room, not a stair's own roof.
pub const ROOF_AREA: usize = 8;
/// Open sky in sight this close (tiles, each way) keeps a small roof
/// outside.
pub const SKY_TILES: i32 = 2;

/// What a visibility grid was computed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// The eye's tile (`Terrain::pit_tile`).
    pub tile: (i32, i32),
    /// The floor layer the player stands on.
    pub floor: i32,
    /// `Terrain::revision` when computed.
    pub revision: u64,
    /// The light grid's revision (M8.6), 0 until there is one.
    pub light: u64,
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
    /// Row by row: how near floor in sight (1 the floor and its rim, 0
    /// void).
    pub near: Vec<f32>,
    /// Row by row: 1 under open sky, 0 roofed or rock.
    pub open: Vec<f32>,
    /// Row by row: lanterns' light, 0..1 (all zero until M8.6).
    pub light: Vec<f32>,
    /// Whether the eye is deep enough under a roof for the inside view.
    pub deep: bool,
}

impl Sight {
    /// The visibility of `tile`: 0 outside the window.
    pub fn at(&self, tile: (i32, i32)) -> f32 {
        self.index(tile).map_or(0.0, |i| self.values[i])
    }

    /// How near `tile` is to floor in sight: 1 floor in sight or its rim,
    /// 0 void (and outside the window).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn near(&self, tile: (i32, i32)) -> f32 {
        self.index(tile).map_or(0.0, |i| self.near[i])
    }

    /// Whether `tile` is under open sky (1) or roofed (0); 1 outside the
    /// window.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn open(&self, tile: (i32, i32)) -> f32 {
        self.index(tile).map_or(1.0, |i| self.open[i])
    }

    /// Lanterns' light on `tile`, 0..1.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn light(&self, tile: (i32, i32)) -> f32 {
        self.index(tile).map_or(0.0, |i| self.light[i])
    }

    /// Fills the light channel from a light grid (M8.6's hook): `light`
    /// gives 0..1 per tile. Call it after computing the grid and again
    /// when the light grid's revision (`Key::light`) changes.
    #[cfg_attr(not(test), allow(dead_code))] // M8.6 fills the channel with it.
    pub fn set_light(&mut self, light: impl Fn((i32, i32)) -> f32) {
        for y in 0..self.size {
            for x in 0..self.size {
                self.light[(y * self.size + x) as usize] = light((self.origin.0 + x, self.origin.1 + y)).clamp(0.0, 1.0);
            }
        }
    }

    fn index(&self, tile: (i32, i32)) -> Option<usize> {
        let (x, y) = (tile.0 - self.origin.0, tile.1 - self.origin.1);
        (x >= 0 && y >= 0 && x < self.size && y < self.size).then(|| (y * self.size + x) as usize)
    }

    /// The grid as an RGBA8 texture's rows: visibility, near, light, open.
    pub fn bytes(&self) -> Vec<u8> {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        (0..self.values.len()).flat_map(|i| [byte(self.values[i]), byte(self.near[i]), byte(self.light[i]), byte(self.open[i])]).collect()
    }

    /// Whether a grid computed for `key` would differ: the eye moved a
    /// tile or a level, or the ground changed.
    pub fn stale(this: Option<&Sight>, key: Key) -> bool {
        this.is_none_or(|sight| sight.key != key)
    }
}

/// Whether `tile` blocks the view at the eye over floor layer `floor`: its
/// cells at the eye layer and the one under it are both solid (1 m to 2 m
/// over the feet), so the next step down a stair is in sight.
pub fn blocks(volume: &sim::volume::Volume, tile: (i32, i32), floor: i32) -> bool {
    volume.is_solid(tile, floor + EYE) && volume.is_solid(tile, floor + EYE - 1)
}

/// Whether `tile` is under open sky from the eye over floor layer `floor`:
/// nothing solid at or over the eye layer.
pub fn open(volume: &sim::volume::Volume, tile: (i32, i32), floor: i32) -> bool {
    volume.top(tile).is_none_or(|top| top <= floor + EYE)
}

/// The visibility round `key.tile` over a `size` window, with `opaque`
/// telling which tiles block a ray; nothing is open sky (see
/// [`compute_with`]).
#[cfg_attr(not(test), allow(dead_code))]
pub fn compute(key: Key, size: i32, opaque: impl Fn((i32, i32)) -> bool) -> Sight {
    compute_with(key, size, opaque, |_| false)
}

/// The volume's grid round `key` (its tile and floor): [`blocks`] and
/// [`open`] over [`WINDOW`].
pub fn of_volume(volume: &sim::volume::Volume, key: Key) -> Sight {
    compute_with(key, WINDOW, |t| blocks(volume, t, key.floor), |t| open(volume, t, key.floor))
}

/// The visibility round `key.tile` over a `size` window, with `opaque`
/// telling which tiles block a ray and `sky` which are under open sky.
pub fn compute_with(key: Key, size: i32, opaque: impl Fn((i32, i32)) -> bool, sky: impl Fn((i32, i32)) -> bool) -> Sight {
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
    // Near: the best ray share of the clear tiles round each, so floor in
    // sight and the rock bordering it are 1 and the rest fades to void.
    let mut near = vec![0.0; n * n];
    for y in 0..size {
        for x in 0..size {
            let mut best: f32 = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if inside(x + dx, y + dy) && !solid[index(x + dx, y + dy)] {
                        best = best.max(raw[index(x + dx, y + dy)]);
                    }
                }
            }
            near[index(x, y)] = (best / NEAR_FULL).min(1.0);
        }
    }
    let mut open = vec![0.0; n * n];
    for y in 0..size {
        for x in 0..size {
            if !solid[index(x, y)] && sky((origin.0 + x, origin.1 + y)) {
                open[index(x, y)] = 1.0;
            }
        }
    }
    // Deep: a room's roof round the eye, or no sky in sight close by.
    let (ex, ey) = (key.tile.0 - origin.0, key.tile.1 - origin.1);
    let (mut roofed, mut sky_near) = (0, false);
    let reach = ROOF_TILES.max(SKY_TILES);
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let (x, y) = (ex + dx, ey + dy);
            if !inside(x, y) || solid[index(x, y)] || raw[index(x, y)] < SEEN {
                continue;
            }
            let ring = dx.abs().max(dy.abs());
            if open[index(x, y)] > 0.5 {
                sky_near |= ring <= SKY_TILES;
            } else if ring <= ROOF_TILES {
                roofed += 1;
            }
        }
    }
    let deep = roofed >= ROOF_AREA || !sky_near;
    Sight { key, origin, size, values, near, open, light: vec![0.0; n * n], deep }
}

/// How much of the ground at a tile `near` (see [`Sight::near`]) is drawn
/// as void while inside: none at floor in sight and its rim, all of it
/// where no floor in sight is a tile off. The shader's `void_at` mirrors
/// it.
#[cfg_attr(not(test), allow(dead_code))] // Mirrored by the shader; tests check it.
pub fn void(near: f32) -> f32 {
    1.0 - near.clamp(0.0, 1.0)
}

/// Whether the segment `from`..`to` crosses only clear tiles before the
/// tile holding `to` (a grid walk, Amanatides and Woo). The fog's rays
/// too (`fog::look`).
pub fn reaches(from: (f32, f32), to: (f32, f32), clear: &impl Fn(i32, i32) -> bool) -> bool {
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
        let key = Key { tile: map.eye, floor: 0, revision: 0, light: 0 };
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
        let key = Key { tile: m.eye, floor: 0, revision: 7, light: 0 };
        let s = compute(key, 16, |t| m.walls.contains(&t));
        assert!(!Sight::stale(Some(&s), key));
        assert!(Sight::stale(None, key));
        assert!(Sight::stale(Some(&s), Key { tile: (3, 1), ..key }));
        assert!(Sight::stale(Some(&s), Key { floor: 1, ..key }));
        assert!(Sight::stale(Some(&s), Key { revision: 8, ..key }));
        assert!(Sight::stale(Some(&s), Key { light: 1, ..key }));
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
        let key = Key { tile: (12, 0), floor: 0, revision: 0, light: 0 };
        let s = of_volume(&volume, key);
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

    #[test]
    fn rock_beyond_a_one_tile_rim_round_what_you_see_is_void() {
        let m = map(&[
            "###########",
            "###########",
            "#...#.....#",
            "#.@.#.....#",
            "#...#.....#",
            "###########",
            "###########",
        ]);
        let s = sight(&m, 32);
        // The floor in sight, and the rim of rock round it (corners too).
        for (x, y) in [(1, 2), (3, 4), (2, 3), (4, 3), (0, 3), (2, 1), (2, 5), (0, 1), (4, 5)] {
            assert_eq!(s.near((x, y)), 1.0, "({x}, {y}) is near");
            assert_eq!(void(s.near((x, y))), 0.0);
        }
        // Rock a tile further out, and the room behind the wall: void.
        for (x, y) in [(2, 0), (2, 6), (0, 0), (5, 3), (7, 3), (9, 2)] {
            assert_eq!(s.near((x, y)), 0.0, "({x}, {y}) is void");
            assert_eq!(void(s.near((x, y))), 1.0);
        }
        // Outside the window: void.
        assert_eq!(s.near((100, 100)), 0.0);
    }

    #[test]
    fn a_tile_at_a_wedge_side_is_partly_near() {
        let m = mouth();
        let s = sight(&m, 128);
        // Out of the mouth: the wedge's middle is near, far off its sides void.
        assert_eq!(s.near((40, 20)), 1.0);
        assert_eq!(s.near((40, 2)), 0.0);
        let soft = (0..41).filter(|&y| (0.01..0.99).contains(&s.near((40, y)))).count();
        assert!(soft >= 1, "a soft side at x 40");
    }

    /// Ground 10 m deep with its top at layer 0, and a stair dug down from
    /// it along +x: tile x = k has its floor at layer -k and `HEADROOM`
    /// clear over it, so the first four steps are open to the sky and the
    /// fifth on are a roofed descending tunnel.
    fn stair() -> sim::volume::Volume {
        let mut volume = sim::volume::Volume::default();
        volume.fill_box((-12, -12), (30, 12), -20, 0);
        for k in 1..=14 {
            volume.clear_box((k, 0), (k, 0), -k, -k + sim::volume::HEADROOM);
        }
        volume
    }

    fn deep(volume: &sim::volume::Volume, tile: (i32, i32), floor: i32) -> bool {
        of_volume(volume, Key { tile, floor, revision: 0, light: 0 }).deep
    }

    #[test]
    fn a_shallow_stair_stays_outside_and_a_tunnel_goes_inside() {
        let volume = stair();
        // Under the first roofed steps the sky is a step or two behind.
        assert!(!deep(&volume, (5, 0), -5), "2.5 m down the stair");
        assert!(!deep(&volume, (6, 0), -6), "3 m down the stair");
        // Further down the sky is out of sight.
        assert!(deep(&volume, (8, 0), -8), "4 m down the stair");
        assert!(deep(&volume, (12, 0), -12), "6 m down the stair");

        // A mountain on flat ground with a room 5 tiles wide and a tunnel
        // one tile wide going in from its west face.
        let mut volume = sim::volume::Volume::default();
        volume.fill_box((-12, -12), (30, 30), -10, 0);
        volume.fill_box((0, -6), (12, 20), 0, 8);
        volume.clear_box((0, -2), (6, 2), 0, 6);
        volume.clear_box((0, 12), (10, 12), 0, 6);
        // The room goes inside at its mouth: its roof is a room's.
        assert!(deep(&volume, (0, 0), 0), "the room's mouth");
        assert!(deep(&volume, (3, 0), 0), "in the room");
        // The narrow tunnel only once the mouth is three tiles back.
        assert!(!deep(&volume, (0, 12), 0), "the tunnel's mouth");
        assert!(!deep(&volume, (1, 12), 0), "a tile in");
        assert!(deep(&volume, (2, 12), 0), "two tiles in: the sky is three back");
        // Out in the open nothing is deep... but `deep` only matters under a
        // roof (`cutaway::follow` checks the ceiling first); here the sky is
        // all round.
        assert!(!deep(&volume, (-5, 0), 0));
    }

    #[test]
    fn down_a_stair_the_next_step_is_in_sight() {
        let volume = stair();
        let s = of_volume(&volume, Key { tile: (8, 0), floor: -8, revision: 0, light: 0 });
        assert_eq!(s.at((9, 0)), 1.0, "the next step down");
        assert!(s.at((7, 0)) > 0.9 && s.at((6, 0)) > 0.9, "the steps up");
        // The step after next is rock at the eye: the rim, then void.
        assert_eq!(s.near((10, 0)), 1.0);
        assert_eq!(s.near((12, 0)), 0.0);
    }

    #[test]
    fn the_light_channel_is_zero_until_a_light_grid_fills_it() {
        let m = map(&["#######", "#..@..#", "#######"]);
        let mut s = sight(&m, 16);
        assert!(s.light.iter().all(|&l| l == 0.0));
        // RGBA: visibility, near, light, open; light all zero.
        let bytes = s.bytes();
        assert_eq!(bytes.len(), (16 * 16 * 4) as usize);
        assert!(bytes.chunks(4).all(|texel| texel[2] == 0));
        let eye = s.index(m.eye).unwrap();
        assert_eq!(&bytes[eye * 4..eye * 4 + 4], &[255, 255, 0, 0], "the eye tile: seen, near, unlit, roofed");
        // M8.6's grid fills it per tile, clamped.
        s.set_light(|(x, _)| if x == 2 { 0.5 } else if x == 4 { 3.0 } else { 0.0 });
        assert_eq!(s.light((2, 1)), 0.5);
        assert_eq!(s.light((4, 1)), 1.0);
        assert_eq!(s.light((3, 1)), 0.0);
        assert_eq!(s.bytes()[s.index((2, 1)).unwrap() * 4 + 2], 128);
    }

    #[test]
    fn open_sky_is_where_nothing_is_over_the_eye() {
        let volume = stair();
        let s = of_volume(&volume, Key { tile: (5, 0), floor: -5, revision: 0, light: 0 });
        assert_eq!(s.open((4, 0)), 1.0, "the open step behind");
        assert_eq!(s.open((5, 0)), 0.0, "under the roof");
        assert_eq!(s.open((6, 0)), 0.0);
        assert_eq!(s.open((5, 3)), 0.0, "rock");
    }
}

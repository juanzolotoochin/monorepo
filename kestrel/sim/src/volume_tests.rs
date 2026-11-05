//! The volume (V.1 of the verticality design): spans, floors, the floor
//! under a body's feet, sloped floors, and the volume in the terrain.

use content::Content;
use fx::{Fx, FxVec2};

use crate::hash::hash_of;
use crate::settlement::SceneSetup;
use crate::terrain::{Terrain, LAYER};
use crate::volume::{height, layer_at, Span, Volume};
use crate::World;

fn v(x: f64, y: f64) -> FxVec2 {
    FxVec2::new(Fx::from_bits((x * 4294967296.0) as i64), Fx::from_bits((y * 4294967296.0) as i64))
}

/// A 16 m arena whose ground is `volume`.
fn arena(volume: Volume) -> Terrain {
    let mut terrain = Terrain::flat(v(0.0, 0.0), Fx::from_int(8));
    terrain.volume = Some(volume);
    terrain
}

fn spans(volume: &Volume, tile: (i32, i32)) -> Vec<(i32, i32)> {
    volume.spans(tile).iter().map(|s| (s.bottom, s.top)).collect()
}

/// A column of `spans` (bottom and top layers) on every tile from `min` to
/// `max`.
fn columns(volume: &mut Volume, min: (i32, i32), max: (i32, i32), spans: &[(i32, i32)]) {
    for &(bottom, top) in spans {
        volume.fill_box(min, max, bottom, top);
    }
}

#[test]
fn spans_split_on_clear_and_merge_on_fill() {
    let mut volume = Volume::default();
    let tile = (3, -2);
    for layer in [0, 1, 3, 2] {
        assert!(volume.fill(tile, layer));
    }
    assert_eq!(volume.spans(tile), [Span { bottom: 0, top: 4 }], "touching cells merge");
    assert!(!volume.fill(tile, 2), "already solid");
    assert!(volume.clear(tile, 2));
    assert_eq!(spans(&volume, tile), [(0, 2), (3, 4)], "a tunnel splits the span");
    assert!(!volume.clear(tile, 2), "already empty");
    assert!(volume.clear(tile, 3));
    assert_eq!(spans(&volume, tile), [(0, 2)]);
    assert!(volume.clear(tile, 0));
    assert_eq!(spans(&volume, tile), [(1, 2)], "cleared from the bottom");
    // Below zero too, and a fill between two spans joins them.
    volume.fill(tile, -1);
    volume.fill(tile, 3);
    assert_eq!(spans(&volume, tile), [(-1, 0), (1, 2), (3, 4)]);
    volume.fill(tile, 0);
    volume.fill(tile, 2);
    assert_eq!(spans(&volume, tile), [(-1, 4)]);
    assert!((-1..4).all(|layer| volume.is_solid(tile, layer)));
    assert!(!volume.is_solid(tile, 4) && !volume.is_solid(tile, -2) && !volume.is_solid((3, -1), 0));
    // Ore sits in solid cells and goes with them.
    assert!(!volume.set_ore(tile, 7, Some(1)), "no ore in the air");
    assert!(volume.set_ore(tile, 1, Some(2)));
    assert_eq!(volume.ore(tile, 1), Some(2));
    assert!(volume.clear(tile, 1));
    assert_eq!(volume.ore(tile, 1), None);
    // An emptied column is gone.
    volume.clear_box(tile, tile, -1, 4);
    assert_eq!(volume.columns().count(), 0);
    assert_eq!(volume, Volume::default());
}

#[test]
fn floors_need_headroom_and_have_ceilings() {
    let mut volume = Volume::default();
    // Ground two layers deep, a roof four layers over it.
    columns(&mut volume, (0, 0), (0, 0), &[(0, 2), (6, 8)]);
    assert_eq!(volume.floors((0, 0)).collect::<Vec<_>>(), [2, 8]);
    assert_eq!(volume.ceiling((0, 0), 2), Some(6));
    assert_eq!(volume.headroom((0, 0), 2), Some(4));
    assert_eq!(volume.ceiling((0, 0), 8), None, "open sky");
    assert_eq!(volume.headroom((0, 0), 8), None);
    assert_eq!(volume.top((0, 0)), Some(8));
    // Three layers under the roof is too low to stand in.
    columns(&mut volume, (1, 0), (1, 0), &[(0, 2), (5, 8)]);
    assert_eq!(volume.floors((1, 0)).collect::<Vec<_>>(), [8]);
    assert_eq!(volume.headroom((1, 0), 2), Some(3));
    assert_eq!(volume.floors((2, 0)).count(), 0, "an empty column");
    assert_eq!(volume.top((2, 0)), None);
    // Heights are layers of half a meter.
    assert_eq!(height(8), Fx::from_int(4));
    assert_eq!(layer_at(Fx::from_ratio(9, 10)), 1);
    assert_eq!(layer_at(-Fx::from_ratio(1, 10)), -1);
}

/// A natural bridge: ground at 1 m and a deck at 4 m over the same spot.
/// Feet under the deck stand on the ground, feet on it on the deck.
#[test]
fn a_bridge_has_two_floors_over_one_spot() {
    let mut volume = Volume::default();
    columns(&mut volume, (-2, -2), (2, 2), &[(0, 2), (6, 8)]);
    let p = v(0.2, -0.3);
    assert_eq!(volume.floor(p, Fx::ONE), Some(Fx::ONE), "under the bridge");
    assert_eq!(volume.floor(p, Fx::from_ratio(3, 4)), Some(Fx::ONE), "a step up onto the ground");
    assert_eq!(volume.floor(p, Fx::from_int(3)), Some(Fx::ONE), "jumping under it");
    assert_eq!(volume.floor(p, Fx::from_int(4)), Some(Fx::from_int(4)), "on top");
    assert_eq!(volume.floor(p, Fx::from_ratio(36, 10)), Some(Fx::from_int(4)), "a step up onto the deck");
    assert_eq!(volume.floor(p, Fx::from_int(9)), Some(Fx::from_int(4)), "falling onto it");
    assert_eq!(volume.floor(p, Fx::ZERO), None, "no floor that low");
    // The terrain's ground is the top floor, and its floor the one underfoot.
    let terrain = arena(volume);
    assert_eq!(terrain.ground(p), Some(Fx::from_int(4)));
    assert_eq!(terrain.tile_height((0, 0)), Some(Fx::from_int(4)));
    assert_eq!(terrain.floor(p, Fx::ONE), Some(Fx::ONE));
    assert_eq!(terrain.ground(v(5.0, 0.0)), None, "no cells, no ground");
}

/// A tunnel floor ramps a layer down into the next tile, as a pit's edge
/// does (V4), taking its neighbours from the floor within a layer of it:
/// the roof over it and a floor two layers up don't count.
#[test]
fn tunnel_floors_slope_by_the_corner_rule() {
    let mut volume = Volume::default();
    // Along +x: a floor at 1 m, then one at 0.5 m, each under a roof at
    // 3 m to 4 m; to -x, ground up to 2 m under a roof from 4 m.
    columns(&mut volume, (0, 0), (0, 0), &[(0, 2), (6, 8)]);
    columns(&mut volume, (1, 0), (1, 0), &[(0, 1), (6, 8)]);
    columns(&mut volume, (-1, 0), (-1, 0), &[(0, 4), (8, 10)]);
    let low = Fx::ONE;
    // Tile 0 slopes from its -x edge, level with the wall, down to tile 1.
    assert_eq!(volume.floor(v(-0.5, 0.0), low), Some(Fx::ONE));
    assert_eq!(volume.floor(v(0.0, 0.0), low), Some(Fx::ONE - LAYER / Fx::from_int(2)));
    assert_eq!(volume.floor(v(0.25, 0.3), low), Some(Fx::from_ratio(5, 8)));
    assert_eq!(volume.floor(v(0.5, 0.0), low), Some(LAYER));
    // Tile 1 is the lowest: flat.
    for x in [0.6, 1.0, 1.4] {
        assert_eq!(volume.floor(v(x, 0.1), LAYER), Some(LAYER), "at {x}");
    }
    // The roofs are level over both, and the floor two layers above tile 0
    // is a wall's edge to it, not a slope.
    for x in [-0.4, 0.0, 0.5, 1.2] {
        assert_eq!(volume.floor(v(x, 0.0), Fx::from_int(4)), Some(Fx::from_int(4)), "the roof at {x}");
    }
    assert_eq!(volume.floor(v(-0.9, 0.0), Fx::from_int(2)), Some(Fx::from_int(2)));
    // A sloped terrain's walks see the top floor's slopes.
    let mut terrain = arena(volume);
    terrain.sloped = true;
    assert!(!terrain.steep((0, 0)));
    assert_eq!(terrain.ground(v(0.0, 0.0)), Some(Fx::from_int(4)));
}

#[test]
fn edits_bump_the_revision_and_change_the_hash() {
    let mut volume = Volume::default();
    columns(&mut volume, (0, 0), (3, 3), &[(0, 4)]);
    let before = hash_of(&volume);
    let flat = Terrain::flat(v(0.0, 0.0), Fx::from_int(8));
    let mut terrain = arena(volume.clone());
    assert_ne!(hash_of(&terrain), hash_of(&flat), "the volume is hashed");
    let (hash, revision) = (hash_of(&terrain), terrain.revision);
    assert!(terrain.clear_cell((1, 1), 3));
    assert!(!terrain.clear_cell((1, 1), 3), "already empty");
    assert_eq!(terrain.revision, revision + 1);
    assert_ne!(hash_of(&terrain), hash);
    assert_ne!(hash_of(terrain.volume.as_ref().unwrap()), before);
    assert!(terrain.fill_cell((1, 1), 3));
    assert_eq!(hash_of(terrain.volume.as_ref().unwrap()), before, "the same cells hash the same");
    // Ore counts too.
    let mut ore = volume.clone();
    ore.set_ore((2, 2), 1, Some(0));
    assert_ne!(hash_of(&ore), before);
    // Without a volume, cell edits do nothing.
    let mut flat = flat;
    assert!(!flat.fill_cell((0, 0), 0) && !flat.clear_cell((0, 0), 0));
    assert_eq!(flat.revision, 0);
}

/// A scene's volume is in tiles from the arena's center; a scene without
/// one has none, and keeps its heightfield ground.
#[test]
fn scenes_opt_into_a_volume() {
    let content = Content::builtin();
    let base = SceneSetup { player: "exosuit".into(), extent: Fx::from_int(16), ..SceneSetup::default() };
    let plain = World::new_scene(5, &content, &base).unwrap();
    assert!(plain.terrain().volume.is_none());
    assert_eq!(plain.terrain().ground(plain.terrain().center), Some(Fx::ZERO));
    let mut volume = Volume::default();
    volume.fill_box((-4, -4), (4, 4), 0, 3);
    let world = World::new_scene(5, &content, &SceneSetup { volume: Some(volume.clone()), ..base }).unwrap();
    let terrain = world.terrain();
    let center = Terrain::pit_tile(terrain.center);
    assert_eq!(terrain.volume, Some(volume.shifted(center)));
    assert_eq!(terrain.ground(terrain.center), Some(Fx::from_ratio(3, 2)));
    assert_ne!(hash_of(terrain), hash_of(plain.terrain()));
}

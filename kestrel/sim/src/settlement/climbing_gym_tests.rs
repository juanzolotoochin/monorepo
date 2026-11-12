//! The Climbing Gym (CL.1 of the climbing design, `kestrel/CLIMBING.md`
//! K1, K10): its map is the scene's own, the `volume:` section of
//! `climbing_gym.ron`, built as `lockstep::scene_setup` builds it. These
//! check that it loads, and that its faces and pillars are as tall as its
//! comments say; and the height cap a layout may reach (64 m).

use content::Content;
use fx::Fx;

use super::SceneSetup;
use super::tunnel_lab_tests::{section, tile};
use crate::terrain::Terrain;
use crate::volume::{height, Volume, VolumeBox, VolumeLayout, LAYOUT_LAYERS};
use crate::{Simulation, World};

const SCENE: &str = include_str!("../../../app/assets/levels/climbing_gym.ron");

fn layout() -> VolumeLayout {
    ron::from_str(&section(SCENE, "volume")).unwrap()
}

/// A scene on `layout`'s volume with the elder at `spawn` (scene meters).
fn scene(layout: &VolumeLayout, spawn: [i32; 2]) -> Result<World, String> {
    let setup = SceneSetup {
        player: "elder".into(),
        extent: Fx::from_int(layout.extent),
        diggable: true,
        sloped: true,
        volume: Some(layout.build()),
        spawn: [Fx::from_int(spawn[0]), Fx::ZERO, Fx::from_int(spawn[1])],
        player_health: 6,
        ..SceneSetup::default()
    };
    World::new_scene(1, &Content::builtin(), &setup)
}

/// The gym as the app makes it, the elder at the scene's spawn.
pub(super) fn gym() -> Simulation {
    assert!(SCENE.contains("spawn: (28.0, 0.0, 26.0)"), "the scene's spawn");
    assert!(SCENE.contains("player_health: Some(6)"), "the scene's health");
    Simulation::from_world(scene(&layout(), [28, 26]).unwrap(), 1)
}

/// The top floor over the tile `x`, `z` tiles from the scene's origin, in
/// meters.
fn top(terrain: &Terrain, volume: &Volume, x: i32, z: i32) -> f64 {
    height(volume.top(tile(terrain, x, z)).unwrap_or_else(|| panic!("no ground at ({x}, {z})"))).to_f64()
}

#[test]
fn the_climbing_gym_loads_and_validates() {
    let layout = layout();
    layout.validate().unwrap();
    assert_eq!(layout.extent, 64, "128 m across");
    let sim = gym();
    let world = sim.world();
    let settlement = world.settlement().unwrap();
    let player = settlement.player().expect("the elder");
    let elder = &settlement.agents[player as usize];
    assert_eq!(Some(elder.character), Content::builtin().names.character_index("elder"), "the elder");
    // On the flat, at 0 m.
    assert_eq!(world.terrain().ground(elder.body.position), Some(Fx::ZERO));
    // Only the elder: no ghouls, no sprites.
    assert_eq!(settlement.agents.len(), 1);
}

/// The faces' heights, read off the top floors either side of each.
#[test]
fn the_gyms_faces_and_pillars_are_as_tall_as_authored() {
    let sim = gym();
    let terrain = sim.world().terrain();
    let volume = terrain.volume.as_ref().unwrap();
    let top = |x, z| top(terrain, volume, x, z);
    // The flat, out to the map's corners.
    for (x, z) in [(0, 40), (40, 0), (-60, -60), (64, 64), (-40, 30)] {
        assert_eq!(top(x, z), 0.0, "the flat at ({x}, {z})");
    }
    // The summit, 40 m: its north face from the ground.
    assert_eq!(top(0, -10), 40.0);
    assert_eq!(top(0, -21), 0.0, "north of the summit");
    // The west face, the full 40 m, in pitches of 13.5 m with 2 m ledges.
    assert_eq!(top(-15, -10), 0.0);
    assert_eq!([top(-14, -10), top(-13, -10)], [13.5, 13.5], "the first ledge");
    assert_eq!([top(-12, -10), top(-11, -10)], [27.0, 27.0], "the second ledge");
    assert_eq!(top(-10, -10), 40.0, "the summit");
    // The south shoulder's 20 m face, and the summit's 20 m over it.
    assert_eq!(top(0, 9), 0.0);
    assert_eq!(top(0, 8), 20.0);
    assert_eq!(top(0, 0), 40.0);
    // The inside corner: the shoulder's west face (x -4) meets the summit's
    // south face (z 0), 40 m there from the ground.
    assert_eq!([top(-4, 1), top(-5, 1), top(-5, 0)], [20.0, 0.0, 40.0]);
    // The chimney: 2 m wide, 6 m deep, the shoulder's full 20 m.
    for z in 3..=8 {
        assert_eq!([top(2, z), top(3, z), top(4, z), top(5, z)], [20.0, 0.0, 0.0, 20.0], "the chimney at z {z}");
    }
    assert_eq!(top(3, 2), 20.0, "the chimney's back wall");
    // The east wing's 10 m faces, the low step's 5 m ones.
    assert_eq!([top(23, 0), top(22, 0)], [0.0, 10.0], "the wing's east face");
    assert_eq!([top(16, 6), top(16, 7), top(16, 15)], [10.0, 5.0, 0.0], "the step's faces");
    // The overhang: rock from 7 to 10 m, 4 m out from the wing's east face,
    // open ground under it with a 7 m ceiling.
    let under = tile(terrain, 24, -10);
    assert_eq!(volume.floors(under).collect::<Vec<_>>(), [0, 20]);
    assert_eq!(volume.ceiling(under, 0), Some(14));
    assert_eq!(height(14).to_f64(), 7.0);
    assert_eq!(top(27, -10), 0.0);
    // The pillars: 5, 10 and 15 m, 2 x 2 tiles, the flat round them.
    for (x, metres) in [(24, 5.0), (30, 10.0), (36, 15.0)] {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(top(x + dx, 20 + dz), metres, "the {metres} m pillar");
        }
        assert_eq!([top(x - 1, 20), top(x + 2, 21), top(x, 19), top(x, 22)], [0.0; 4]);
    }
    // Nothing reaches past the summit.
    let highest = volume.columns().filter_map(|(_, spans)| spans.last()).map(|s| s.top).max();
    assert_eq!(highest, Some(80));
}

/// A layout box may reach 64 m over the scene's zero (K10): one to 60 m
/// loads and makes a 60 m floor; one to 70 m is refused, naming the cap.
#[test]
fn a_box_to_60_m_loads_and_one_to_70_m_is_refused() {
    assert_eq!(*LAYOUT_LAYERS.end(), 128, "64 m");
    assert_eq!(*LAYOUT_LAYERS.start(), -32, "16 m below");
    let tower = |top: i32| {
        let mut layout = layout();
        layout.boxes.push(VolumeBox::Solid { min: [-40, -40], max: [-38, -38], bottom: 0, top, material: Default::default() });
        layout
    };
    let tall = tower(120);
    tall.validate().unwrap();
    let world = scene(&tall, [28, 26]).unwrap();
    let terrain = world.terrain();
    assert_eq!(top(terrain, terrain.volume.as_ref().unwrap(), -39, -39), 60.0);
    // The cap itself, 64 m, loads too.
    tower(128).validate().unwrap();
    let error = tower(140).validate().unwrap_err();
    assert!(error.contains("leave layers -32 to 128 (-16 m to 64 m)"), "{error}");
    assert!(error.contains(&format!("volume box {}", layout().boxes.len())), "{error} names the box");
    // Under 16 m below is still refused.
    let mut deep = layout();
    deep.boxes.push(VolumeBox::Empty { min: [0, 40], max: [1, 41], bottom: -34, top: 0 });
    assert!(deep.validate().unwrap_err().contains("leave layers"));
}

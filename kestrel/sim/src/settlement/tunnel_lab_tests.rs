//! The Tunnel Lab (V.2 of the verticality design, `kestrel/VERTICALITY.md`
//! V10): its map is the scene's own, the `volume:` section of
//! `tunnel_lab.ron`, built as `lockstep::scene_setup` builds it.

use content::Content;
use fx::Fx;

use super::{PileSetup, SceneSetup};
use crate::terrain::Terrain;
use crate::volume::{height, VolumeLayout, HEADROOM};
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SCENE: &str = include_str!("../../../app/assets/levels/tunnel_lab.ron");

/// The text of the scene's `key: Some(...)` section, inside the `Some`:
/// its parentheses matched past comments and strings.
fn section(text: &str, key: &str) -> String {
    let open = format!("{key}: Some(");
    let start = text.find(&open).unwrap_or_else(|| panic!("no {open} in the scene")) + open.len();
    let (mut depth, mut chars) = (1, text[start..].char_indices().peekable());
    while let Some((at, c)) = chars.next() {
        match c {
            '/' if chars.peek().is_some_and(|&(_, n)| n == '/') => {
                chars.by_ref().find(|&(_, n)| n == '\n');
            }
            '"' => {
                chars.by_ref().find(|&(_, n)| n == '"');
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return text[start..start + at].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{key}'s section never closes")
}

fn layout() -> VolumeLayout {
    ron::from_str(&section(SCENE, "volume")).unwrap()
}

/// The Tunnel Lab as the app makes it.
fn tunnel_lab() -> Simulation {
    let layout = layout();
    let setup = SceneSetup {
        player: "exosuit".into(),
        extent: Fx::from_int(layout.extent),
        diggable: true,
        sloped: true,
        volume: Some(layout.build()),
        piles: Some(PileSetup { capacity: 12, dispensers: Vec::new(), piles: Vec::new(), collector: Some([Fx::from_int(-3), Fx::from_int(-4)]) }),
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// The tile `x`, `z` tiles from the scene's origin.
fn tile(terrain: &Terrain, x: i32, z: i32) -> (i32, i32) {
    let center = Terrain::pit_tile(terrain.center);
    (center.0 + x, center.1 + z)
}

#[test]
fn the_tunnel_lab_volume_is_valid() {
    layout().validate().unwrap();
}

/// Over the cave's middle the bridge makes two floors: the cave's at 0 m
/// with 3 m to its ceiling, and the bridge's top at 5 m under open sky.
#[test]
fn the_bridge_has_two_floors_over_the_cave() {
    let sim = tunnel_lab();
    let volume = sim.world().terrain().volume.as_ref().expect("the tunnel lab has a volume");
    let spot = tile(sim.world().terrain(), -11, 13);
    assert_eq!(volume.floors(spot).collect::<Vec<_>>(), [0, 10]);
    assert_eq!(volume.headroom(spot, 0), Some(6));
    assert_eq!(volume.headroom(spot, 10), None);
    assert_eq!(height(10), Fx::from_int(5));
    // The stairs up its west side climb a layer a tile, from the ground to
    // the bridge's top.
    let tops: Vec<_> = (-28..=-18).map(|x| volume.top(tile(sim.world().terrain(), x, 13)).unwrap()).collect();
    assert_eq!(tops, (0..=10).collect::<Vec<_>>());
}

/// Through the mountain the tunnel's floor is at 0 m, 3 m wide, with 3 m
/// of headroom under the mountain all the way from mouth to mouth; the
/// mountain's top is 8 m up.
#[test]
fn the_tunnel_runs_through_the_mountain_with_headroom() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let volume = terrain.volume.as_ref().unwrap();
    for x in 3..=25 {
        for z in -1..=1 {
            let t = tile(terrain, x, z);
            assert_eq!(volume.floors(t).next(), Some(0), "the tunnel's floor at ({x}, {z})");
            // Roofed from 6 to 22, open cuts at the mouths.
            let headroom = volume.headroom(t, 0);
            if (6..=22).contains(&x) {
                assert_eq!(headroom, Some(6), "headroom at ({x}, {z})");
                assert!(headroom.unwrap() >= HEADROOM);
            } else {
                assert_eq!(headroom, None, "open sky at ({x}, {z})");
            }
        }
        // The walls beside it are solid at the tunnel's height.
        assert!(volume.is_solid(tile(terrain, x, 2), 0) && volume.is_solid(tile(terrain, x, -2), 0));
    }
    assert_eq!(volume.top(tile(terrain, 14, 4)), Some(16), "8 m up");
    // Ore inside: amber high up, blue in the tunnel's south wall, green deep.
    assert_eq!(volume.ore(tile(terrain, 12, 6), 10), Some(crate::mine::AMBER));
    assert_eq!(volume.ore(tile(terrain, 14, -2), 2), Some(crate::mine::BLUE));
    assert_eq!(volume.ore(tile(terrain, 17, 2), -3), Some(crate::mine::GREEN));
    assert_eq!(volume.ore(tile(terrain, 0, 0), -1), None, "plain stone");
    // The spawn is on the open ground at 0 m.
    let player = sim.world().settlement().unwrap().player().unwrap();
    let body = &sim.world().settlement().unwrap().agents[player as usize].body;
    assert_eq!(terrain.ground(body.position), Some(Fx::ZERO));
}

/// Until V.4 digs the volume, digging here is today's dig and must not
/// break the world.
#[test]
fn digging_in_the_tunnel_lab_does_not_panic() {
    let mut sim = tunnel_lab();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    sim.step(&[Command { player: PlayerId(0), kind: CommandKind::DigDown { agent } }]);
    for _ in 0..content::millis_to_ticks(5000) {
        sim.step(&[]);
    }
    sim.world().settlement().unwrap().validate().unwrap();
}

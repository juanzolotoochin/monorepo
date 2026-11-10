//! The Crawler Lab (CR.1 of the crawler, `kestrel/CRAWLER.md` C11): its map
//! is the scene's own `volume:` section of `crawler_lab.ron` and its rules
//! `crawler_lab_supply.ron`, built as `lockstep::scene_setup` builds them,
//! with the crawler parked beside the spawn.

use content::scenario::Scenario;
use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::tunnel_lab_tests::{command, me, meters, section, tile};
use super::vehicle::{self, Control, VehicleState};
use super::{SceneSetup, VehicleSetup};
use crate::volume::VolumeLayout;
use crate::{CommandKind, Simulation, World};

const SCENE: &str = include_str!("../../../app/assets/levels/crawler_lab.ron");
const SUPPLY: &str = include_str!("../../../app/assets/levels/crawler_lab_supply.ron");

fn layout() -> VolumeLayout {
    ron::from_str(&section(SCENE, "volume")).unwrap()
}

fn seconds(s: f64) -> u64 {
    (s * content::TICKS_PER_SECOND as f64).round() as u64
}

/// The Crawler Lab as the app makes it.
fn crawler_lab() -> Simulation {
    assert!(SCENE.contains(r#"vehicles: [(character: "crawler", position: (2.5, 0.0, 0.0), heading: 0.0)]"#), "the scene's crawler");
    assert!(SCENE.contains(r#"player: "elder""#) && SCENE.contains("spawn: (0.0, 0.0, 0.0)"));
    let layout = layout();
    let setup = SceneSetup {
        scenario: Some(Scenario::parse(SUPPLY).unwrap()),
        player: "elder".into(),
        extent: Fx::from_int(layout.extent),
        sloped: true,
        volume: Some(layout.build()),
        vehicles: vec![VehicleSetup { character: "crawler".into(), position: [Fx::from_ratio(5, 2), Fx::ZERO], heading: Fx::ZERO }],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// Every feature is where the scene's comment says, at its height in
/// layers: the terraces and the 1.5 m step, the cliff, the trench, the pit
/// and the moat round its island.
#[test]
fn the_crawler_lab_has_every_kind_of_ground() {
    layout().validate().unwrap();
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let volume = terrain.volume.as_ref().expect("the crawler lab has a volume");
    let top = |x: i32, z: i32| volume.top(tile(terrain, x, z)).unwrap();
    assert_eq!([top(0, 0), top(9, 0), top(13, 0), top(20, 0)], [0, 1, 2, 3], "terraces of half a meter up to the plateau");
    assert_eq!([top(23, -7), top(23, -6)], [0, 3], "a 1.5 m step onto the plateau");
    assert_eq!([top(0, -17), top(0, -18)], [0, 4], "a 2 m cliff onto the mesa");
    assert_eq!([top(-10, 0), top(-11, 0), top(-12, 0), top(-13, 0)], [0, -4, -4, 0], "a trench 2 m wide and 2 m deep");
    assert_eq!([top(-21, 0), top(-22, 0), top(-24, 0), top(-25, 0)], [0, -6, -6, 0], "a pit 3 m wide and 3 m deep");
    assert_eq!([top(0, 13), top(0, 14), top(0, 15), top(0, 16), top(0, 26), top(0, 27)], [0, -4, -4, 0, 0, -4], "a moat 2 m wide round the island");
    assert_eq!([top(-8, 20), top(-7, 20), top(-6, 20)], [-4, -4, 0]);
    // Ground room for the crawler between the trench and the pit.
    assert!((-21..=-13).all(|x| (-8..=8).all(|z| top(x, z) == 0)));
}

/// The crawler is parked in the vehicles table beside the elder, on the
/// ground, with its definition's legs; the ghouls come.
#[test]
fn the_crawler_is_parked_beside_the_spawn() {
    let mut sim = crawler_lab();
    let center = sim.world().terrain().center;
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.vehicles.len(), 1);
    let crawler = &s.vehicles[0];
    let content = Content::builtin();
    assert_eq!(crawler.character, content.names.character_index("crawler").unwrap());
    let def = &s.characters[crawler.character as usize];
    assert!(def.crawler.is_some(), "a crawler, with legs");
    assert_eq!(crawler.position, center + FxVec2::new(Fx::from_ratio(5, 2), Fx::ZERO));
    assert_eq!((crawler.height, crawler.wheels), (Fx::ZERO, [Fx::ZERO; 4]));
    assert_eq!(me(&sim).body.position, center, "the elder at the spawn");
    assert_eq!(content.names.characters[s.agents[s.player().unwrap() as usize].character as usize], "character/elder");
    for _ in 0..seconds(15.0) {
        sim.step(&[]);
    }
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.threats.len(), 3, "three ghouls out");
    assert!(s.threats.iter().all(|t| s.characters[t.character as usize].chase.is_some()), "chasers");
}

/// G boards the cab: the pilot rides on the deck, 3 m up, and steers it;
/// G again climbs down beside it onto the ground.
#[test]
fn a_pilot_boards_drives_and_climbs_down() {
    let mut sim = crawler_lab();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    command(&mut sim, CommandKind::Board { agent });
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.driving(agent as usize), Some(0), "in reach of the cab");
    let deck = s.characters[s.vehicles[0].character as usize].crawler.unwrap().deck;
    let parked = s.vehicles[0].position;
    command(&mut sim, CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false } });
    for _ in 0..seconds(4.0) {
        sim.step(&[]);
        let s = sim.world().settlement().unwrap();
        let (crawler, pilot) = (&s.vehicles[0], &s.agents[agent as usize]);
        assert_eq!(pilot.body.position, crawler.position, "the pilot rides along");
        assert_eq!(pilot.avatar().unwrap().height, crawler.height + deck, "on the deck");
    }
    command(&mut sim, CommandKind::MoveAvatar { agent, intent: Intent::Stop });
    for _ in 0..seconds(2.0) {
        sim.step(&[]);
    }
    let crawler = sim.world().settlement().unwrap().vehicles[0].clone();
    assert!(crawler.position.y - parked.y > Fx::from_int(3), "it went south: {:?}", crawler.position - parked);
    assert_eq!(crawler.speed, Fx::ZERO);

    command(&mut sim, CommandKind::Board { agent });
    let s = sim.world().settlement().unwrap();
    assert_eq!(s.driving(agent as usize), None, "climbed down");
    let beside = (me(&sim).body.position - crawler.position).length();
    assert!(beside > Fx::from_ratio(3, 2) && beside < Fx::from_ratio(5, 2), "beside it: {} m", beside.to_f64());
    assert_eq!(me(&sim).avatar().unwrap().height, Fx::ZERO, "on the ground");
}

/// Until its feet walk (CR.2) the crawler rolls on the floors under its
/// hips: up the 1.5 m step onto the plateau, but not up the 2 m cliff.
#[test]
fn until_it_walks_the_crawler_rolls_up_the_step_but_not_the_cliff() {
    let sim = crawler_lab();
    let terrain = sim.world().terrain();
    let content = Content::builtin();
    let ix = content.names.character_index("crawler").unwrap();
    let def = &content.rules.characters[ix as usize];
    let drive = |from: (f64, f64), heading: Fx, seconds_held: f64| {
        let mut v = VehicleState::new(ix, def, meters(&sim, from.0, from.1), heading, terrain).expect("parked on the ground");
        v.control = Control { direction: crate::nav::heading_vector(heading), boost: false };
        for _ in 0..seconds(seconds_held) {
            vehicle::self_update(&mut v, terrain, def);
        }
        v
    };
    // South (+z) from north of the plateau, up its 1.5 m north face.
    let up = drive((23.0, -14.0), Fx::ZERO, 10.0);
    assert_eq!(up.height, Fx::from_ratio(3, 2), "on the plateau: {:?}", up.position - terrain.center);
    // North (-z) from the spawn into the mesa's 2 m face: it stops.
    let stopped = drive((0.0, -8.0), Fx::PI, 12.0);
    assert_eq!(stopped.height, Fx::ZERO);
    let front = stopped.position.y - terrain.center.y - def.vehicle.unwrap().wheelbase / Fx::from_int(2);
    assert!(front > Fx::from_int(-18) && front < Fx::from_ratio(-33, 2), "its front hips at the cliff's foot: {}", front.to_f64());
}

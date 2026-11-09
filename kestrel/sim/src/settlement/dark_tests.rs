//! Sprites and ghouls read light (M8.7 of the Sacred Tree depths,
//! `kestrel/SACRED_TREE_DEPTHS.md` L3, L3a, L4): underground a sprite walks
//! and mines only on lit floors (open sky is lit), except on a lantern
//! errand; a stranded one walks back to light; ghouls pay more for lantern
//! light but are never barred from it. Every town test checks every
//! invariant on every tick (`dig_tests::step`).

use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::chase::{self, Goal, Quarry};
use super::dig_tests::{command, step};
use super::light::{Lamp, LightGrid};
use super::light_tests::{light_up, put_elder, stock, until_built};
use super::miner_tests::{first_dig, map, mine, settlement, story, strip_ore, tile, tunnel};
use super::{haulers, miners, AgentIx, BuildingIx, Event, SceneSetup, Threat};
use crate::nav;
use crate::terrain::Terrain;
use crate::volume::{height, Volume, HEADROOM};
use crate::{CommandKind, Simulation, World};

const HZ: u64 = TICKS_PER_SECOND as u64;
const MOSS: AgentIx = 3;
const REED: AgentIx = 4;
const RADIUS: Fx = Fx::from_ratio(11, 50);

/// A 33 m volume scene, its floor at 0 m, split along x by a ridge of rock
/// x -2..2 and 4 m high, which roofed tunnels a tile wide go through at the
/// rows `tunnels`; with `gap`, the ridge is open to the sky at z 10..12, a
/// way round. The player spawns in a far corner.
fn ridge(tunnels: &[i32], gap: bool) -> Simulation {
    let mut volume = Volume::default();
    volume.fill_box((-16, -16), (16, 16), -8, 0);
    volume.fill_box((-2, -16), (2, 16), 0, 8);
    for &z in tunnels {
        volume.clear_box((-2, z), (2, z), 0, HEADROOM);
    }
    if gap {
        volume.clear_box((-2, 10), (2, 12), 0, 8);
    }
    let setup = SceneSetup {
        player: "exosuit".into(),
        extent: Fx::from_int(16),
        diggable: true,
        sloped: true,
        volume: Some(volume),
        spawn: [Fx::from_int(-13), Fx::ZERO, Fx::from_int(-13)],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// A tile `x`, `z` tiles from the scene's middle (where its volume's
/// tile (0, 0) lies).
fn at(sim: &Simulation, x: i32, z: i32) -> (i32, i32) {
    let c = Terrain::pit_tile(sim.world().terrain().center);
    (c.0 + x, c.1 + z)
}

fn lamp(tile: (i32, i32)) -> Lamp {
    Lamp { tile, floor: 0, range: Fx::from_int(6) }
}

/// The tiles, from the scene's middle, a route from `start` passes,
/// sampled every quarter meter.
fn tiles(sim: &Simulation, start: FxVec2, route: &[FxVec2]) -> Vec<(i32, i32)> {
    let c = at(sim, 0, 0);
    let mut out = Vec::new();
    let mut from = start;
    for &to in route {
        let n = (nav::distance(from, to) * Fx::from_int(4)).ceil().floor_to_int().max(1);
        for i in 0..=n {
            let t = Terrain::pit_tile(from + (to - from) * Fx::from_ratio(i, n));
            out.push((t.0 - c.0, t.1 - c.1));
        }
        from = to;
    }
    out
}

/// The rows of the ridge (x -2..2) a route crosses it at.
fn crossings(sim: &Simulation, start: FxVec2, route: &[FxVec2]) -> std::collections::BTreeSet<i32> {
    tiles(sim, start, route).into_iter().filter(|t| (-2..=2).contains(&t.0)).map(|t| t.1).collect()
}

/// The two ends of the ridge's crossing, 6 m either side of it.
fn ends(sim: &Simulation) -> (FxVec2, FxVec2) {
    (Terrain::pit_center(at(sim, -6, 0)), Terrain::pit_center(at(sim, 6, 0)))
}

/// A sprite's route keeps to lit floors: through a dark tunnel never; round
/// by the open-sky gap when there is one, nowhere when there isn't; through
/// the tunnel once a lantern lights it. Without the rule (`lit` none, a
/// lantern errand's) it takes the tunnel, the short way.
#[test]
fn a_sprite_wont_route_through_a_dark_tunnel() {
    let sim = ridge(&[0], true);
    let (start, goal) = ends(&sim);
    let terrain = sim.world().terrain();
    let dark = LightGrid::of_lamps(terrain, &[]);
    let route = |terrain: &Terrain, light: Option<&LightGrid>| haulers::route_at(terrain, start, Fx::ZERO, goal, Some(Fx::ZERO), RADIUS, &[], light);
    let free = route(terrain, None);
    assert_eq!(crossings(&sim, start, &free), [0].into(), "free, the tunnel: {free:?}");
    let lit = route(terrain, Some(&dark));
    assert!(!lit.is_empty(), "there is a way round under the sky");
    let rows = crossings(&sim, start, &lit);
    assert!(!rows.is_empty() && rows.iter().all(|z| (10..=12).contains(z)), "round by the gap, not the tunnel: {rows:?}");
    assert_eq!(lit, route(terrain, Some(&dark)), "deterministic");

    let shut = ridge(&[0], false);
    let terrain = shut.world().terrain();
    let dark = LightGrid::of_lamps(terrain, &[]);
    assert!(route(terrain, Some(&dark)).is_empty(), "no way but the dark: it doesn't go");
    let lantern = LightGrid::of_lamps(terrain, &[lamp(at(&shut, 0, 0))]);
    assert!(lantern.lit(terrain, Terrain::pit_center(at(&shut, 2, 0)), Fx::ZERO), "the lantern lights the tunnel");
    let through = route(terrain, Some(&lantern));
    assert_eq!(crossings(&shut, start, &through), [0].into(), "lit, the tunnel: {through:?}");
}

/// A ghoul's 3D plan (`chase::shun`, `LIT_STEP`) between two tunnels of the
/// same length takes the dark one, whichever is lit; with a target standing
/// in the lit one, or no other way, it still goes in and reaches it.
#[test]
fn a_ghoul_takes_the_dark_way_and_still_reaches_a_lit_target() {
    let sim = ridge(&[-2, 2], false);
    let (start, goal) = ends(&sim);
    let terrain = sim.world().terrain();
    let plan = |terrain: &Terrain, light: &LightGrid, to: FxVec2| {
        let shun = chase::shun(terrain, light);
        nav::path_toward_with(terrain, start, Fx::ZERO, to, Some(Fx::ZERO), chase::WALK_CLEARANCE, &[], shun.as_deref())
    };
    for (lit, dark) in [(-2, 2), (2, -2)] {
        let light = LightGrid::of_lamps(terrain, &[lamp(at(&sim, 0, lit))]);
        assert_eq!(light.lantern(terrain, start, Fx::ZERO), 0, "the start is dark");
        let p = plan(terrain, &light, goal);
        assert!(p.reached, "{p:?}");
        let route: Vec<FxVec2> = p.route.iter().map(|&(q, _)| q).collect();
        assert_eq!(crossings(&sim, start, &route), [dark].into(), "the dark tunnel, not the lit one at {lit}: {route:?}");
        assert_eq!(p, plan(terrain, &light, goal), "deterministic");
        // A target standing in the lit tunnel: it goes in.
        let inside = Terrain::pit_center(at(&sim, 0, lit));
        let p = plan(terrain, &light, inside);
        assert!(p.reached, "it reaches a target in light");
        assert_eq!(p.route.last().map(|&(q, _)| q), Some(inside));
    }
    // One tunnel only, lit: never barred.
    let one = ridge(&[2], false);
    let terrain = one.world().terrain();
    let light = LightGrid::of_lamps(terrain, &[lamp(at(&one, 0, 2))]);
    assert!(plan(terrain, &light, goal).reached, "through the lit tunnel, the only way");
}

/// In the town's tunnel (`miner_tests::tunnel`), dark, with ore only in its
/// end wall: the wall is no target (its stand is dark), and a miner never
/// stands on a dark floor nor digs there; it waits by the stone pile. A
/// lantern in the tunnel lights the stand, and it walks in and digs it.
#[test]
fn a_miner_wont_mine_a_dark_face_and_will_once_a_lantern_lights_it() {
    let mut sim = map();
    let (wall, stand, floor) = tunnel(&mut sim);
    let from_tree = (wall.0 - tile(&sim, 0, 0).0, wall.1 - tile(&sim, 0, 0).1);
    strip_ore(&mut sim, |t| t != from_tree);
    // A tick, so the grid is up to date with the scene's kinds.
    step(&mut sim, &[]);
    let dock = {
        let s = settlement(&sim);
        Terrain::pit_tile(s.depots[s.stone_depot().unwrap().1 as usize].position)
    };
    let targets = miners::search(sim.world().terrain(), &settlement(&sim).light, dock, RADIUS);
    assert!(targets.iter().all(|t| t.tile != wall), "the dark wall is no target: {targets:?}");
    mine(&mut sim, REED);
    for _ in 0..40 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        let a = &s.agents[REED as usize];
        assert!(s.lit(sim.world().terrain(), a.body.position, a.height()), "Reed keeps to light: {}", story(&sim, REED));
        assert!(a.sprite().unwrap().dig.is_none(), "nothing to dig in light");
    }
    let lantern_at = tile(&sim, 16, -26);
    light_up(&mut sim, lantern_at, floor);
    let dig = first_dig(&mut sim, REED, 240);
    assert_eq!((dig.tile, dig.layer), (wall, Some(floor)), "lit, the wall is dug");
    assert_eq!(Terrain::pit_tile(settlement(&sim).agents[REED as usize].body.position), stand);
}

/// The dark tunnel's first lantern (L3a): its blueprint on the tunnel's
/// floor, the miners walk the dark tunnel on their errand, bring its stone
/// and amber and build it. Off an errand, no miner stands on a dark floor
/// except stranded on its way back to light.
#[test]
fn a_miner_on_a_lantern_errand_walks_into_the_dark_and_builds_it() {
    let mut sim = map();
    let (_, stand, floor) = tunnel(&mut sim);
    strip_ore(&mut sim, |t| (t.0 - 16).abs() <= 10 && (t.1 + 26).abs() <= 10);
    let at = tile(&sim, 16, -26);
    let kind = super::light_tests::lantern(&sim);
    put_elder(&mut sim, stand, height(floor));
    stock(&mut sim, 1);
    step(&mut sim, &[command(CommandKind::PlaceBlueprint { kind, position: Terrain::pit_center(at) })]);
    let b = (settlement(&sim).buildings.len() - 1) as BuildingIx;
    assert!(settlement(&sim).buildings[b as usize].site().is_some());
    mine(&mut sim, MOSS);
    mine(&mut sim, REED);
    let mut in_dark = 0;
    for _ in 0..240 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        if s.buildings[b as usize].built() {
            break;
        }
        for who in [MOSS, REED] {
            let a = &s.agents[who as usize];
            let sprite = a.sprite().unwrap();
            if !s.lit(sim.world().terrain(), a.body.position, a.height()) {
                assert!(sprite.on_errand() || sprite.light.stranded, "{who} in the dark off an errand: {}", story(&sim, who));
                assert!(sprite.dig.is_none(), "no digging in the dark");
                in_dark += 1;
            }
        }
    }
    until_built(&mut sim, b, 1);
    assert!(in_dark > 0, "they walked the dark tunnel to the site");
    assert!(settlement(&sim).light.level(at, floor) > 0, "it lights the tunnel");
}

/// A miner put down in the dark tunnel, not on an errand, is stranded: it
/// walks back to light (the trench's open sky) and keeps to it after.
#[test]
fn a_miner_stranded_in_the_dark_walks_back_to_light() {
    let mut sim = map();
    let (_, stand, floor) = tunnel(&mut sim);
    mine(&mut sim, REED);
    {
        let s = sim.world_mut().settlement_mut();
        let a = &mut s.agents[REED as usize];
        a.body.position = Terrain::pit_center(stand);
        a.body.route.clear();
        let sprite = a.sprite_mut().unwrap();
        sprite.height = height(floor);
        sprite.plan.clear();
        sprite.step = 0;
    }
    let lit = |sim: &Simulation| {
        let s = settlement(sim);
        let a = &s.agents[REED as usize];
        s.lit(sim.world().terrain(), a.body.position, a.height())
    };
    assert!(!lit(&sim), "in the dark");
    let mut out = None;
    for tick in 0..60 * HZ {
        step(&mut sim, &[]);
        assert!(settlement(&sim).agents[REED as usize].sprite().unwrap().dig.is_none(), "no dig in the dark");
        if lit(&sim) {
            out = Some(tick);
            break;
        }
    }
    let out = out.unwrap_or_else(|| panic!("back to light: {}", story(&sim, REED)));
    eprintln!("back in light after {:.1} s", out as f64 / HZ as f64);
    for _ in 0..30 * HZ {
        step(&mut sim, &[]);
        assert!(lit(&sim), "it keeps to light: {}", story(&sim, REED));
    }
}

/// A ghoul still comes into the lit tunnel after a sprite standing at its
/// end (L4: light costs, never bars), and hits it there.
#[test]
fn a_ghoul_comes_into_a_lit_tunnel_after_a_sprite() {
    let mut sim = map();
    let (_, stand, floor) = tunnel(&mut sim);
    let lantern_at = tile(&sim, 16, -26);
    light_up(&mut sim, lantern_at, floor);
    {
        let s = sim.world_mut().settlement_mut();
        for c in &mut s.characters {
            if let Some(chase) = &mut c.chase {
                chase.leash = Fx::from_int(40);
            }
        }
        let a = &mut s.agents[REED as usize];
        a.body.position = Terrain::pit_center(stand);
        a.body.home = a.body.position;
        a.body.route.clear();
        a.sprite_mut().unwrap().height = height(floor);
    }
    let terrain = sim.world().terrain();
    let position = Terrain::pit_center(tile(&sim, 4, -26));
    let feet = chase::surface_feet(terrain, position);
    let character = Content::builtin().names.character_index("ghoul_sacred_tree").unwrap();
    let s = sim.world_mut().settlement_mut();
    let id = s.next_id;
    s.next_id += 1;
    s.threats.push(Threat {
        id,
        wave: 0,
        character,
        position,
        heading: Fx::ZERO,
        travelled: Fx::ZERO,
        hp: 2,
        armor: 0,
        pierced: false,
        shocked_until: 0,
        route: vec![position],
        stagger_until: 0,
        knockback: FxVec2::ZERO,
        windup: None,
        ready_at: 0,
        burn: None,
        goal: Goal::Chase(Quarry::Agent(REED)),
        stranded: false,
        height: feet,
        chase: Vec::new(),
        backoff: Default::default(),
    });
    let (mut lit, mut hit) = (false, false);
    for _ in 0..40 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        let Some(t) = s.threats.first() else { break };
        lit |= s.light.lantern(sim.world().terrain(), t.position, t.height) > 0;
        hit |= s.events.iter().any(|e| matches!(e, Event::ThreatSwing { agent, hit: true, .. } if *agent == REED));
        if hit {
            break;
        }
    }
    assert!(lit, "the ghoul walked into the lantern's light");
    assert!(hit, "and hit the sprite there");
}

/// Light-read miners play the same on 1 and 4 threads and on from a save:
/// the light grid and the miners' reach are derived, not saved.
#[test]
fn light_read_miners_are_deterministic_across_threads_and_saves() {
    let run = |threads: usize, save_at: Option<u64>| {
        let mut sim = map();
        let (_, _, floor) = tunnel(&mut sim);
        let lantern_at = tile(&sim, 16, -26);
        light_up(&mut sim, lantern_at, floor);
        let mut sim = Simulation::from_world(sim.into_world(), threads);
        let jobs = [command(CommandKind::SetSpriteJob { agent: MOSS, job: super::SpriteJob::Mine }), command(CommandKind::SetSpriteJob { agent: REED, job: super::SpriteJob::Mine })];
        let mut hashes = Vec::new();
        for tick in 0..60 * HZ {
            if save_at == Some(tick) {
                let world = sim.into_world();
                let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
                let rules = world.rules().clone();
                sim = Simulation::from_world(World::from_state(postcard::from_bytes(&bytes).unwrap(), rules).unwrap(), threads);
            }
            let turn = if tick == 0 { &jobs[..] } else { &[] };
            hashes.push(sim.step(turn).total);
        }
        hashes
    };
    let one = run(1, None);
    assert_eq!(one, run(4, None), "the thread count changed them");
    assert_eq!(one, run(2, Some(25 * HZ)), "a save changed them");
}

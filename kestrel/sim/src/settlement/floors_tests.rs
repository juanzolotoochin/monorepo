//! Every body on a floor (M8.2 of the Sacred Tree depths,
//! `kestrel/SACRED_TREE_DEPTHS.md` T5): in a volume scene haulers and
//! threats have a feet height, walk on foot and plan in 3D; a ghoul chases
//! a sprite into the Tunnel Lab's tunnel and under its bridge, can't hit one
//! on another floor, and waits as close as it gets to one it can't reach,
//! backing its plans off; a hauler carries down a dug stair. Every test
//! checks every invariant on every tick.

use content::scenario::Scenario;
use content::Content;
use fx::{Fx, FxVec2};

use super::tunnel_lab_tests::{layout, meters, Cells};
use super::{AgentIx, Event, Goal, Quarry, SceneSetup, Settlement};
use crate::nav;
use crate::volume::Volume;
use crate::{Simulation, World};

const HZ: u64 = content::TICKS_PER_SECOND as u64;

/// A scenario for a volume scene of `extent`: the player far off in a
/// corner, `sprites` (id and scene meters) and haulers, depots and links
/// as RON lists, and a Sacred Tree ghoul coming from `from` to `to` after
/// `first` seconds.
fn scenario(extent: i32, sprites: &[(&str, f64, f64)], depots: &str, links: &str, haulers: &str, wave: Option<((f64, f64), (f64, f64), f64)>) -> Scenario {
    let sprites: Vec<String> = sprites.iter().map(|(id, x, z)| format!(r#"(id: "{id}", label: "{id}", character: "sprite", position: ({x:.1}, {z:.1}))"#)).collect();
    let waves = wave.map_or(String::new(), |((fx, fz), (tx, tz), first)| {
        format!(r#"(id: "test", position: ({fx:.1}, {fz:.1}), target: ({tx:.1}, {tz:.1}), character: "ghoul_sacred_tree", first: {first:.1}, every: 1.0, count: 1)"#)
    });
    Scenario::parse(&format!(
        r#"(id: "floors", extent: {extent}.0, items: [(id: "wood", label: "WOOD", skin: "item_lumber", scale: 0.45)],
         depots: [{depots}], sources: [], links: [{links}],
         agents: [(id: "player", label: "YOU", character: "exosuit", position: (0.0, 0.0), capacity: 8, links: [],
                   cargo_anchor: (0.0, 0.65, -0.19), equipment: None, controlled: true){haulers}],
         sprites: [{}], producers: [], defenders: [], waves: [{waves}], base_health: 5, player_health: 0)"#,
        sprites.join(", ")
    ))
    .unwrap()
}

/// A scene over `volume` (`extent` tiles each way) with `scenario`, its
/// player spawning in the far corner, out of every ghoul's way.
fn scene(volume: Volume, extent: i32, scenario: Scenario) -> Simulation {
    let corner = Fx::from_int(-(extent - 3));
    let setup = SceneSetup {
        scenario: Some(scenario),
        player: "exosuit".into(),
        extent: Fx::from_int(extent),
        diggable: true,
        sloped: true,
        volume: Some(volume),
        spawn: [corner, Fx::ZERO, corner],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

/// The Tunnel Lab's volume with `sprites` and a ghoul.
fn lab(sprites: &[(&str, f64, f64)], wave: ((f64, f64), (f64, f64), f64)) -> Simulation {
    let layout = layout();
    scene(layout.build(), layout.extent, scenario(layout.extent, sprites, "", "", "", Some(wave)))
}

/// A small volume (33 m of ground, its floor at 0 m, with `boxes` over
/// it), as `tunnel_lab_tests::small` makes one.
fn small_volume(boxes: &[Cells]) -> Volume {
    let mut volume = Volume::default();
    volume.fill_box((-16, -16), (16, 16), -8, 0);
    for &(solid, min, max, bottom, top) in boxes {
        if solid {
            volume.fill_box(min, max, bottom, top);
        } else {
            volume.clear_box(min, max, bottom, top);
        }
    }
    volume
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn agent(sim: &Simulation, id: &str) -> AgentIx {
    settlement(sim).agents.iter().position(|a| a.id == id).unwrap() as AgentIx
}

/// Steps once, checking every invariant, and returns the tick's events.
fn step(sim: &mut Simulation) -> Vec<Event> {
    sim.step(&[]);
    settlement(sim).validate().unwrap();
    settlement(sim).events.clone()
}

/// Steps until a threat is out.
fn spawn(sim: &mut Simulation) {
    for _ in 0..5 * HZ {
        if !settlement(sim).threats.is_empty() {
            return;
        }
        step(sim);
    }
    panic!("no ghoul came");
}

/// Sets the ghouls' leash (and detect radius) so a chase lasts: a sprite
/// walks faster than a ghoul and would get past the 5 m leash.
fn long_leash(sim: &mut Simulation) {
    for c in &mut sim.world_mut().settlement_mut().characters {
        if let Some(chase) = &mut c.chase {
            chase.leash = Fx::from_int(40);
        }
    }
}

/// The swings that hit `quarry` among `events`.
fn hits(events: &[Event], quarry: AgentIx) -> usize {
    events.iter().filter(|e| matches!(e, Event::ThreatSwing { agent, hit: true, .. } if *agent == quarry)).count()
}

/// Sends sprite `id` home to scene meters `x`, `z`: an idle sprite walks
/// home (on a 3D path in a volume).
fn send_home(sim: &mut Simulation, id: &str, x: f64, z: f64) {
    let home = meters(sim, x, z);
    let i = agent(sim, id) as usize;
    sim.world_mut().settlement_mut().agents[i].body.home = home;
}

/// A ghoul north-west of the tunnel's west mouth spots a sprite beside it
/// and chases it into the tunnel, round the mountain's corner (no straight
/// walk), on the tunnel's floor at 0 m under the mountain, and hits it
/// there. The sprite walks there on a 3D path too.
#[test]
fn a_ghoul_chases_a_sprite_into_the_tunnel() {
    let mut sim = lab(&[("ash", 1.0, 2.6)], ((1.0, 4.2), (-28.0, 4.0), 0.2));
    let ash = agent(&sim, "ash");
    long_leash(&mut sim);
    send_home(&mut sim, "ash", 12.0, 0.0);
    spawn(&mut sim);
    let center = sim.world().terrain().center;
    let (mut chased, mut inside, mut hit) = (false, false, 0);
    for _ in 0..40 * HZ {
        let events = step(&mut sim);
        let s = settlement(&sim);
        let Some(t) = s.threats.first() else { break };
        chased |= t.goal == Goal::Chase(Quarry::Agent(ash));
        let at = t.position - center;
        // Under the mountain's roof (x 6 to 22) it walks on the tunnel's
        // floor, never on the mountain over it.
        if at.x > Fx::from_int(6) && at.x < Fx::from_int(22) && at.y.abs() < Fx::from_int(2) {
            assert_eq!(t.height, Fx::ZERO, "on the tunnel's floor at {at:?}");
            inside = true;
        }
        let sprite = &s.agents[ash as usize];
        if sprite.body.alive() {
            assert!(sprite.height() <= Fx::ONE, "the sprite walks on the ground and the tunnel's floor: {:?}", sprite.height());
        }
        hit += hits(&events, ash);
        if inside && hit > 0 {
            break;
        }
    }
    assert!(chased, "the ghoul spotted the sprite");
    assert!(inside, "the ghoul followed it into the tunnel");
    assert!(hit > 0, "and hit it there");
    let t = &settlement(&sim).threats[0];
    assert!(t.position.x - center.x > Fx::from_int(8), "deep in the tunnel: {:?}", t.position - center);
}

/// A ghoul south-west of the bridge chases a sprite into the cave under it:
/// round the bridge's corner on the cave's floor at 0 m, not on the
/// bridge's top 5 m over it, and hits it there.
#[test]
fn a_ghoul_chases_a_sprite_under_the_bridge() {
    let mut sim = lab(&[("ash", -14.5, 7.6)], ((-16.0, 6.6), (-28.0, 0.0), 0.2));
    let ash = agent(&sim, "ash");
    long_leash(&mut sim);
    send_home(&mut sim, "ash", -11.0, 13.0);
    spawn(&mut sim);
    let center = sim.world().terrain().center;
    let (mut under, mut hit) = (false, 0);
    for _ in 0..40 * HZ {
        let events = step(&mut sim);
        let s = settlement(&sim);
        let Some(t) = s.threats.first() else { break };
        let at = t.position - center;
        if at.x > Fx::from_int(-13) && at.x < Fx::from_int(-9) && at.y > Fx::from_int(11) {
            assert_eq!(t.height, Fx::ZERO, "on the cave's floor at {at:?}");
            under = true;
        }
        hit += hits(&events, ash);
        if under && hit > 0 {
            break;
        }
    }
    assert!(under, "the ghoul followed the sprite under the bridge");
    assert!(hit > 0, "and hit it there");
}

/// Puts the first threat at scene meters `x`, `z` with its feet at
/// `height`, chasing `quarry`.
fn chase_from(sim: &mut Simulation, x: f64, z: f64, height: Fx, quarry: AgentIx) {
    let p = meters(sim, x, z);
    let t = &mut sim.world_mut().settlement_mut().threats[0];
    t.position = p;
    t.height = height;
    t.goal = Goal::Chase(Quarry::Agent(quarry));
}

/// Puts agent `id` at scene meters `x`, `z` on the floor at `height`,
/// staying there (its home).
fn put(sim: &mut Simulation, id: &str, x: f64, z: f64, height: Fx) {
    let p = meters(sim, x, z);
    let i = agent(sim, id) as usize;
    let a = &mut sim.world_mut().settlement_mut().agents[i];
    a.body.position = p;
    a.body.home = p;
    a.body.route.clear();
    a.sprite_mut().unwrap().height = height;
}

/// A ghoul on the bridge right over a sprite in the cave can't swing at
/// it, nor one in the cave at a sprite on the bridge: they are 5 m apart,
/// on two floors over one spot. Until it comes round onto the sprite's
/// floor, nothing it does reaches.
#[test]
fn a_ghoul_cant_hit_a_sprite_on_another_floor() {
    for (ghoul, sprite) in [(Fx::from_int(5), Fx::ZERO), (Fx::ZERO, Fx::from_int(5))] {
        let mut sim = lab(&[("ash", -14.5, 7.6)], ((-16.0, 6.6), (-28.0, 0.0), 0.2));
        let ash = agent(&sim, "ash");
        spawn(&mut sim);
        put(&mut sim, "ash", -11.0, 13.0, sprite);
        chase_from(&mut sim, -11.0, 13.2, ghoul, ash);
        for _ in 0..4 * HZ {
            let events = step(&mut sim);
            let s = settlement(&sim);
            let t = &s.threats[0];
            assert!(t.windup.is_none(), "no swing from {:?} at a sprite at {sprite:?}", t.height);
            assert!(!events.iter().any(|e| matches!(e, Event::ThreatWindup { .. } | Event::ThreatSwing { .. })), "{events:?}");
            assert_eq!(s.agents[ash as usize].body.health, s.agents[ash as usize].body.max_health);
            assert!((t.height - ghoul).abs() <= Fx::ONE, "it stays on its own floor this long: {:?}", t.height);
        }
    }
}

/// A sprite on a sheer pillar 3 m high, which nothing walks up: the ghoul
/// chasing it walks to the foot of the pillar, the reachable point nearest
/// it, and waits there, planning again only after its backoff, which
/// doubles while it can't reach (2, 4, then 8 s), never swinging.
#[test]
fn a_ghoul_that_cant_reach_waits_at_the_nearest_point_and_backs_off() {
    let pillar: Cells = (true, (3, -1), (5, 1), 0, 6);
    let volume = small_volume(&[pillar]);
    let mut sim = scene(volume, 16, scenario(16, &[("ash", -4.0, 6.0)], "", "", "", Some(((-6.0, 6.0), (-14.0, 6.0), 0.2))));
    let ash = agent(&sim, "ash");
    long_leash(&mut sim);
    spawn(&mut sim);
    put(&mut sim, "ash", 4.0, 0.0, Fx::from_int(3));
    chase_from(&mut sim, -2.0, 0.0, Fx::ZERO, ash);
    let mut plans = Vec::new();
    let mut last = settlement(&sim).threats[0].backoff;
    for _ in 0..30 * HZ {
        let events = step(&mut sim);
        let t = &settlement(&sim).threats[0];
        assert_eq!(t.goal, Goal::Chase(Quarry::Agent(ash)), "it keeps after the sprite, within its leash");
        assert_eq!(t.height, Fx::ZERO, "on the ground");
        assert_eq!(hits(&events, ash), 0);
        if t.backoff != last {
            plans.push((sim.world().tick(), t.backoff.failures));
            last = t.backoff;
        }
    }
    let t = &settlement(&sim).threats[0];
    // The pillar's middle: its sides are 1.5 m out.
    let foot = meters(&sim, 4.0, 0.0);
    assert!(nav::distance(t.position, foot) <= Fx::from_ratio(21, 10), "it waits at the pillar's foot, by its side: {:?}", t.position - foot);
    let gaps: Vec<u64> = plans.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let failed = super::follow::FAILED_REPLAN_TICKS;
    assert!(plans.len() >= 4, "it planned again now and then: {plans:?}");
    assert_eq!(gaps[..3], [failed, failed * 2, failed * 4], "the waits double: {plans:?}");
    assert!(gaps[3..].iter().all(|&g| g == failed * 4), "then stay at 8 s: {plans:?}");
}

/// The forest's worker hauls wood from a depot on the ground to one at the
/// bottom of a pit 2 m deep, walking down the stair dug into its side and
/// back up, its feet on each floor.
#[test]
fn a_hauler_carries_wood_down_a_dug_stair() {
    let pit: Cells = (false, (5, -3), (8, 3), -4, 0);
    let stair: [Cells; 3] = [(false, (2, -1), (2, 1), -1, 0), (false, (3, -1), (3, 1), -2, 0), (false, (4, -1), (4, 1), -3, 0)];
    let mut boxes = vec![pit];
    boxes.extend(stair);
    let depots = r#"(id: "top", label: "TOP", position: (-3.0, 0.0), capacity: 20, stock: [(item: "wood", units: 8)]),
                    (id: "bottom", label: "BOTTOM", position: (7.0, 1.0), capacity: 20, stock: [])"#;
    let links = r#"(id: "down", from: "top", to: "bottom", item: "wood", target: 8, priority: 1)"#;
    let haulers = r#", (id: "worker", label: "WORKER", character: "worker", position: (-3.0, -2.0), capacity: 4, links: ["down"],
                     cargo_anchor: (0.0, 0.65, -0.19), equipment: None)"#;
    let mut sim = scene(small_volume(&boxes), 16, scenario(16, &[], depots, links, haulers, None));
    let worker = agent(&sim, "worker") as usize;
    let bottom = settlement(&sim).depot_index("bottom").unwrap() as usize;
    let wood = settlement(&sim).item_index("wood").unwrap();
    let mut lowest = Fx::ZERO;
    for _ in 0..90 * HZ {
        step(&mut sim);
        let s = settlement(&sim);
        let hauler = s.agents[worker].hauler().unwrap();
        lowest = lowest.min(hauler.height);
        assert!(hauler.height >= Fx::from_int(-2) && hauler.height <= Fx::ZERO, "on the ground, the stair or the pit's floor: {:?}", hauler.height);
        if s.depots[bottom].inventory.count(wood) == 8 {
            break;
        }
    }
    let s = settlement(&sim);
    assert_eq!(s.depots[bottom].inventory.count(wood), 8, "all the wood went down");
    assert_eq!(lowest, Fx::from_int(-2), "it stood on the pit's floor");
}

/// A sprite killed in a tunnel comes back at home on home's floor, not
/// where it fell (M8.2): its home is in the tunnel, under the mountain.
#[test]
fn a_sprite_comes_back_on_homes_floor() {
    let mut sim = lab(&[("ash", 12.0, 0.0)], ((1.0, 4.2), (-28.0, 4.0), 0.2));
    let ash = agent(&sim, "ash") as usize;
    assert_eq!(settlement(&sim).agents[ash].height(), Fx::ZERO, "it starts on the tunnel's floor");
    // Dead on the bridge's top, it comes back in the tunnel.
    let p = meters(&sim, -11.0, 13.0);
    let tick = sim.world().tick();
    let s = sim.world_mut().settlement_mut();
    s.agents[ash].body.position = p;
    s.agents[ash].sprite_mut().unwrap().height = Fx::from_int(5);
    assert!(s.hurt_sprite(ash, 99, tick, 3));
    for _ in 0..10 {
        step(&mut sim);
    }
    let a = &settlement(&sim).agents[ash];
    assert!(a.body.alive());
    assert_eq!(a.body.position, meters(&sim, 12.0, 0.0));
    assert_eq!(a.height(), Fx::ZERO);
}

/// A chase in 3D runs the same every time.
#[test]
fn chases_in_3d_are_deterministic() {
    let run = || {
        let mut sim = lab(&[("ash", 1.0, 2.6)], ((1.0, 4.2), (-28.0, 4.0), 0.2));
        long_leash(&mut sim);
        send_home(&mut sim, "ash", 12.0, 0.0);
        let hashes: Vec<u64> = (0..20 * HZ).map(|_| sim.step(&[]).total).collect();
        (hashes, settlement(&sim).behaviour_digest(), settlement(&sim).threats.clone())
    };
    let (a, b) = (run(), run());
    assert_eq!(a.0, b.0);
    assert_eq!(a.1, b.1);
    assert!(a.2.iter().any(|t| t.goal != Goal::Tree || t.height != Fx::ZERO || !t.chase.is_empty()), "it chased: {:?}", a.2);
}

/// The forest outpost (no volume) keeps its haulers' and threats' feet at
/// zero and never backs a plan off: other scenes run as they did.
#[test]
fn other_scenes_keep_bodies_at_zero() {
    let text = include_str!("../../../app/assets/levels/combat_arena_supply.ron");
    let scenario = Scenario::parse(text).unwrap();
    let mut sim = Simulation::from_world(World::new_settlement(11, &Content::builtin(), &scenario, &[]).unwrap(), 1);
    for _ in 0..30 * HZ {
        sim.step(&[]);
        let s = settlement(&sim);
        assert!(s.threats.iter().all(|t| t.height == Fx::ZERO && t.chase.is_empty() && t.backoff == super::Backoff::default()));
        assert!(s.agents.iter().filter_map(|a| a.hauler()).all(|h| h.height == Fx::ZERO && h.backoff == super::Backoff::default()));
        assert!(s.projectiles.iter().all(|p| p.floor == Fx::ZERO));
    }
    let _ = FxVec2::ZERO;
}

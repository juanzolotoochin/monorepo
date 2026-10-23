//! Bevy projection of the shared simulation. All stock and decisions belong to gameplay_core.
use crate::{assets::SceneLibrary, rig::Rig};
use bevy::prelude::*;
use gameplay_core::{Endpoint, Inventory, Simulation};
use std::collections::BTreeMap;
#[derive(Resource, Default)]
pub struct Settlement {
    current: Option<usize>,
    pub simulation: Option<Simulation>,
    views: BTreeMap<String, (String, Rig)>,
    labels: BTreeMap<String, Entity>,
    hud: Option<Entity>,
}
#[derive(Component)]
pub struct Caption(Vec3);
struct View {
    skin: String,
    position: Vec3,
    heading: f32,
    scale: f32,
    travel: f32,
    activity: f32,
    aim: Option<f32>,
}
impl View {
    fn new(skin: &str, p: [f32; 2]) -> Self {
        Self {
            skin: skin.into(),
            position: Vec3::new(p[0], 0., p[1]),
            heading: 0.,
            scale: 1.,
            travel: 0.,
            activity: 0.,
            aim: None,
        }
    }
}
pub fn setup(world: &mut World) {
    let index = world.resource::<SceneLibrary>().current;
    if world.resource::<Settlement>().current == Some(index) {
        return;
    }
    let mut state = world.remove_resource::<Settlement>().unwrap();
    for (_, (_, rig)) in std::mem::take(&mut state.views) {
        world.despawn(rig.root);
    }
    for (_, e) in std::mem::take(&mut state.labels) {
        world.despawn(e);
    }
    if let Some(e) = state.hud.take() {
        world.despawn(e);
    }
    state.current = Some(index);
    let scenario = world.resource::<SceneLibrary>().scenes[index]
        .scene
        .scenario
        .clone();
    let obstacles = world
        .query::<&crate::gameplay::Obstacle>()
        .iter(world)
        .map(|o| gameplay_core::navigation::Disc {
            center: [o.0.center.x, o.0.center.z],
            radius: o.0.radius,
        })
        .collect();
    state.simulation = scenario.map(|s| Simulation::new(s, obstacles).expect("validated scenario"));
    if let Some(sim) = &state.simulation {
        for d in &sim.config().depots {
            let e = world
                .spawn((
                    Caption(Vec3::new(d.position[0], 1.05, d.position[1])),
                    Text::new(""),
                    TextFont {
                        font_size: 12.,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.87, 0.6)),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                ))
                .id();
            state.labels.insert(d.id.clone(), e);
        }
        state.hud = Some(
            world
                .spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 14.,
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.83, 0.63)),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(36),
                        bottom: px(50),
                        ..default()
                    },
                ))
                .id(),
        );
    }
    world.insert_resource(state);
}
fn slots(n: u32, spacing: f32) -> Vec3 {
    Vec3::new(
        (n % 4) as f32 * spacing - 1.5 * spacing,
        (n / 12) as f32 * spacing + 0.18,
        ((n / 4) % 3) as f32 * spacing - spacing,
    )
}
fn cargo_position(sim: &Simulation, id: &str) -> Vec3 {
    let a = &sim.agents[id];
    let config = sim.config().agents.iter().find(|c| c.id == id).unwrap();
    let (p, h, anchor) = if let (Some(t), Some(e)) = (&a.trailer, &config.equipment) {
        (t.position, t.heading, e.cargo_anchor)
    } else {
        (a.position, a.heading, config.cargo_anchor)
    };
    Vec3::new(p[0], 0., p[1]) + Quat::from_rotation_y(h) * Vec3::from_array(anchor)
}
fn endpoint(sim: &Simulation, e: &Endpoint) -> Vec3 {
    match e {
        Endpoint::Depot(id) => {
            let d = sim.config().depots.iter().find(|d| d.id == *id).unwrap();
            Vec3::new(d.position[0], 0.25, d.position[1])
        }
        Endpoint::Cargo(id) => cargo_position(sim, id),
    }
}
fn inventory(
    views: &mut BTreeMap<String, View>,
    sim: &Simulation,
    key: &str,
    stock: &Inventory,
    p: Vec3,
    heading: f32,
    spacing: f32,
) {
    let mut slot = 0;
    for (id, units) in &stock.items {
        let item = sim.config().items.iter().find(|i| i.id == *id).unwrap();
        for i in 0..*units {
            let mut view = View::new(&item.skin, [0., 0.]);
            view.position = p + Quat::from_rotation_y(heading) * slots(slot, spacing);
            view.scale = item.scale;
            view.heading = heading;
            views.insert(format!("{key}/{id}/{i}"), view);
            slot += 1;
        }
    }
}
pub fn tick(world: &mut World) {
    let dt = world.resource::<Time>().delta_secs_f64();
    let mut state = world.remove_resource::<Settlement>().unwrap();
    let Some(sim) = &mut state.simulation else {
        world.insert_resource(state);
        return;
    };
    // The live protocol, interactive UI, and scripted demos change this same model.
    if world
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::KeyP)
    {
        sim.paused = !sim.paused;
    }
    sim.advance(dt).expect("bounded frame duration");
    for event in &sim.events {
        use gameplay_core::Event;
        let cue = match event {
            Event::TransferStarted(_) => Some(crate::sound::Cue::Toss),
            Event::TransferArrived(_) => Some(crate::sound::Cue::Land),
            Event::Shot { .. } => Some(crate::sound::Cue::Swing),
            Event::Hit { .. } => Some(crate::sound::Cue::Stone),
            Event::Produced { .. } | Event::Equipped { .. } => Some(crate::sound::Cue::Clang),
            _ => None,
        };
        if let Some(cue) = cue {
            world.resource_mut::<crate::sound::Sound>().play(cue);
        }
    }
    let mut desired = BTreeMap::new();
    for config in &sim.config().agents {
        let a = &sim.agents[&config.id];
        let mut view = View::new(&config.skin, a.position);
        view.heading = a.heading;
        view.travel = a.travelled;
        desired.insert(format!("agent/{}", a.id), view);
        inventory(
            &mut desired,
            sim,
            &format!("cargo/{}", a.id),
            &a.cargo,
            cargo_position(sim, &a.id),
            a.trailer.as_ref().map_or(a.heading, |t| t.heading),
            0.18,
        );
        if let (Some(t), Some(e)) = (&a.trailer, &config.equipment) {
            let mut v = View::new(&e.skin, t.position);
            v.heading = t.heading;
            v.travel = t.travelled;
            desired.insert(format!("equipment/{}", a.id), v);
        }
    }
    for config in &sim.config().producers {
        let machine = &sim.producers[&config.id];
        let mut v = View::new(&config.skin, config.position);
        v.activity = machine.cycles as f32
            + (if machine.remaining > 0 {
                1. - machine.remaining as f32 / (config.seconds * 60.)
            } else {
                0.
            });
        desired.insert(format!("machine/{}", config.id), v);
    }
    for config in &sim.config().defenders {
        let d = &sim.defenders[&config.id];
        let mut v = View::new(&config.skin, config.position);
        v.aim = d
            .aim
            .map(|p| (p[0] - config.position[0]).atan2(p[1] - config.position[1]));
        v.activity = (1. - (sim.tick.saturating_sub(d.fired_at)) as f32 / 15.).max(0.);
        desired.insert(format!("defender/{}", config.id), v);
    }
    for t in sim.threats.values() {
        let mut v = View::new(&sim.config().waves[t.wave].skin, t.position);
        v.heading = t.heading;
        v.travel = t.travelled;
        desired.insert(format!("threat/{}", t.id), v);
    }
    for d in &sim.config().depots {
        inventory(
            &mut desired,
            sim,
            &format!("depot/{}", d.id),
            &sim.depots[&d.id],
            Vec3::new(d.position[0], 0., d.position[1]),
            0.,
            0.23,
        );
    }
    for t in sim.transfers.values() {
        let item = sim.config().items.iter().find(|i| i.id == t.item).unwrap();
        let f = (sim.tick - t.born) as f32 / (t.arrival - t.born) as f32;
        let mut v = View::new(&item.skin, [0., 0.]);
        v.position = endpoint(sim, &t.from).lerp(endpoint(sim, &t.to), f)
            + Vec3::Y * (std::f32::consts::PI * f).sin() * 0.9;
        v.heading = f * 4.;
        v.scale = item.scale;
        desired.insert(format!("transfer/{}", t.id), v);
    }
    for p in sim.projectiles.values() {
        let item = sim.config().items.iter().find(|i| i.id == p.item).unwrap();
        let f = (sim.tick - p.born) as f32 / (p.arrival - p.born) as f32;
        let mut v = View::new(&item.skin, [0., 0.]);
        v.position = Vec3::new(p.from[0], 1.6, p.from[1]).lerp(Vec3::new(p.to[0], 0.5, p.to[1]), f);
        v.heading = -(p.to[1] - p.from[1]).atan2(p.to[0] - p.from[0]);
        v.scale = 0.8;
        desired.insert(format!("projectile/{}", p.id), v);
    }
    for d in &sim.config().depots {
        let stock = &sim.depots[&d.id];
        let counts = stock
            .items
            .iter()
            .map(|(id, n)| format!("{n} {id}"))
            .collect::<Vec<_>>()
            .join(" · ");
        world.get_mut::<Text>(state.labels[&d.id]).unwrap().0 = format!(
            "{}\n{}",
            d.label,
            if counts.is_empty() {
                "empty".into()
            } else {
                counts
            }
        );
    }
    if let Some(e) = state.hud {
        world.get_mut::<Text>(e).unwrap().0=format!("LUMBER + IRON → ARROWS → DEFENSE    |    LUMBER + IRON → CART → OX\n{} delivered   ·   {} crafted   ·   {} shots / {} ghouls stopped   ·   base {}    [P] pause",sim.metrics.delivered,sim.metrics.cycles,sim.metrics.shots,sim.metrics.kills,sim.health);
    }
    let pack = &world.resource::<SceneLibrary>().scenes[world.resource::<SceneLibrary>().current];
    let profile = pack.scene.style.clone().unwrap_or_default();
    let skins = pack.skins.clone();
    let stale: Vec<_> = state
        .views
        .keys()
        .filter(|k| !desired.contains_key(*k))
        .cloned()
        .collect();
    for key in stale {
        let (_, rig) = state.views.remove(&key).unwrap();
        world.despawn(rig.root);
    }
    for (key, view) in desired {
        let skin = skins
            .iter()
            .find(|s| s.id == view.skin)
            .expect("validated skin");
        let (_, rig) = state
            .views
            .entry(key)
            .or_insert_with(|| (view.skin.clone(), Rig::spawn(world, skin, &profile)));
        rig.pose(
            world,
            view.position,
            view.heading,
            skin.scale * view.scale,
            view.travel,
            view.activity,
            view.aim,
        );
    }
    world.insert_resource(state);
}
pub fn snapshot(world: &World) -> serde_json::Value {
    world.resource::<Settlement>().simulation.as_ref().map_or(serde_json::Value::Null,|s|serde_json::json!({"simulation":s,"balances":s.balances(),"scenario":s.config(),"seconds":s.seconds()}))
}
pub fn captions(
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(&Caption, &mut Node, &mut Visibility)>,
) {
    for (p, mut node, mut visible) in &mut labels {
        if let Ok(v) = camera.0.world_to_viewport(camera.1, p.0) {
            *visible = Visibility::Inherited;
            node.left = px(v.x - 45.);
            node.top = px(v.y);
        } else {
            *visible = Visibility::Hidden;
        }
    }
}

pub fn model(world: &mut World) -> Result<Mut<'_, Simulation>, String> {
    let state = world.resource_mut::<Settlement>();
    if state.simulation.is_none() {
        return Err("No settlement loaded".into());
    }
    Ok(state.map_unchanged(|s| s.simulation.as_mut().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_supply_chain_builds_transport_and_defends_with_conserved_materials() {
        let library = SceneLibrary::new();
        let pack = &library.scenes[6];
        let obstacles = pack
            .environment
            .as_ref()
            .unwrap()
            .obstacles
            .iter()
            .map(|(p, r)| gameplay_core::navigation::Disc {
                center: [p[0], p[2]],
                radius: *r,
            })
            .collect();
        let mut sim = Simulation::new(pack.scene.scenario.clone().unwrap(), obstacles).unwrap();
        for _ in 0..1800 {
            sim.advance(1. / 30.).unwrap();
            sim.validate_state().unwrap();
        }
        assert!(sim.metrics.delivered > 50);
        assert!(sim.metrics.cycles > 10);
        assert_eq!(sim.metrics.equipped, 1);
        assert!(sim.metrics.kills >= 7);
        assert_eq!(sim.health, 20);
        assert_eq!(sim.agents["bramble"].cargo.capacity, 9);
        assert!(sim.balances().values().all(|b| b.conserved));
    }
}

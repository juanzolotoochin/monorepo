//! Bevy projection of the shared simulation. All stock and decisions belong to gameplay_core.
use crate::{
    assets::SceneLibrary,
    rig::{Fade, Rig, RigTemplate},
};
use bevy::prelude::*;
use gameplay_core::{Endpoint, Inventory, Simulation};
use std::collections::BTreeMap;
#[derive(Resource, Default)]
pub struct Settlement {
    current: Option<usize>,
    pub simulation: Option<Simulation>,
    views: BTreeMap<String, (String, Rig)>,
    templates: BTreeMap<String, RigTemplate>,
    pool: BTreeMap<String, Vec<Rig>>,
    labels: BTreeMap<String, Entity>,
    hud: Option<Entity>,
    last_sound: u64,
    dying: Vec<Corpse>,
    attached: BTreeMap<u64, Vec<Rig>>,
    hit_until: BTreeMap<u64, u64>,
    blockers: Vec<Entity>,
    pub notice: String,
    pub inspection: Option<String>,
}
struct Corpse {
    skin: String,
    rig: Rig,
    born: u64,
    rest: Transform,
    fade: Fade,
    arrows: Vec<Rig>,
}
#[derive(Component)]
pub struct Caption {
    building: bool,
    position: Vec3,
    detail: String,
    units: u32,
}
struct View {
    skin: String,
    position: Vec3,
    heading: f32,
    roll: f32,
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
            roll: 0.,
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
    for (_, rigs) in std::mem::take(&mut state.pool) {
        for rig in rigs {
            world.despawn(rig.root);
        }
    }
    for corpse in state.dying.drain(..) {
        corpse.fade.restore(world);
        world.despawn(corpse.rig.root);
    }
    state.attached.clear();
    state.hit_until.clear();
    state.inspection = None;
    state.last_sound = 0;
    state.templates.clear();
    for (_, e) in std::mem::take(&mut state.labels) {
        world.despawn(e);
    }
    if let Some(e) = state.hud.take() {
        world.despawn(e);
    }
    for e in state.blockers.drain(..) {
        world.despawn(e);
    }
    state.notice = if index == 7 {
        "RIGHT CLICK TO MOVE  /  SHIFT + LEFT CLICK TO STRIKE".into()
    } else {
        "LMB stock: take 4 / Ctrl+LMB: deposit cargo / E: toggle nearby machine".into()
    };
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
    if scenario.is_some() {
        let pack = &world.resource::<SceneLibrary>().scenes[index];
        let skins = pack.skins.clone();
        let profile = pack.scene.style.clone().unwrap_or_default();
        for skin in &skins {
            state
                .templates
                .insert(skin.id.clone(), RigTemplate::upload(world, skin, &profile));
        }
    }
    state.simulation = scenario.map(|s| Simulation::new(s, obstacles).expect("validated scenario"));
    if let Some(sim) = &state.simulation {
        for (p, r) in sim
            .config()
            .producers
            .iter()
            .map(|p| (p.position, p.radius))
            .chain(
                sim.config()
                    .defenders
                    .iter()
                    .map(|p| (p.position, p.radius)),
            )
        {
            if r > 0. {
                state.blockers.push(
                    world
                        .spawn(crate::gameplay::Obstacle(crate::navigation::Disc {
                            center: Vec3::new(p[0], 0., p[1]),
                            radius: r,
                        }))
                        .id(),
                );
            }
        }
        for d in &sim.config().depots {
            let e = world
                .spawn((
                    Caption {
                        building: false,
                        position: Vec3::new(d.position[0], 0.7, d.position[1]),
                        detail: String::new(),
                        units: 0,
                    },
                    BackgroundColor(Color::NONE),
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
        for (key, position) in sim
            .config()
            .producers
            .iter()
            .map(|p| (format!("producer/{}", p.id), p.position))
            .chain(
                sim.config()
                    .defenders
                    .iter()
                    .map(|d| (format!("defender/{}", d.id), d.position)),
            )
        {
            let e = world
                .spawn((
                    Caption {
                        building: true,
                        position: Vec3::new(position[0], 1.4, position[1]),
                        detail: String::new(),
                        units: 0,
                    },
                    BackgroundColor(Color::NONE),
                    Text::new(""),
                    TextFont {
                        font_size: 14.,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.90, 0.76)),
                    Node {
                        position_type: PositionType::Absolute,
                        max_width: px(310.),
                        ..default()
                    },
                ))
                .id();
            state.labels.insert(key, e);
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
                        top: px(146),
                        ..default()
                    },
                ))
                .id(),
        );
    }
    world.insert_resource(state);
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
            Vec3::new(d.position[0], 0., d.position[1]) + Vec3::from_array(d.stock_anchor)
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
    layout: &gameplay_core::CargoLayout,
) {
    let mut slot = 0;
    for (id, units) in &stock.items {
        let item = sim.config().items.iter().find(|i| i.id == *id).unwrap();
        for i in 0..*units {
            let mut view = View::new(&item.skin, [0., 0.]);
            let offset = Vec3::from_array(layout.offset(slot));
            view.position = p + Quat::from_rotation_y(heading) * offset;
            view.scale = item.scale * layout.scale;
            view.roll = layout.roll;
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
    let player = world
        .query_filtered::<&Transform, With<crate::gameplay::Player>>()
        .single(world)
        .unwrap();
    let pos = player.translation;
    let heading = player.rotation.to_euler(EulerRot::YXZ).0;
    let controlled: Vec<_> = sim
        .config()
        .agents
        .iter()
        .filter(|a| a.controlled)
        .map(|a| a.id.clone())
        .collect();
    for id in controlled {
        sim.set_controlled_pose(&id, [pos.x, pos.z], heading)
            .unwrap();
    }
    let simulation_start = std::time::Instant::now();
    let tick_before = sim.tick;
    sim.advance(dt.min(0.25)).expect("bounded frame duration");
    crate::telemetry::elapsed(world, "settlement_simulation", simulation_start);
    world
        .resource_mut::<crate::telemetry::Telemetry>()
        .count("simulation_steps", sim.tick - tick_before);
    world.resource_mut::<crate::telemetry::Telemetry>().gauge(
        "simulation_steps_this_frame",
        (sim.tick - tick_before) as f64,
    );
    if dt > 0.25 {
        world
            .resource_mut::<crate::telemetry::Telemetry>()
            .count("simulation_clamped_frames", 1);
        world
            .resource_mut::<crate::telemetry::Telemetry>()
            .gauge("last_clamped_seconds", dt - 0.25);
    }
    let projection_start = std::time::Instant::now();
    if world
        .resource::<crate::gameplay::Controls>()
        .attack
        .is_some_and(|a| {
            a.age >= crate::gameplay::WINDUP + 0.055 && a.age < crate::gameplay::ACTIVE_END
        })
    {
        if let Some(id) = sim
            .config()
            .agents
            .iter()
            .find(|a| a.controlled)
            .map(|a| a.id.clone())
        {
            let hits = sim.strike(&id).unwrap();
            if hits > 0 {
                let mut controls = world.resource_mut::<crate::gameplay::Controls>();
                if let Some(attack) = &mut controls.attack {
                    attack.impact_angle = Some(attack.angle());
                    attack.age = 0.0;
                }
                controls.velocity = Vec3::ZERO;
                controls.impacts += hits;
                let mut feedback = world.resource_mut::<crate::effects::Feedback>();
                feedback.hit_stop = 0.075;
                feedback.trauma = 0.24;
            }
        }
    }
    for event in &sim.events {
        use gameplay_core::Event;
        if let Event::Hit {
            target,
            projectile,
            killed,
            position,
        } = event
        {
            let direction =
                Vec3::new(position[0] - pos.x, 0.0, position[1] - pos.z).normalize_or(Vec3::Z);
            crate::effects::blood(
                world,
                Vec3::new(position[0], 0.65, position[1]),
                direction,
                (*target as u32).wrapping_add(sim.tick as u32),
            );
            let key = format!("threat/{target}");
            if let (Some(projectile), Some((_, body))) = (projectile, state.views.get(&key)) {
                let parent = body.root;
                if let Some((_, arrow)) = state.views.remove(&format!("projectile/{projectile}")) {
                    let parent_transform = *world.get::<Transform>(parent).unwrap();
                    let mut arrow_transform = *world.get::<Transform>(arrow.root).unwrap();
                    arrow_transform.translation = Vec3::new(position[0], 0.6, position[1]);
                    *world.get_mut::<Transform>(arrow.root).unwrap() = Transform::from_matrix(
                        parent_transform.to_matrix().inverse() * arrow_transform.to_matrix(),
                    );
                    world.entity_mut(parent).add_child(arrow.root);
                    state.attached.entry(*target).or_default().push(arrow);
                }
            }
            if *killed {
                if let Some((skin, rig)) = state.views.remove(&key) {
                    let rest = *world.get::<Transform>(rig.root).unwrap();
                    let fade = Fade::new(world, rig.root);
                    state.dying.push(Corpse {
                        skin,
                        rig,
                        born: sim.tick,
                        rest,
                        fade,
                        arrows: state.attached.remove(target).unwrap_or_default(),
                    });
                }
                state.hit_until.remove(target);
            } else {
                state.hit_until.insert(*target, sim.tick + 14);
            }
        }
        let cue = match event {
            Event::TransferStarted(_) => None,
            Event::TransferArrived(_) => None,
            Event::Shot { .. } => Some(crate::sound::Cue::Swing),
            Event::Hit {
                projectile: None, ..
            } => Some(crate::sound::Cue::GhoulHit),
            Event::Hit { .. } => Some(crate::sound::Cue::Stone),
            Event::Produced { .. } | Event::Equipped { .. } => Some(crate::sound::Cue::Clang),
            _ => None,
        };
        if let Some(cue) = cue {
            if sim.tick >= state.last_sound + 18 {
                world.resource_mut::<crate::sound::Sound>().play(cue);
                state.last_sound = sim.tick;
            } else {
                world
                    .resource_mut::<crate::telemetry::Telemetry>()
                    .count("activity_sounds_coalesced", 1);
            }
        }
    }
    let mut desired = BTreeMap::new();
    for config in &sim.config().agents {
        let a = &sim.agents[&config.id];
        let mut view = View::new(&config.skin, a.position);
        view.heading = a.heading;
        view.travel = a.travelled;
        if !config.controlled {
            desired.insert(format!("agent/{}", a.id), view);
        }
        inventory(
            &mut desired,
            sim,
            &format!("cargo/{}", a.id),
            &a.cargo,
            cargo_position(sim, &a.id),
            a.trailer.as_ref().map_or(a.heading, |t| t.heading),
            config
                .equipment
                .as_ref()
                .filter(|_| a.trailer.is_some())
                .map_or(&config.cargo_layout, |e| &e.cargo_layout),
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
        let wounded = t.hp < sim.config().waves[t.wave].hp;
        v.activity = state
            .hit_until
            .get(&t.id)
            .map_or(if wounded { 0.58 } else { 0.0 }, |until| {
                (until.saturating_sub(sim.tick) as f32 / 14.).max(if wounded { 0.58 } else { 0.0 })
            });
        if wounded {
            v.roll = 0.12 + (t.travelled * 13.0).sin() * 0.05;
        }
        desired.insert(format!("threat/{}", t.id), v);
    }
    for d in &sim.config().depots {
        inventory(
            &mut desired,
            sim,
            &format!("depot/{}", d.id),
            &sim.depots[&d.id],
            Vec3::new(d.position[0], 0., d.position[1]) + Vec3::from_array(d.stock_anchor),
            0.,
            &d.stock_layout,
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
        let label = format!(
            "{}\n{}",
            d.label,
            if counts.is_empty() {
                "empty".into()
            } else {
                counts
            }
        );
        let mut caption = world.get_mut::<Caption>(state.labels[&d.id]).unwrap();
        if caption.detail != label {
            caption.detail = label;
            caption.units = stock.total();
            world
                .resource_mut::<crate::telemetry::Telemetry>()
                .count("label_updates", 1);
        }
    }
    for p in &sim.config().producers {
        let detail = producer_description(sim, p);
        let mut caption = world
            .get_mut::<Caption>(state.labels[&format!("producer/{}", p.id)])
            .unwrap();
        if caption.detail != detail {
            caption.detail = detail;
        }
    }
    for d in &sim.config().defenders {
        let stock = sim.depots[&d.depot].units(&d.ammo);
        let label = if d.label.is_empty() {
            d.id.to_uppercase()
        } else {
            d.label.clone()
        };
        let detail = format!("{}\n{}\n\nConsumes: 1 {} per shot\nProduces: {} damage per hit\nFire interval: {:.1}s · Range: {:.1}m\nAmmunition: {}\nStatus: {}", label, d.description, d.ammo, d.damage, d.period, d.range, stock,
            if !sim.defenders[&d.id].enabled { "Disabled" } else if stock == 0 { "Waiting for arrows" } else { "Guarding the approach" });
        let mut caption = world
            .get_mut::<Caption>(state.labels[&format!("defender/{}", d.id)])
            .unwrap();
        if caption.detail != detail {
            caption.detail = detail;
        }
    }
    if let Some(e) = state.hud {
        let cargo = sim
            .agents
            .get("player")
            .map(|a| {
                a.cargo
                    .items
                    .iter()
                    .map(|(i, n)| format!("{n} {i}"))
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let line = if state.current == Some(7) {
            format!(
                "{} GHOULS SLAIN  /  {} STRIKES LANDED  /  {} APPROACHING",
                sim.metrics.melee_kills,
                sim.metrics.melee_hits,
                sim.threats.len()
            )
        } else {
            format!(
                "BASE {}  /  {} GHOULS STOPPED  /  PACK {} /8 {}   [P] pause  [F3] metrics",
                sim.health,
                sim.metrics.kills,
                sim.agents.get("player").map_or(0, |a| a.cargo.total()),
                cargo
            )
        };
        if world.get::<Text>(e).unwrap().0 != line {
            world.get_mut::<Text>(e).unwrap().0 = line;
        }
    }
    crate::telemetry::elapsed(world, "settlement_projection", projection_start);
    let sync_start = std::time::Instant::now();
    let mut spawned = 0;
    let mut recycled = 0;
    let mut writes = 0;
    let mut skipped = 0;
    let mut pooled = 0;
    let mut destroyed = 0;
    let mut surviving = vec![];
    for corpse in state.dying.drain(..) {
        let age = (sim.tick - corpse.born) as f32 / 60.;
        if age >= 3.0 {
            corpse.fade.restore(world);
            for arrow in corpse.arrows {
                world.despawn(arrow.root);
            }
            world.entity_mut(corpse.rig.root).insert(Visibility::Hidden);
            state.pool.entry(corpse.skin).or_default().push(corpse.rig);
            pooled += 1;
        } else {
            let mut t = corpse.rest;
            t.rotation *= Quat::from_rotation_x(-1.5 * (age / 0.6).min(1.));
            *world.get_mut::<Transform>(corpse.rig.root).unwrap() = t;
            corpse.fade.opacity(world, 1. - ((age - 1.) / 2.).max(0.));
            surviving.push(corpse);
        }
    }
    state.dying = surviving;
    let stale: Vec<_> = state
        .views
        .keys()
        .filter(|k| !desired.contains_key(*k))
        .cloned()
        .collect();
    for key in stale {
        let (skin, rig) = state.views.remove(&key).unwrap();
        let pool = state.pool.entry(skin).or_default();
        if pool.len() < 64 {
            world.entity_mut(rig.root).insert(Visibility::Hidden);
            pool.push(rig);
            pooled += 1;
        } else {
            world.despawn(rig.root);
            destroyed += 1;
        }
    }
    for (key, mut view) in desired {
        view.position.y +=
            crate::jetpack::height(world.resource::<SceneLibrary>(), view.position).unwrap_or(0.);
        let template = &state.templates[&view.skin];
        if !state.views.contains_key(&key) {
            let rig = if let Some(rig) = state.pool.get_mut(&view.skin).and_then(|p| p.pop()) {
                world.entity_mut(rig.root).insert(Visibility::Inherited);
                recycled += 1;
                rig
            } else {
                spawned += 1;
                template.spawn(world)
            };
            state.views.insert(key.clone(), (view.skin.clone(), rig));
        }
        let (_, rig) = &state.views[&key];
        let (changed, unchanged) = rig.pose(
            world,
            view.position,
            view.heading,
            view.roll,
            template.scale * view.scale,
            view.travel,
            view.activity,
            view.aim,
        );
        writes += changed;
        skipped += unchanged;
    }
    crate::telemetry::elapsed(world, "settlement_sync", sync_start);
    let mut telemetry = world.resource_mut::<crate::telemetry::Telemetry>();
    for (name, value) in [
        ("rigs_spawned", spawned),
        ("rigs_reused", recycled),
        ("rigs_pooled", pooled),
        ("rigs_destroyed", destroyed),
        ("transform_writes", writes),
        ("transform_writes_avoided", skipped),
    ] {
        telemetry.count(name, value);
    }
    telemetry.gauge("active_rigs", state.views.len() as f64);
    telemetry.gauge("dying_rigs", state.dying.len() as f64);
    telemetry.gauge(
        "attached_projectiles",
        state.attached.values().map(|v| v.len()).sum::<usize>() as f64,
    );
    telemetry.gauge(
        "pooled_rigs",
        state.pool.values().map(|p| p.len()).sum::<usize>() as f64,
    );
    world.insert_resource(state);
}
pub fn snapshot(world: &World) -> serde_json::Value {
    world.resource::<Settlement>().simulation.as_ref().map_or(serde_json::Value::Null,|s|serde_json::json!({"simulation":s,"balances":s.balances(),"scenario":s.config(),"seconds":s.seconds()}))
}
/// Pin the same hover card for deterministic capture or remote inspection.
/// Passing None restores mouse-driven hover.
pub fn inspect_building(world: &mut World, id: Option<&str>) -> Result<(), String> {
    let mut state = world.resource_mut::<Settlement>();
    let key = if let Some(id) = id {
        [format!("producer/{id}"), format!("defender/{id}")]
            .into_iter()
            .find(|key| state.labels.contains_key(key))
            .ok_or("Unknown building")
            .map(Some)?
    } else {
        None
    };
    state.inspection = key;
    Ok(())
}
fn producer_description(sim: &Simulation, p: &gameplay_core::Producer) -> String {
    let recipe = |stacks: &[gameplay_core::Stack]| {
        stacks
            .iter()
            .map(|s| format!("{} {}", s.units, s.item))
            .collect::<Vec<_>>()
            .join(" + ")
    };
    let state = &sim.producers[&p.id];
    let status = match sim.production_status(&p.id).unwrap() {
        "processing" => format!("Working · {:.1}s remaining", state.remaining as f32 / 60.),
        "disabled" => "Disabled; current job will finish".into(),
        "commission_complete" => "Commission complete".into(),
        "output_blocked" => "Waiting for output space".into(),
        "input_starved" => "Waiting for ingredients".into(),
        _ => "Ready".into(),
    };
    format!("{}\n{}\n\nConsumes: {}\nProduces: {}\nCycle: {:.1}s · Completed: {}\nStatus: {}\nE nearby: enable / disable", p.label, p.description,
        recipe(&p.ingredients), recipe(&p.products), p.seconds, state.cycles, status)
}
pub fn captions(
    settlement: Res<Settlement>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    windows: Query<&Window>,
    mut labels: Query<(
        Entity,
        &Caption,
        &mut Node,
        &mut Visibility,
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut BackgroundColor,
    )>,
) {
    let cursor = windows.iter().next().and_then(|w| w.cursor_position());
    let hovered = settlement
        .inspection
        .as_ref()
        .and_then(|key| settlement.labels.get(key).copied())
        .or_else(|| {
            cursor.and_then(|cursor| {
                labels
                    .iter()
                    .filter_map(|(e, p, _, _, _, _, _, _)| {
                        camera
                            .0
                            .world_to_viewport(camera.1, p.position)
                            .ok()
                            .map(|v| {
                                (
                                    e,
                                    v.distance_squared(cursor),
                                    if p.building { 65.0_f32 } else { 40.0_f32 },
                                )
                            })
                    })
                    .filter(|(_, d, radius)| *d < radius * radius)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(e, _, _)| e)
            })
        });
    for (e, p, mut node, mut visibility, mut text, mut font, mut color, mut background) in
        &mut labels
    {
        let Ok(v) = camera.0.world_to_viewport(camera.1, p.position) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let expanded = hovered == Some(e);
        *visibility = if (hovered.is_some() || p.building) && !expanded {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        let content = if expanded {
            if p.building {
                p.detail.clone()
            } else {
                format!("{}\nLMB take / Ctrl+LMB deposit", p.detail)
            }
        } else {
            p.units.to_string()
        };
        if text.0 != content {
            text.0 = content;
        }
        font.font_size = if expanded { 14. } else { 10. };
        color.0 = if expanded {
            Color::srgb(0.95, 0.89, 0.72)
        } else {
            Color::srgba(0.85, 0.85, 0.65, 0.65)
        };
        background.0 = if expanded {
            Color::srgba(0.025, 0.045, 0.03, 0.94)
        } else {
            Color::NONE
        };
        node.padding = UiRect::all(px(if expanded { 8. } else { 0. }));
        let size = windows
            .iter()
            .next()
            .map_or(Vec2::new(1440., 960.), |w| Vec2::new(w.width(), w.height()));
        node.left = px((v.x - if expanded { 45. } else { 3. })
            .clamp(8., (size.x - if expanded { 330. } else { 20. }).max(8.)));
        node.top =
            px(v.y.clamp(8., (size.y - if expanded { 300. } else { 20. }).max(8.)));
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
        for frame in 0..3000 {
            sim.advance(1. / 30.).unwrap();
            sim.validate_state().unwrap();
            if frame == 1799 {
                assert_eq!(sim.health, 20, "first minute defense failed");
            }
            let a = &sim.agents["bramble"];
            if let Some(t) = &a.trailer {
                assert!(
                    gameplay_core::navigation::distance(a.position, t.position) >= 1.45,
                    "ox overlapped the cart at tick {}",
                    sim.tick
                );
            }
        }
        let p = &sim.config().producers[0];
        let card = producer_description(&sim, p);
        assert!(card.contains(&p.description));
        assert!(card.contains("Consumes: 1 lumber + 1 iron"));
        assert!(card.contains("Produces: 4 arrow"));
        assert!(card.contains("Cycle: 4.0s"));
        assert!(
            sim.metrics
                .deliveries_by_agent
                .get("bramble")
                .copied()
                .unwrap_or(0)
                >= 12,
            "ox never completed a loaded cart trip: {:?}",
            sim.metrics
        );
        assert!(sim.metrics.delivered > 30, "{:?}", sim.metrics);
        assert!(sim.metrics.cycles > 8);
        assert_eq!(sim.metrics.equipped, 1);
        assert!(sim.metrics.kills >= 7);
        assert!(
            sim.health > 0,
            "settlement fell during the extended transport check"
        );
        assert_eq!(sim.agents["bramble"].cargo.capacity, 9);
        assert!(sim.balances().values().all(|b| b.conserved));
    }
}

/// Queue a precise shared-model transfer and walk to a reachable side of its dock.
pub fn player_transfer(
    world: &mut World,
    depot: &str,
    item: &str,
    units: u32,
    deposit: bool,
) -> Result<u32, String> {
    let start = world
        .query_filtered::<&Transform, With<crate::gameplay::Player>>()
        .single(world)
        .unwrap()
        .translation;
    let state = world.resource::<Settlement>();
    let sim = state.simulation.as_ref().ok_or("No settlement loaded")?;
    let actor = sim
        .config()
        .agents
        .iter()
        .find(|a| a.controlled)
        .ok_or("No controlled actor")?
        .id
        .clone();
    let dock = sim
        .config()
        .depots
        .iter()
        .find(|d| d.id == depot)
        .ok_or("Unknown depot")?
        .position;
    let goal = Vec3::new(dock[0], 0., dock[1]);
    let obstacles: Vec<_> = world
        .query::<&crate::gameplay::Obstacle>()
        .iter(world)
        .map(|o| o.0)
        .collect();
    let route = crate::navigation::path(start, goal, &obstacles);
    if route.is_empty() {
        return Err("Dock is unreachable".into());
    }
    let mut state = world.resource_mut::<Settlement>();
    let sim = state.simulation.as_mut().unwrap();
    let before = sim.agents[&actor].cargo.total();
    sim.transfer(&actor, depot, item, units, deposit)?;
    let expected = if deposit {
        before - units
    } else {
        before + units
    };
    state.notice = format!(
        "{} {units} {item} {} {depot}",
        if deposit { "Delivering" } else { "Collecting" },
        if deposit { "to" } else { "from" }
    );
    world.resource_mut::<crate::gameplay::Controls>().route = route;
    Ok(expected)
}
pub fn input(world: &mut World) {
    if world.resource::<Settlement>().simulation.is_none() {
        return;
    }
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let toggle = keys.just_pressed(KeyCode::KeyE);
    let deposit = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let manual = world
        .resource::<ButtonInput<MouseButton>>()
        .pressed(MouseButton::Right)
        || [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD]
            .iter()
            .any(|k| keys.pressed(*k));
    if manual {
        let mut state = world.resource_mut::<Settlement>();
        let sim = state.simulation.as_mut().unwrap();
        if let Some(id) = sim
            .config()
            .agents
            .iter()
            .find(|a| a.controlled)
            .map(|a| a.id.clone())
        {
            if sim.manual_jobs.contains_key(&id) {
                sim.cancel_transfer(&id).unwrap();
                state.notice = "Transfer canceled; airborne items still arrive.".into();
            }
        }
    }
    if toggle {
        let p = world
            .query_filtered::<&Transform, With<crate::gameplay::Player>>()
            .single(world)
            .unwrap()
            .translation;
        let mut state = world.resource_mut::<Settlement>();
        let sim = state.simulation.as_mut().unwrap();
        let nearest = sim
            .config()
            .producers
            .iter()
            .filter(|m| Vec2::new(m.position[0] - p.x, m.position[1] - p.z).length() < 2.5)
            .min_by(|a, b| {
                Vec2::new(a.position[0] - p.x, a.position[1] - p.z)
                    .length_squared()
                    .total_cmp(
                        &Vec2::new(b.position[0] - p.x, b.position[1] - p.z).length_squared(),
                    )
            })
            .map(|m| m.id.clone());
        if let Some(id) = nearest {
            let enabled = !sim.producers[&id].enabled;
            sim.set_machine(&id, enabled).unwrap();
            state.notice = format!("{id} {}", if enabled { "enabled" } else { "paused" });
        }
    }
    if shift
        || !world
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left)
    {
        return;
    }
    let mut windows = world.query::<&Window>();
    let mut cameras = world.query_filtered::<(&Camera, &GlobalTransform), With<Camera3d>>();
    let Some(window) = windows.iter(world).next() else {
        return;
    };
    let Some((camera, transform)) = cameras.iter(world).next() else {
        return;
    };
    let Some(p) = crate::gameplay::cursor_ground(camera, transform, window) else {
        return;
    };
    let state = world.resource::<Settlement>();
    let sim = state.simulation.as_ref().unwrap();
    let Some(actor) = sim.config().agents.iter().find(|a| a.controlled) else {
        return;
    };
    let Some(depot) = sim
        .config()
        .depots
        .iter()
        .filter(|d| Vec2::new(d.position[0] - p.x, d.position[1] - p.z).length() < 0.9)
        .min_by(|a, b| {
            Vec2::new(a.position[0] - p.x, a.position[1] - p.z)
                .length_squared()
                .total_cmp(&Vec2::new(b.position[0] - p.x, b.position[1] - p.z).length_squared())
        })
    else {
        return;
    };
    let stock = if deposit {
        &sim.agents[&actor.id].cargo
    } else {
        &sim.depots[&depot.id]
    };
    let Some(item) = stock
        .items
        .keys()
        .find(|i| sim.transfer_limit(&actor.id, &depot.id, i, deposit) > 0)
        .cloned()
    else {
        return;
    };
    let units = sim
        .transfer_limit(&actor.id, &depot.id, &item, deposit)
        .min(if deposit { 64 } else { 4 });
    let depot = depot.id.clone();
    if let Err(e) = player_transfer(world, &depot, &item, units, deposit) {
        world.resource_mut::<Settlement>().notice = e;
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn arrows_attach_corpses_fade_and_scene_exit_releases_temporary_materials() {
        let options = crate::control::Options {
            simulation: true,
            ..default()
        };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut library = SceneLibrary::new();
        library.initial = 6;
        let mut app = crate::build_app(options, library, hub);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 30.),
        ));
        app.update();
        let blends = |world: &World| {
            world
                .resource::<Assets<StandardMaterial>>()
                .iter()
                .filter(|(_, m)| m.alpha_mode == AlphaMode::Blend)
                .count()
        };
        let initial_blends = blends(app.world());
        let mut attached = false;
        let mut corpse = false;
        for _ in 0..1800 {
            app.update();
            let state = app.world().resource::<Settlement>();
            attached |= !state.attached.is_empty();
            corpse |= !state.dying.is_empty();
            for rig in state
                .views
                .values()
                .map(|(_, rig)| rig)
                .chain(state.pool.values().flatten())
            {
                // Every bone between the root and mesh must propagate inherited visibility.
                if let Some(children) = app.world().get::<Children>(rig.root) {
                    for e in children.iter() {
                        assert!(app.world().get::<Visibility>(e).is_some());
                    }
                }
            }
        }
        assert!(attached);
        assert!(corpse);
        inspect_building(app.world_mut(), Some("sawmill")).unwrap();
        assert!(inspect_building(app.world_mut(), Some("unknown")).is_err());
        app.world_mut().resource_mut::<SceneLibrary>().requested = Some(0);
        app.update();
        let state = app.world().resource::<Settlement>();
        assert!(state.simulation.is_none());
        assert!(state.inspection.is_none());
        assert!(state.dying.is_empty());
        assert!(state.attached.is_empty());
        assert_eq!(
            blends(app.world()),
            initial_blends,
            "temporary fade materials leaked across scene exit"
        );
    }
}

//! Bevy projection of the settlement. All stock and decisions belong to the
//! lockstep simulation; this module draws its mirror (`crate::legacy`) and
//! turns clicks into commands.
use crate::{
    assets::SceneLibrary,
    rig::{Fade, Rig, RigTemplate},
};
use bevy::prelude::*;
use crate::legacy::{Endpoint, Inventory, Simulation, HZ};
use std::collections::BTreeMap;
#[derive(Resource, Default)]
pub struct Settlement {
    current: Option<usize>,
    /// This frame's view of the simulated settlement.
    pub simulation: Option<Simulation>,
    /// The scene's scenario, as the view sees it.
    pub config: Option<crate::legacy::Scenario>,
    views: BTreeMap<String, (String, Rig)>,
    templates: BTreeMap<String, RigTemplate>,
    pool: BTreeMap<String, Vec<Rig>>,
    labels: BTreeMap<String, Entity>,
    hud: Option<Entity>,
    last_sound: u64,
    last_attack_serial: u64,
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
    injury: f32,
    recoil: f32,
    aim: Option<f32>,
    pierced: bool,
    armored: bool,
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
            injury: 0.,
            recoil: 0.,
            aim: None,
            pierced: false,
            armored: true,
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
    state.last_attack_serial = 0;
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
    let has_weapons = world.resource::<SceneLibrary>().scenes[index]
        .scene
        .scenario
        .as_ref()
        .is_some_and(|s| !s.weapons.is_empty());
    state.notice = if has_weapons {
        "RIGHT CLICK: MOVE  /  SHIFT+CLICK: ATTACK  /  Z SWORD  X SPEAR  C BOW  V ARC COIL".into()
    } else {
        "LMB stock: take 4 / Ctrl+LMB: deposit cargo / E: toggle nearby machine".into()
    };
    state.current = Some(index);
    let scenario = world.resource::<SceneLibrary>().scenes[index].scene.scenario.clone();
    if scenario.is_some() {
        let pack = &world.resource::<SceneLibrary>().scenes[index];
        let skins = pack.skins.clone();
        let profile = pack.scene.style.clone().unwrap_or_default();
        for skin in &skins {
            state.templates.insert(skin.id.clone(), RigTemplate::upload(world, skin, &profile));
        }
    }
    state.config = scenario.as_ref().map(crate::legacy::Scenario::new);
    state.simulation = None;
    if let Some(config) = state.config.clone() {
        for d in &config.depots {
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
                        font_size: FontSize::Px(12.),
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
        for (key, position) in config
            .producers
            .iter()
            .map(|p| (format!("producer/{}", p.id), p.position))
            .chain(
                config
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
                        font_size: FontSize::Px(14.),
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
                        font_size: FontSize::Px(14.),
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
    layout: &content::scenario::CargoLayout,
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
    if world
        .get_resource::<crate::control::Hub>()
        .is_some_and(|hub| hub.warming_up())
    {
        return;
    }
    if world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyP) {
        let mut game = world.resource_mut::<crate::lockstep::Game>();
        game.paused = !game.paused;
    }
    let mut state = world.remove_resource::<Settlement>().unwrap();
    let Some(sim) = &state.simulation else {
        world.insert_resource(state);
        return;
    };
    let sim = sim.clone();
    let sim = &sim;
    let pos = world
        .query_filtered::<&Transform, With<crate::gameplay::Player>>()
        .single(world)
        .unwrap()
        .translation;
    let projection_start = std::time::Instant::now();
    let hits = sim.events.iter().filter(|event| matches!(event, crate::legacy::Event::Hit { projectile: None, .. })).count() as u32;
    let projection_start = std::time::Instant::now();
    if hits > 0 && world.resource::<crate::gameplay::Controls>().attack
        .is_some_and(|a| matches!(a.weapon, content::scenario::Weapon::Sword | content::scenario::Weapon::Spear)) {
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
    let mut arc_source = pos + Vec3::Y * 0.75;
    let mut arc_fired = false;
    for event in &sim.events {
        use crate::legacy::Event;
        if let Event::ArcFired { from, to } = event {
            arc_source = Vec3::new(from[0], 0.72, from[1]);
            crate::effects::arc(
                world,
                arc_source,
                Vec3::new(to[0], 0.72, to[1]),
                sim.tick as u32,
            );
            arc_fired = true;
        }
        if let Event::Hit {
            target,
            projectile,
            killed,
            position,
            impact,
        } = event
        {
            let direction =
                Vec3::new(position[0] - pos.x, 0.0, position[1] - pos.z).normalize_or(Vec3::Z);
            if *impact == crate::legacy::Impact::Arc {
                let end = Vec3::new(position[0], 0.72, position[1]);
                if !arc_fired {
                    crate::effects::arc(world, arc_source, end, *target as u32);
                }
                arc_fired = false;
                arc_source = end;
            } else if *impact == crate::legacy::Impact::Deflect {
                crate::effects::deflection(
                    world,
                    Vec3::new(position[0], 0.72, position[1]),
                    *target as u32,
                );
            } else {
                crate::effects::blood(
                    world,
                    Vec3::new(position[0], 0.65, position[1]),
                    direction,
                    (*target as u32).wrapping_add(sim.tick as u32),
                );
            }
            let key = format!("threat/{target}");
            if let (Some(projectile), Some((_, body))) = (projectile, state.views.get(&key)) {
                if matches!(
                    impact,
                    crate::legacy::Impact::Arrow | crate::legacy::Impact::BoltImpale
                ) {
                    let parent = body.root;
                    if let Some((_, arrow)) =
                        state.views.remove(&format!("projectile/{projectile}"))
                    {
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
            }
            if *impact == crate::legacy::Impact::Decapitate {
                if let Some((skin, rig)) = state.views.get(&key) {
                    if let (Some(template), Some(transform)) = (
                        state.templates.get(skin),
                        rig.part_world_transform(world, voxel_assets::ActorPart::Head),
                    ) {
                        crate::effects::severed_head(world, template, transform, *target as u32);
                    }
                    rig.show_part(world, voxel_assets::ActorPart::Head, false);
                }
            }
            if *killed && *impact != crate::legacy::Impact::BoltImpale {
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
                state.hit_until.insert(*target, sim.tick + (0.23 * HZ) as u64);
            }
        }
        if let Event::PinnedLanded { target, .. } = event {
            let key = format!("threat/{target}");
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
        }
        if let Event::BlockedShot { position } = event {
            crate::effects::deflection(
                world,
                Vec3::new(position[0], 0.3, position[1]),
                sim.tick as u32,
            );
        }
        let cue = match event {
            Event::Shot { defender } if defender == "player" => Some(crate::sound::Cue::Bow),
            Event::Shot { .. } => Some(crate::sound::Cue::Ballista),
            Event::ArcFired { .. } => Some(crate::sound::Cue::Arc),
            Event::Hit {
                impact: crate::legacy::Impact::Arc,
                ..
            } => Some(crate::sound::Cue::GhoulHit),
            Event::Hit {
                impact: crate::legacy::Impact::Deflect,
                ..
            } => Some(crate::sound::Cue::Clang),
            Event::Hit {
                impact: crate::legacy::Impact::Spear,
                ..
            } => Some(crate::sound::Cue::Spear),
            Event::Hit {
                impact: crate::legacy::Impact::BoltPierce,
                ..
            } => Some(crate::sound::Cue::Pierce),
            Event::Hit {
                impact: crate::legacy::Impact::BoltImpale,
                ..
            } => Some(crate::sound::Cue::Pierce),
            Event::Hit {
                impact: crate::legacy::Impact::Decapitate,
                ..
            } => Some(crate::sound::Cue::Decapitate),
            Event::Hit {
                impact: crate::legacy::Impact::Arrow,
                ..
            } => Some(crate::sound::Cue::ArrowHit),
            Event::Hit {
                projectile: None, ..
            } => Some(crate::sound::Cue::GhoulHit),
            Event::Hit { .. } => Some(crate::sound::Cue::Stone),
            Event::PinnedLanded { .. } => Some(crate::sound::Cue::GhoulHit),
            Event::BlockedShot { .. } => Some(crate::sound::Cue::Stone),
            Event::Produced { .. } | Event::Equipped { .. } => Some(crate::sound::Cue::Clang),
            _ => None,
        };
        if let Some(cue) = cue {
            let combat_event = matches!(
                event,
                Event::Shot { .. }
                    | Event::ArcFired { .. }
                    | Event::Hit { .. }
                    | Event::PinnedLanded { .. }
                    | Event::BlockedShot { .. }
            );
            if combat_event || sim.tick >= state.last_sound + (0.3 * HZ) as u64 {
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
                1. - machine.remaining as f32 / (config.seconds * HZ)
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
        v.activity = (1. - (sim.time() - d.fired_at as f32) / (0.25 * HZ)).max(0.);
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
                ((*until as f32 - sim.time()) / (0.23 * HZ)).max(if wounded { 0.58 } else { 0.0 })
            });
        if wounded {
            v.injury = 1.0;
            v.roll = 0.18 + (t.travelled * 13.0).sin() * 0.05;
        }
        v.pierced = t.pierced;
        v.armored = t.armor > 0;
        v.recoil = if sim.tick < t.stagger_until
            && t.knockback.iter().map(|v| v * v).sum::<f32>() > 0.04
        {
            1.0
        } else {
            0.0
        };
        desired.insert(format!("threat/{}", t.id), v);
    }
    for p in sim.pinned.values() {
        let mut v = View::new(&sim.config().waves[p.wave].skin, p.position);
        let f = ((sim.time() - p.born as f32) / (p.land - p.born) as f32).clamp(0., 1.);
        v.position.y = (std::f32::consts::PI * f).sin() * 0.8;
        v.heading = p.heading;
        v.roll = -f * 0.8;
        desired.insert(format!("threat/{}", p.target), v);
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
        let f = ((sim.time() - t.born as f32) / (t.arrival - t.born) as f32).clamp(0., 1.);
        let mut v = View::new(&item.skin, [0., 0.]);
        v.position = endpoint(sim, &t.from).lerp(endpoint(sim, &t.to), f)
            + Vec3::Y * (std::f32::consts::PI * f).sin() * 0.9;
        v.heading = f * 4.;
        v.scale = item.scale;
        desired.insert(format!("transfer/{}", t.id), v);
    }
    for p in sim.projectiles.values() {
        let item = sim.config().items.iter().find(|i| i.id == p.item).unwrap();
        let f = ((sim.time() - p.born as f32) / (p.arrival - p.born) as f32).clamp(0., 1.);
        let mut v = View::new(&item.skin, [0., 0.]);
        v.position = Vec3::new(p.from[0], p.launch_height, p.from[1])
            .lerp(Vec3::new(p.to[0], 0.55, p.to[1]), f);
        v.heading = -(p.to[1] - p.from[1]).atan2(p.to[0] - p.from[0]);
        v.scale = 0.65;
        desired.insert(format!("projectile/{}", p.id), v);
    }
    for p in sim.spent_projectiles.values() {
        let item = sim.config().items.iter().find(|i| i.id == p.item).unwrap();
        let f = ((sim.time() - p.born as f32) / (p.arrival - p.born) as f32).clamp(0., 1.);
        let mut v = View::new(&item.skin, [0., 0.]);
        v.position =
            Vec3::new(p.from[0], 0.55, p.from[1]).lerp(Vec3::new(p.to[0], 0.09, p.to[1]), f);
        v.heading = -(p.to[1] - p.from[1]).atan2(p.to[0] - p.from[0]);
        v.scale = 0.65;
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
        let line = if !sim.config().weapons.is_empty() {
            format!(
                "{:?}  /  {} SLAIN  /  {} SWORD HITS  /  {} APPROACHING",
                world.resource::<crate::gameplay::Controls>().weapon,
                sim.metrics.kills,
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
        let age = (sim.time() - corpse.born as f32) / HZ;
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
        rig.show_part(world, voxel_assets::ActorPart::WoundPlug, !view.pierced);
        rig.show_part(world, voxel_assets::ActorPart::Armor, view.armored);
        rig.show_part(world, voxel_assets::ActorPart::Head, true);
        let (changed, unchanged) = rig.pose(
            world,
            view.position,
            view.heading,
            view.roll,
            template.scale * view.scale,
            view.travel,
            view.activity,
            view.injury,
            view.recoil,
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
    let fingerprint = world.resource::<crate::lockstep::Game>().fingerprint();
    world.resource::<Settlement>().simulation.as_ref().map_or(serde_json::Value::Null, |s| {
        serde_json::json!({"simulation": s, "replay_fingerprint": fingerprint, "balances": s.balances(), "scenario": s.config(), "seconds": s.seconds()})
    })
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
fn producer_description(sim: &Simulation, p: &crate::legacy::Producer) -> String {
    let recipe = |stacks: &[crate::legacy::Stack]| {
        stacks
            .iter()
            .map(|s| format!("{} {}", s.units, s.item))
            .collect::<Vec<_>>()
            .join(" + ")
    };
    let state = &sim.producers[&p.id];
    let status = match sim.production_status(&p.id).unwrap() {
        "processing" => format!("Working · {:.1}s remaining", state.remaining as f32 / HZ),
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
        font.font_size = FontSize::Px(if expanded { 14. } else { 10. });
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

#[cfg(test)]
mod tests {
    use super::*;
    use fx::{Fx, FxVec2};

    /// The forest outpost as the game builds it: its scenario plus the scene's
    /// obstacles, run for 100 seconds.
    #[test]
    fn authored_supply_chain_builds_transport_and_defends_with_conserved_materials() {
        let library = SceneLibrary::new();
        let pack = &library.scenes[6];
        let obstacles: Vec<_> = pack
            .environment
            .as_ref()
            .unwrap()
            .obstacles
            .iter()
            .map(|(p, r)| sim::terrain::Disc { center: FxVec2::new(Fx::quantize(p[0]), Fx::quantize(p[2])), radius: Fx::quantize(*r) })
            .collect();
        let scenario = pack.scene.scenario.clone().unwrap();
        let world = sim::World::new_settlement(1, content::Content::builtin().rules, &scenario, &obstacles).unwrap();
        let center = world.terrain().center;
        let mut game = sim::Simulation::from_world(world, 2);
        let config = crate::legacy::Scenario::new(&scenario);
        for tick in 0..3000 {
            game.step(&[]);
            let s = game.world().settlement().unwrap();
            s.validate().unwrap();
            if tick == 1799 {
                assert_eq!(s.health, 20, "first minute defense failed");
            }
            let a = s.agent("bramble");
            if let Some(t) = &a.trailer {
                assert!(sim::nav::distance(a.position, t.position) >= Fx::from_ratio(29, 20), "ox overlapped the cart at tick {tick}");
            }
        }
        let s = game.world().settlement().unwrap();
        let view = Simulation::new(&config, None, s, game.world().tick(), 0.0, center, &[]);
        let p = &view.config().producers[0];
        let card = producer_description(&view, p);
        assert!(card.contains(&p.description));
        assert!(card.contains("Consumes: 1 lumber + 1 iron"));
        assert!(card.contains("Produces: 4 arrow"));
        assert!(card.contains("Cycle: 4.0s"));
        assert!(
            view.metrics.deliveries_by_agent.get("bramble").copied().unwrap_or(0) >= 12,
            "ox never completed a loaded cart trip: {:?}",
            view.metrics
        );
        assert!(view.metrics.delivered > 30, "{:?}", view.metrics);
        assert!(view.metrics.cycles > 8);
        assert_eq!(view.metrics.equipped, 1);
        assert!(view.metrics.kills >= 7, "{:?}", view.metrics);
        assert!(view.health > 0, "settlement fell during the extended transport check");
        assert_eq!(view.agents["bramble"].cargo.capacity, 9);
        assert!(view.balances().values().all(|b| b.conserved));
    }
}

/// Reserve an exact transfer and walk to the dock. Returns the cargo the
/// player will carry once it completes.
pub fn player_transfer(world: &mut World, depot: &str, item: &str, units: u32, deposit: bool) -> Result<u32, String> {
    let state = world.resource::<Settlement>();
    let sim = state.simulation.as_ref().ok_or("No settlement loaded")?;
    let actor = sim.config().agents.iter().find(|a| a.controlled).ok_or("No controlled actor")?.id.clone();
    let dock = sim.config().depots.iter().find(|d| d.id == depot).ok_or("Unknown depot")?.position;
    let before = sim.agents[&actor].cargo.total();
    let game = world.resource::<crate::lockstep::Game>();
    let current = game.settlement().ok_or("No settlement loaded")?;
    let (Some(agent), Some(d), Some(i)) = (game.player(), current.depot_index(depot), current.item_index(item)) else {
        return Err("Unknown depot or item".into());
    };
    if sim.manual_jobs.contains_key(&actor) {
        return Err("actor already has a transfer in progress".into());
    }
    let limit = current.transfer_limit(agent as usize, d as usize, i as usize, deposit);
    if units == 0 || limit < units {
        return Err(if deposit {
            "cargo or unreserved destination space is insufficient".into()
        } else {
            "unreserved stock or cargo space is insufficient".into()
        });
    }
    let expected = if deposit { before - units } else { before + units };
    let mut game = world.resource_mut::<crate::lockstep::Game>();
    game.command(sim::CommandKind::Transfer { agent, depot: d, item: i, units, deposit });
    game.go_to(Vec3::new(dock[0], 0., dock[1]), false);
    world.resource_mut::<Settlement>().notice = format!(
        "{} {units} {item} {} {depot}",
        if deposit { "Delivering" } else { "Collecting" },
        if deposit { "to" } else { "from" }
    );
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
        let busy = world.resource::<Settlement>().simulation.as_ref().is_some_and(|s| !s.manual_jobs.is_empty());
        let agent = world.resource::<crate::lockstep::Game>().player();
        if let (true, Some(agent)) = (busy, agent) {
            world.resource_mut::<crate::lockstep::Game>().command(sim::CommandKind::CancelTransfer { agent });
            world.resource_mut::<Settlement>().notice = "Transfer canceled; airborne items still arrive.".into();
        }
    }
    if toggle {
        let p = world
            .query_filtered::<&Transform, With<crate::gameplay::Player>>()
            .single(world)
            .unwrap()
            .translation;
        let state = world.resource::<Settlement>();
        let sim = state.simulation.as_ref().unwrap();
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
            let producer = world.resource::<crate::lockstep::Game>().settlement().and_then(|s| s.producer_index(&id));
            if let Some(producer) = producer {
                world.resource_mut::<crate::lockstep::Game>().command(sim::CommandKind::SetMachine { producer, enabled });
            }
            world.resource_mut::<Settlement>().notice = format!("{id} {}", if enabled { "enabled" } else { "paused" });
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
    let game = world.resource::<crate::lockstep::Game>();
    let Some(current) = game.settlement() else {
        return;
    };
    let (Some(agent), Some(d)) = (game.player(), current.depot_index(&depot.id)) else {
        return;
    };
    let limit = |item: &str| current.item_index(item).map_or(0, |i| current.transfer_limit(agent as usize, d as usize, i as usize, deposit));
    let Some(item) = stock.items.keys().find(|i| limit(i) > 0).cloned() else {
        return;
    };
    let units = limit(&item).min(if deposit { 64 } else { 4 });
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

//! Bevy projection of the settlement. All stock and decisions belong to the
//! lockstep simulation; this module draws its mirror (`crate::legacy`) and
//! turns clicks into commands.
use crate::{
    assets::SceneLibrary,
    rig::{Fade, Rig, RigTemplate},
};
use bevy::prelude::*;
use crate::legacy::{Endpoint, Inventory, Simulation, HZ};
use service::{AppServices, Phase, Service, WorldLayer, WorldState};
use std::collections::BTreeMap;
/// The settlement's view (World layer).
#[derive(Resource, Default)]
pub struct Settlement {
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
    /// The town's glows (sprites' flames, buildings' lamps) by key.
    glows: BTreeMap<String, Entity>,
    glow_assets: Option<GlowAssets>,
    /// How each building kind's skin is fitted to its footprint.
    building_shapes: BTreeMap<String, BuildingShape>,
    /// The tick each building last put out a unit, for its lamp's flash.
    produced_at: BTreeMap<usize, u64>,
    /// The tick each building last had a Burst, for its lamp's bigger flash.
    burst_at: BTreeMap<usize, u64>,
    /// The elder's last interact refusal and the tick it came, shown over
    /// its head for `REFUSAL_SECONDS`.
    refusal: Option<(String, f32)>,
    /// A blueprint's ghost: its skin, its rig, and the rig's materials made
    /// see-through and tinted.
    ghost: Option<(String, Rig, Fade)>,
    /// The tree's canopy, split off to fade while it hides the elder, a site
    /// or the ghost (`canopy`); `None` until the town has been drawn.
    canopy: Option<crate::canopy::Canopy>,
}
/// How long an interact refusal stays over the elder.
const REFUSAL_SECONDS: f32 = 2.5;
impl Settlement {
    /// Shows `reason` over the player's head for a moment: an interact
    /// refused by the sim (`Event::BundleRefused`) or before it was sent.
    pub fn refuse(&mut self, reason: &str) {
        let now = self.simulation.as_ref().map_or(0.0, Simulation::time);
        self.refusal = Some((reason.to_string(), now));
    }
}
struct Corpse {
    skin: String,
    rig: Rig,
    born: u64,
    rest: Transform,
    fade: Fade,
    arrows: Vec<Rig>,
}
/// A label in the world. Folded, a depot's shows its unit count and a
/// building's card is hidden; one with a `brief` shows that instead, in its
/// `tint`. Hovered (or pinned), it unfolds to `detail`.
#[derive(Component)]
pub struct Caption {
    /// A card with no take or deposit hint, hidden folded unless it has a
    /// brief.
    building: bool,
    position: Vec3,
    detail: String,
    units: u32,
    brief: String,
    tint: Color,
}

/// The glows' shared mesh and a material per state.
struct GlowAssets {
    mesh: Handle<Mesh>,
    materials: [Handle<StandardMaterial>; GLOWS.len()],
}

/// What a glow shows, an index into `GLOWS`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glow {
    Working = 0,
    Moving = 1,
    Waiting = 2,
    Idle = 3,
    Full = 4,
    Elder = 5,
}

/// The glows' colors, by `Glow`: the warm flame of work, a pale yellow
/// walk, the cool blue of waiting, a dim ember for idle, red for an output
/// that is full, and violet for the elder.
const GLOWS: [(f32, f32, f32); 6] = [
    (1.0, 0.55, 0.12),
    (1.0, 0.86, 0.45),
    (0.35, 0.62, 1.0),
    (0.45, 0.36, 0.30),
    (1.0, 0.16, 0.10),
    (0.78, 0.45, 1.0),
];

impl Glow {
    /// The color of text in this state: the glow's, but an idle ember's
    /// would be too dark to read, so idle text is a pale gray.
    fn text(self) -> Color {
        if self == Glow::Idle {
            return Color::srgb(0.82, 0.80, 0.74);
        }
        let (r, g, b) = GLOWS[self as usize];
        Color::srgb(r, g, b)
    }
}

/// A building kind's skin fitted to the kind's footprint.
#[derive(Clone, Copy)]
struct BuildingShape {
    /// The view scale that fits the skin's body inside the footprint.
    scale: f32,
    /// The fitted skin's height in meters.
    height: f32,
}

/// How a building's output is stacked: three across and two deep, then
/// layer on layer.
const OUTPUT_LAYOUT: content::scenario::CargoLayout =
    content::scenario::CargoLayout { columns: 3, rows: 2, spacing: [0.2, 0.11, 0.17], scale: 1.0, roll: 0.0 };
/// Seconds a lamp flares after its building puts out a unit.
const FLARE_SECONDS: f32 = 0.45;
/// Seconds a lamp flares after a Burst, brighter than a unit's.
const BURST_FLARE_SECONDS: f32 = 1.2;
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
    strike: crate::rig::Strike,
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
            strike: crate::rig::Strike::default(),
            aim: None,
            pierced: false,
            armored: true,
        }
    }
}
impl Service for Settlement {
    type Layer = WorldLayer;
    type Deps = (crate::lockstep::WorldMirror, crate::scene_view::SceneView);

    fn init(world: &mut World) -> Self {
        build(world)
    }

    /// Despawns the world's rigs, captions and HUD, and restores the
    /// materials fading corpses borrowed.
    fn shutdown(mut self, world: &mut World) {
        for (_, (_, rig)) in std::mem::take(&mut self.views) {
            world.despawn(rig.root);
        }
        for (_, rigs) in std::mem::take(&mut self.pool) {
            for rig in rigs {
                world.despawn(rig.root);
            }
        }
        for corpse in self.dying.drain(..) {
            corpse.fade.restore(world);
            world.despawn(corpse.rig.root);
        }
        for (_, e) in std::mem::take(&mut self.labels) {
            world.despawn(e);
        }
        if let Some(e) = self.hud.take() {
            world.despawn(e);
        }
        for e in self.blockers.drain(..) {
            world.despawn(e);
        }
        for (_, e) in std::mem::take(&mut self.glows) {
            world.despawn(e);
        }
        if let Some((_, rig, fade)) = self.ghost.take() {
            fade.restore(world);
            world.despawn(rig.root);
        }
        if let Some(glows) = self.glow_assets.take() {
            world.resource_mut::<Assets<Mesh>>().remove(&glows.mesh);
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            for material in &glows.materials {
                materials.remove(material);
            }
        }
    }

    fn systems(app: &mut App) {
        let active = in_state(WorldState::Active);
        app.add_service_systems::<Self, _>(Phase::Input, input.run_if(crate::control::interactive).run_if(active.clone()))
            .add_service_systems::<Self, _>(Phase::Mirror, crate::lockstep::mirror_settlement.run_if(active.clone()))
            .add_service_systems::<Self, _>(Phase::Present, tick.run_if(active.clone()))
            .add_systems(
                PostUpdate,
                captions.after(bevy::transform::TransformSystems::Propagate).run_if(crate::control::rendered).run_if(active),
            );
    }
}

/// The settlement's view for a world: rig templates for the scene's skins,
/// and captions and a HUD for its scenario.
fn build(world: &mut World) -> Settlement {
    let index = world.resource::<crate::scene_view::SceneView>().index;
    let mut state = Settlement::default();
    let has_weapons = world.resource::<SceneLibrary>().scenes[index]
        .scene
        .scenario
        .as_ref()
        .is_some_and(|s| !s.weapons.is_empty());
    state.notice = if has_weapons {
        "WASD: MOVE  /  SHIFT: RUN  /  LEFT CLICK: ATTACK  /  Z SWORD  X SPEAR  C BOW  V ARC COIL".into()
    } else {
        "LMB stock: take 4 / Ctrl+LMB: deposit cargo / E: toggle nearby machine".into()
    };
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
    if let Some(config) = state.config.clone().filter(|c| !c.building_kinds.is_empty() || !c.sprites.is_empty()) {
        let pack = world.resource::<SceneLibrary>().scenes[index].clone();
        for kind in &config.building_kinds {
            let skin = pack.skins.iter().find(|s| s.id == kind.skin).expect("validated building skin");
            state.building_shapes.insert(kind.id.clone(), building_shape(skin, state.templates[&kind.skin].scale, kind.radius));
        }
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(1.0).mesh().ico(2).unwrap());
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let materials = GLOWS.map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                emissive: LinearRgba::rgb(r * 6.0, g * 6.0, b * 6.0),
                unlit: true,
                ..default()
            })
        });
        state.glow_assets = Some(GlowAssets { mesh, materials });
    }
    if let Some(config) = state.config.clone() {
        for d in &config.depots {
            let e = world
                .spawn((
                    Caption {
                        building: false,
                        position: Vec3::new(d.position[0], 0.7, d.position[1]),
                        detail: String::new(),
                        units: 0,
                        brief: String::new(),
                        tint: Color::NONE,
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
                        brief: String::new(),
                        tint: Color::NONE,
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
    state
}
/// Ticks a threat's swing takes to follow through after it lands.
const SWING_TICKS: f32 = 0.3 * HZ;
/// How far a threat's swing is drawn at `drawn` (`Simulation::drawn_time`,
/// the tick its position is drawn at): the windup builds from 0 when it
/// starts to 1 when it lands, then the swing follows through, from 1 back
/// to 0 over `SWING_TICKS`, hit or miss.
fn threat_strike(t: &crate::legacy::Threat, drawn: f32) -> crate::rig::Strike {
    let swing = t.swung.map_or(0.0, |landed| {
        let age = (drawn - landed as f32) / SWING_TICKS;
        // Held out, then drawn back.
        if (0.0..1.0).contains(&age) { 1.0 - age * age } else { 0.0 }
    });
    let windup = match &t.windup {
        Some(w) if swing == 0.0 => {
            let start = w.lands.saturating_sub(w.ticks) as f32;
            ((drawn - start) / w.ticks.max(1) as f32).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    crate::rig::Strike { windup, swing }
}
/// An agent's definition. Players who joined copy the scenario's avatar.
fn agent_config<'a>(sim: &'a Simulation, id: &str) -> &'a crate::legacy::Agent {
    let agents = &sim.config().agents;
    agents.iter().find(|c| c.id == id).or_else(|| agents.iter().find(|c| c.controlled)).expect("a scenario agent")
}
fn cargo_position(sim: &Simulation, id: &str) -> Vec3 {
    let a = &sim.agents[id];
    let config = agent_config(sim, id);
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
        Endpoint::Output(b) | Endpoint::Site(b) => {
            let p = sim.buildings[*b as usize].output_position;
            Vec3::new(p[0], 0., p[1])
        }
        Endpoint::Ground(id) => {
            let p = sim.ground_piles.iter().find(|p| p.id == *id).map_or([0., 0.], |p| p.position);
            Vec3::new(p[0], 0., p[1])
        }
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
/// The height arc coil arcs are drawn at.
pub const ARC_HEIGHT: f32 = 0.72;

/// The arc coil's arcs through one frame's events. An `ArcFired` arcs to its
/// first target, which its first `Hit { impact: Arc }` struck; each later arc
/// hit arcs on from the one before.
struct ArcChain {
    source: Vec3,
    /// An `ArcFired` drew (or prediction drew) the arc to the next hit.
    fired: bool,
}

impl ArcChain {
    fn new(source: Vec3) -> Self {
        ArcChain { source, fired: false }
    }

    /// A shot from `from` to `to`: the arc to draw, unless prediction already
    /// drew it (`presented`). Either way its hits chain on from it.
    fn fired(&mut self, from: Vec3, to: Vec3, presented: bool) -> Option<(Vec3, Vec3)> {
        self.source = from;
        self.fired = true;
        (!presented).then_some((from, to))
    }

    /// An arc hit at `end`: the arc to draw to it, if its shot didn't.
    fn hit(&mut self, end: Vec3) -> Option<(Vec3, Vec3)> {
        let arc = (!self.fired).then_some((self.source, end));
        self.fired = false;
        self.source = end;
        arc
    }
}

/// How fast a predicted arrow's flight runs, against the drawn time, until
/// its confirmed projectile catches up. The arrow is drawn an input lead
/// ahead of the confirmed world, like the avatar that looses it, but its
/// hits are the confirmed world's (stage 5). Flying at half pace, it meets
/// its confirmed projectile after twice the lead, a short way out of the
/// bow, and from then on is drawn where the projectile is, so it reaches a
/// ghoul when the hit is confirmed, not an input lead before.
pub const PREDICTED_ARROW_PACE: f32 = 0.5;

/// The local player's bow shots prediction presented
/// (`lockstep::predict_events`), as arrows in flight ahead of the confirmed
/// world. The matched confirmed shot's projectile takes its arrow over
/// (`take_over`): the same rig, drawn no further back than it was. One the
/// confirmed world never shoots is withdrawn, and one left unconfirmed
/// once it has come down is dropped.
#[derive(Resource, Default)]
pub struct PredictedArrows(pub Vec<PredictedArrow>);

#[derive(Clone, Debug, PartialEq)]
pub struct PredictedArrow {
    /// The presented event's id (`event_prediction`).
    pub id: u64,
    /// The tick the replay fires it on.
    pub fired: u64,
    /// Where it flies, as `legacy::Projectile`: from `from`, `height` up,
    /// toward `to`, over `flight` ticks.
    pub from: crate::legacy::Point,
    pub to: crate::legacy::Point,
    pub height: f32,
    pub flight: u64,
    /// The confirmed projectile that took it over.
    pub projectile: Option<u64>,
}

impl PredictedArrow {
    /// How many ticks into its flight it is drawn at drawn time `drawn` (on
    /// the projectiles' clock, `Simulation::time`) on its own: at
    /// `PREDICTED_ARROW_PACE`.
    pub fn age(&self, drawn: f32) -> f32 {
        (PREDICTED_ARROW_PACE * (drawn - self.fired as f32)).max(0.0)
    }

    /// Its view's key until a projectile takes it over.
    fn key(&self) -> String {
        format!("projectile/predicted/{}", self.id)
    }
}

impl PredictedArrows {
    /// The presented shot `id` was confirmed as `projectile`: it is drawn
    /// from now on as that projectile's view. Whether it was still drawn.
    pub fn take_over(&mut self, id: u64, projectile: u64) -> bool {
        self.0.iter_mut().find(|arrow| arrow.id == id && arrow.projectile.is_none()).map(|arrow| arrow.projectile = Some(projectile)).is_some()
    }

    /// The presented shot `id` won't be confirmed: it is no longer drawn.
    pub fn withdraw(&mut self, id: u64) {
        self.0.retain(|arrow| arrow.id != id);
    }

    /// How far into its flight the projectile `id`, `confirmed` ticks in,
    /// is drawn: no further back than the arrow it took over.
    pub fn projectile_age(&self, id: u64, confirmed: f32, drawn: f32) -> f32 {
        self.0.iter().find(|arrow| arrow.projectile == Some(id)).map_or(confirmed, |arrow| confirmed.max(arrow.age(drawn)))
    }

    /// Drops the arrows done flying at drawn time `drawn`: those taken over
    /// by a projectile the confirmed world no longer has, and those never
    /// confirmed that have come down.
    pub fn retain_flying(&mut self, drawn: f32, flying: impl Fn(u64) -> bool) {
        self.0.retain(|arrow| match arrow.projectile {
            Some(projectile) => flying(projectile),
            None => arrow.age(drawn) < arrow.flight as f32,
        });
    }
}

/// An arrow's view, `f` of the way from `from` (`height` up) to `to`.
fn arrow_view(skin: &str, from: crate::legacy::Point, height: f32, to: crate::legacy::Point, f: f32) -> View {
    let mut v = View::new(skin, [0., 0.]);
    v.position = Vec3::new(from[0], height, from[1]).lerp(Vec3::new(to[0], 0.55, to[1]), f);
    v.heading = -(to[1] - from[1]).atan2(to[0] - from[0]);
    v.scale = 0.65;
    v
}

pub fn tick(world: &mut World) {
    if world
        .get_resource::<crate::control::Hub>()
        .is_some_and(|hub| hub.in_warmup())
    {
        return;
    }
    // Networked games can't pause: everyone shares one clock.
    if world.resource::<debug_ui::GameInput>().keys().just_pressed(KeyCode::KeyP)
        && !world.resource::<crate::lockstep::SessionService>().networked()
    {
        let mut service = world.resource_mut::<crate::lockstep::SessionService>();
        service.paused = !service.paused;
    }
    let mut state = world.remove_resource::<Settlement>().unwrap();
    let Some(sim) = &state.simulation else {
        world.insert_resource(state);
        return;
    };
    let sim = sim.clone();
    let sim = &sim;
    let pos = world
        .query_filtered::<&Transform, With<crate::character::Player>>()
        .single(world)
        .unwrap()
        .translation;
    let me = world.resource::<crate::lockstep::WorldMirror>().player_id();
    // Hit pause, rebound and camera shake are for this player's own hits.
    let hits = sim
        .events
        .iter()
        .filter(|event| matches!(event, crate::legacy::Event::Hit { projectile: None, by, .. } if *by == me))
        .count() as u32;
    let projection_start = web_time::Instant::now();
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
    let mut arcs = ArcChain::new(pos + Vec3::Y * 0.75);
    // This player's events that prediction already presented, in order
    // (see `event_prediction`); other events are presented as they are
    // confirmed.
    let local = world.resource::<crate::lockstep::WorldMirror>().player();
    let mut presented_early = world
        .get_resource_mut::<crate::event_prediction::EventPrediction>()
        .map(|mut ledger| std::mem::take(&mut ledger.confirmed))
        .unwrap_or_default();
    for event in &sim.events {
        use crate::legacy::Event;
        let presented = match event {
            Event::LaserFired { agent, .. } if Some(*agent) == local => presented_early.next("laser"),
            Event::ArcFired { agent, .. } if Some(*agent) == local => presented_early.next("arc"),
            Event::BowFired { agent, .. } if Some(*agent) == local => presented_early.next("bow"),
            _ => None,
        };
        if let (Event::BowFired { projectile, .. }, Some(id)) = (event, presented) {
            // The arrow prediction drew flies on as this projectile's view.
            if let Some(mut arrows) = world.get_resource_mut::<PredictedArrows>() {
                let key = arrows.0.iter().find(|arrow| arrow.id == id).map(PredictedArrow::key);
                match (projectile, key) {
                    (Some(projectile), Some(key)) if arrows.take_over(id, *projectile) => {
                        if let Some(view) = state.views.remove(&key) {
                            let confirmed = format!("projectile/{projectile}");
                            if state.views.contains_key(&confirmed) {
                                world.despawn(view.1.root);
                            } else {
                                state.views.insert(confirmed, view);
                            }
                        }
                    }
                    _ => arrows.withdraw(id),
                }
            }
        }
        let drawn_shot = presented.filter(|_| matches!(event, Event::LaserFired { .. }));
        if let Event::ArcFired { from, to, .. } = event {
            let from = Vec3::new(from[0], ARC_HEIGHT, from[1]);
            let to = Vec3::new(to[0], ARC_HEIGHT, to[1]);
            if let Some((from, to)) = arcs.fired(from, to, presented.is_some()) {
                crate::effects::arc(world, from, to, sim.tick as u32);
            }
        }
        if let (Event::LaserFired { cover, .. }, Some(drawn)) = (event, drawn_shot) {
            // The drawn beam stays; its cover waits for it as it would for this one.
            if let Some(index) = cover {
                crate::effects::hand_over_cover(world, drawn, *index);
            }
        } else if let Event::LaserFired { agent, to, hit_actor, cover } = event {
            if let Some(index) = cover { world.resource_mut::<crate::effects::CoverContacts>().begin(*index as usize); }
            let height = world.resource::<crate::lockstep::SessionService>()
                .ground_height(Vec3::new(to[0], 0.0, to[1])).unwrap_or(0.0) + 0.8;
            world.resource_mut::<crate::effects::PendingLasers>().0.push(crate::effects::LaserShot {
                agent: *agent,
                target: Vec3::new(to[0], height, to[1]),
                hit_actor: *hit_actor,
                cover: *cover,
                seed: crate::effects::laser_impact_seed(sim.tick, *agent),
                predicted: None,
            });
        }
        if let Event::Hit {
            target,
            by,
            projectile,
            killed,
            position,
            impact,
        } = event
        {
            // Blood sprays away from whoever struck.
            let from = by
                .as_ref()
                .and_then(|id| sim.agents.get(id))
                .map_or(pos, |a| Vec3::new(a.position[0], 0.0, a.position[1]));
            let direction =
                Vec3::new(position[0] - from.x, 0.0, position[1] - from.z).normalize_or(Vec3::Z);
            if *impact == crate::legacy::Impact::Arc {
                let end = Vec3::new(position[0], ARC_HEIGHT, position[1]);
                if let Some((from, to)) = arcs.hit(end) {
                    crate::effects::arc(world, from, to, *target as u32);
                }
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
        if let Event::ThreatSwing { threat, agent, from, hit: true, killed } = event {
            // Blood off the player, away from the threat, where the player is
            // drawn: the local avatar is ahead of the confirmed world.
            let mine = me.as_deref() == Some(agent.as_str());
            let at = match sim.agents.get(agent) {
                Some(a) if !mine => {
                    let p = Vec3::new(a.position[0], 0.0, a.position[1]);
                    p + Vec3::Y
                        * crate::jetpack::height(&world.resource::<crate::scene_view::SceneView>().pack, p)
                            .unwrap_or(0.0)
                }
                _ => pos,
            };
            let from = from.map_or(at, |p| Vec3::new(p[0], at.y, p[1]));
            let direction = (at - from).with_y(0.0).normalize_or(Vec3::Z);
            crate::effects::blood(
                world,
                at + Vec3::Y * 0.9,
                direction,
                (*threat as u32).wrapping_add(sim.tick as u32),
            );
            if mine {
                let mut feedback = world.resource_mut::<crate::effects::Feedback>();
                feedback.trauma = feedback.trauma.max(if *killed { 0.45 } else { 0.3 });
            }
        }
        if let Event::BuildingProduced { building } = event {
            state.produced_at.insert(*building, sim.tick);
        }
        if let Event::BuildingBurst { building, .. } = event {
            state.burst_at.insert(*building, sim.tick);
        }
        if let Event::BurstRefused { agent, reason } = event {
            if Some(agent) == me.as_ref() {
                state.refusal = Some((reason.message(), sim.time()));
            }
        }
        if let Event::BundleRefused { agent, reason } = event {
            if Some(agent) == me.as_ref() {
                state.refusal = Some((reason.message().to_string(), sim.time()));
            }
        }
        // Nobody sends a blueprint but a player, so this one's is shown.
        if let Event::BlueprintRefused { reason } = event {
            state.refusal = Some((reason.message().to_string(), sim.time()));
        }
        let cue = match event {
            // Prediction played these when it presented the shot.
            Event::LaserFired { .. } | Event::ArcFired { .. } | Event::BowFired { .. } if presented.is_some() => None,
            Event::BowFired { .. } => Some(crate::sound::Cue::Bow),
            Event::Shot { .. } => Some(crate::sound::Cue::Ballista),
            Event::ArcFired { .. } => Some(crate::sound::Cue::Arc),
            // The visible bolt plays this cue when it reaches the actor.
            Event::Hit { impact: crate::legacy::Impact::Laser, .. } => None,
            Event::LaserFired { .. } => Some(crate::sound::Cue::Laser),
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
            Event::ThreatSwing { hit: true, .. } => Some(crate::sound::Cue::GhoulHit),
            Event::ThreatSwing { hit: false, .. } => Some(crate::sound::Cue::Swing),
            Event::Produced { .. } | Event::Equipped { .. } => Some(crate::sound::Cue::Clang),
            // A gatherer's unit pops onto its pile as a dispenser's does.
            // So do a Burst's, all at once.
            Event::BuildingProduced { .. } | Event::BuildingBurst { .. } => Some(crate::sound::Cue::Dispense),
            Event::BlueprintPlaced { .. } | Event::PartInstalled { .. } => Some(crate::sound::Cue::Clang),
            _ => None,
        };
        if let Some(cue) = cue {
            let combat_event = matches!(
                event,
                Event::Shot { .. }
                    | Event::BowFired { .. }
                    | Event::ArcFired { .. }
                    | Event::LaserFired { .. }
                    | Event::Hit { .. }
                    | Event::PinnedLanded { .. }
                    | Event::BlockedShot { .. }
                    | Event::ThreatSwing { .. }
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
    for a in sim.agents.values() {
        if let Some(sprite) = sim.config().sprite(&a.id) {
            desired.insert(format!("agent/{}", a.id), sprite_view(sim, sprite, a));
            continue;
        }
        let config = agent_config(sim, &a.id);
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
        v.strike = threat_strike(t, sim.drawn_time());
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
    let glows = town(world, &mut state, sim, &mut desired, pos);
    hide_possessing(world);
    town_interact(world, &mut state, sim, pos);
    if !sim.config().building_kinds.is_empty() {
        town_ghost(world, &mut state, sim);
        town_canopy(world, &mut state, sim, pos);
        town_help(world);
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
    // The local player's arrows prediction drew, on the projectiles' clock
    // at the drawn time.
    let drawn = world
        .get_resource::<crate::prediction::DrawnAvatar>()
        .and_then(|drawn| drawn.0.as_ref())
        .map_or(sim.time(), |drawn| drawn.time + 1.0);
    let mut arrows = world
        .get_resource_mut::<PredictedArrows>()
        .map(|mut arrows| std::mem::take(&mut *arrows))
        .unwrap_or_default();
    arrows.retain_flying(drawn, |id| sim.projectiles.contains_key(&id));
    for p in sim.projectiles.values() {
        let item = sim.config().items.iter().find(|i| i.id == p.item).unwrap();
        let age = arrows.projectile_age(p.id, sim.time() - p.born as f32, drawn);
        let f = (age / (p.arrival - p.born) as f32).clamp(0., 1.);
        desired.insert(format!("projectile/{}", p.id), arrow_view(&item.skin, p.from, p.launch_height, p.to, f));
    }
    if let Some(item) = sim.config().items.iter().find(|i| i.id == "arrow") {
        for arrow in arrows.0.iter().filter(|arrow| arrow.projectile.is_none()) {
            let f = (arrow.age(drawn) / arrow.flight.max(1) as f32).clamp(0., 1.);
            desired.insert(arrow.key(), arrow_view(&item.skin, arrow.from, arrow.height, arrow.to, f));
        }
    }
    if let Some(mut kept) = world.get_resource_mut::<PredictedArrows>() {
        *kept = arrows;
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
        // The town's piles say what they are, not only how much: the tree's
        // stock is the town's score.
        let town = !sim.config().building_kinds.is_empty();
        let mut caption = world.get_mut::<Caption>(state.labels[&d.id]).unwrap();
        if caption.detail != label {
            if town {
                caption.brief = format!("{} {}", d.label, stock.total());
                caption.tint = Color::srgb(0.98, 0.92, 0.70);
            }
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
        let mine = me.as_ref().and_then(|id| sim.agents.get(id));
        let cargo = mine
            .map(|a| {
                a.cargo
                    .items
                    .iter()
                    .map(|(i, n)| format!("{n} {i}"))
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let weapon = world.resource::<crate::gameplay::Controls>().weapon;
        let line = if weapon == content::scenario::Weapon::Laser {
            let mirror = world.resource::<crate::lockstep::WorldMirror>();
            let health = mirror.player()
                .and_then(|id| mirror.current.as_ref()?.agents.get(id as usize))
                .map_or_else(|| "--".to_string(), |agent| format!("{}/{}", agent.body.health, agent.body.max_health));
            let players = mirror.current.as_ref().map_or(0, |current| current.agents.iter().filter(|agent| agent.avatar().is_some()).count());
            format!("LASER  /  HEALTH {health}  /  {players} PLAYERS")
        } else if !sim.config().building_kinds.is_empty() {
            let tree = |item: &str| sim.config().depots.iter().map(|d| sim.depots[&d.id].units(item)).sum::<u32>();
            let items = sim.config().items.iter().map(|i| format!("{} {}", i.label, tree(&i.id))).collect::<Vec<_>>().join("  ");
            let burst = burst_indicator(world).map_or_else(String::new, |(_, burst)| format!("  /  {burst}"));
            format!(
                "SACRED TREE  {}  /  PACK {}/{} {}{}   [P] pause",
                items,
                mine.map_or(0, |a| a.cargo.total()),
                mine.map_or(0, |a| a.cargo.capacity),
                cargo,
                burst
            )
        } else if !sim.config().weapons.is_empty() {
            format!(
                "{:?}  /  {} SLAIN  /  {} MELEE HITS  /  {} APPROACHING",
                weapon,
                sim.metrics.kills,
                sim.metrics.melee_hits,
                sim.threats.len()
            )
        } else {
            format!(
                "BASE {}  /  {} GHOULS STOPPED  /  PACK {} /8 {}   [P] pause  [F3] metrics",
                sim.health,
                sim.metrics.kills,
                mine.map_or(0, |a| a.cargo.total()),
                cargo
            )
        };
        if world.get::<Text>(e).unwrap().0 != line {
            world.get_mut::<Text>(e).unwrap().0 = line;
        }
    }
    crate::telemetry::elapsed(world, "settlement_projection", projection_start);
    let sync_start = web_time::Instant::now();
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
            crate::jetpack::height(&world.resource::<crate::scene_view::SceneView>().pack, view.position).unwrap_or(0.);
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
        let (_, rig) = state.views.get_mut(&key).unwrap();
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
            view.strike,
            view.aim,
            None,
        );
        writes += changed;
        skipped += unchanged;
    }
    sync_glows(world, &mut state, glows);
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
/// Fits a building skin to its kind's footprint: its body, seen from
/// above, fills the footprint's width, so the slot spot and the output pile
/// beside it stay clear of the model. `template_scale` is the skin's own.
fn building_shape(skin: &voxel_assets::SkinMesh, template_scale: f32, radius: f32) -> BuildingShape {
    let positions = || {
        skin.parts
            .iter()
            .filter(|p| p.part == voxel_assets::ActorPart::Body)
            .flat_map(|p| p.meshes.iter().flat_map(|m| m.positions.iter()))
    };
    let half = positions().map(|p| p[0].abs().max(p[2].abs())).fold(0.0_f32, f32::max).max(0.01);
    let top = positions().map(|p| p[1]).fold(0.0_f32, f32::max);
    let scale = radius / (half * template_scale);
    BuildingShape { scale, height: top * template_scale * scale }
}
/// The heading that turns a skin's front (+z) toward `to` from `from`.
fn facing(from: [f32; 2], to: [f32; 2]) -> f32 {
    (to[0] - from[0]).atan2(to[1] - from[1])
}
/// A sprite's rig: its character's skin, each a little different in size
/// so the four read as separate. Working, it faces its building and bobs
/// with the work.
fn sprite_view(sim: &Simulation, sprite: &crate::legacy::Sprite, a: &crate::legacy::AgentState) -> View {
    const SIZES: [f32; 4] = [1.0, 0.9, 1.07, 0.95];
    let index = sim.config().sprites.iter().position(|s| s.id == sprite.id).unwrap_or(0);
    let mut view = View::new(&sprite.skin, a.position);
    view.heading = a.heading;
    view.travel = a.travelled;
    view.scale = SIZES[index % SIZES.len()];
    let state = sim.sprites.get(&a.id);
    if let Some(b) = state.filter(|s| s.activity == crate::legacy::SpriteActivity::Working).and_then(|s| s.building) {
        view.heading = facing(a.position, sim.buildings[b].position);
        view.activity = 0.5 + 0.5 * (sim.time() / HZ * 7.0 + index as f32).sin();
    }
    view
}
/// A sprite's job, by its building kind's label ("WOOD GATHERER"), and
/// shortened to the kind's product for its tag ("WOOD"), so neighbors' tags
/// overlap less. A ballista, which makes nothing, keeps its label.
fn job_labels(sim: &Simulation, job: Option<&String>) -> (String, String) {
    let Some(kind) = job.and_then(|id| sim.config().building_kinds.iter().find(|k| k.id == *id)) else {
        return (String::new(), String::new());
    };
    let Some(product) = &kind.product else { return (kind.label.clone(), kind.label.clone()) };
    let product = sim.config().items.iter().find(|i| i.id == *product).map_or_else(|| product.to_uppercase(), |i| i.label.clone());
    (kind.label.clone(), product)
}
/// An agent's label: a sprite's or a scenario agent's.
fn agent_label(sim: &Simulation, id: &str) -> String {
    sim.config()
        .sprite(id)
        .map(|s| s.label.clone())
        .or_else(|| sim.config().agents.iter().find(|a| a.id == id).map(|a| a.label.clone()))
        .unwrap_or_else(|| id.to_uppercase())
}
/// A caption that follows something in the town, made on first use.
fn town_caption(world: &mut World, state: &mut Settlement, key: &str, position: Vec3, brief: String, tint: Color, detail: String) {
    let e = *state.labels.entry(key.to_string()).or_insert_with(|| {
        world
            .spawn((
                Caption { building: true, position, detail: String::new(), units: 0, brief: String::new(), tint },
                BackgroundColor(Color::NONE),
                Text::new(""),
                TextFont { font_size: FontSize::Px(11.), ..default() },
                TextColor(tint),
                Node { position_type: PositionType::Absolute, max_width: px(310.), ..default() },
            ))
            .id()
    });
    let mut caption = world.get_mut::<Caption>(e).unwrap();
    if caption.position != position {
        caption.position = position;
    }
    if caption.brief != brief || caption.tint != tint || caption.detail != detail {
        caption.brief = brief;
        caption.tint = tint;
        caption.detail = detail;
    }
}
/// The town (`SACRED_TREE_HANDOFF.md`): each building in its kind's skin,
/// its rotor turning while it works, with a lamp over it and a card saying
/// who works it; its output stacked on its pile with the count; and each
/// sprite's flame and tag, saying its job and what it is doing (A7).
/// `player` is where the local elder stands, which unfolds the tags of the
/// sprites beside it (`TAG_UNFOLD`). Returns the glows to draw.
fn town(world: &mut World, state: &mut Settlement, sim: &Simulation, desired: &mut BTreeMap<String, View>, player: Vec3) -> Vec<(String, Vec3, f32, Glow)> {
    use crate::legacy::{BuildingStatus, Occupant, SpriteActivity};
    let mut glows = vec![];
    let ground = |world: &World, p: Vec3| crate::jetpack::height(&world.resource::<crate::scene_view::SceneView>().pack, p).unwrap_or(0.);
    let seconds = sim.time() / HZ;
    let burst = burst_indicator(world);
    for (i, b) in sim.buildings.iter().enumerate() {
        let kind = sim.config().building_kinds.iter().find(|k| k.id == b.kind).expect("a scenario building kind");
        let shape = state.building_shapes[&b.kind];
        let heading = facing(b.position, b.output_position);
        let mut v = View::new(&kind.skin, b.position);
        v.heading = heading;
        // A site shows its parts going in: a bare plot (ringed on the ground,
        // `input::hud`), then the skin at a share of its size for each part
        // installed, and whole once built.
        let grown = b.site.as_ref().map_or(1.0, |site| site.part as f32 / site.parts.max(1) as f32);
        v.scale = shape.scale * grown.max(0.0);
        // A machine skin turns its rotor a full turn per `activity`: three
        // turns a unit while it works, standing still while idle or full.
        v.activity = b.progress * 3.0;
        // A ballista's skin is the forest's turret: it turns to its last
        // aim and recoils as it shoots, as a defender's does.
        if let Some(turret) = &b.turret {
            v.aim = turret.aim.map(|p| (p[0] - b.position[0]).atan2(p[1] - b.position[1]));
            v.activity = (1. - (sim.time() - turret.fired_at as f32) / (0.25 * HZ)).max(0.);
        }
        if grown > 0.0 {
            desired.insert(format!("building/{i}"), v);
        }
        let out = Vec3::new(b.output_position[0], 0., b.output_position[1]);
        inventory(desired, sim, &format!("output/{i}"), &b.output, out, heading, &OUTPUT_LAYOUT);
        let center = Vec3::new(b.position[0], 0., b.position[1]);
        let center = center + Vec3::Y * ground(world, center);
        let (glow, status) = match (&b.occupant, b.status) {
            (Some(Occupant::Elder(_)), BuildingStatus::Full) => (Glow::Full, "FULL / POSSESSED".to_string()),
            (Some(Occupant::Elder(_)), BuildingStatus::NoAmmo) => (Glow::Full, "NO AMMO / POSSESSED".to_string()),
            (Some(Occupant::Elder(_)), _) => (Glow::Elder, "POSSESSED".to_string()),
            (Some(Occupant::Sprite(a)), BuildingStatus::Full) => (Glow::Full, format!("FULL / {}", agent_label(sim, a))),
            (Some(Occupant::Sprite(a)), BuildingStatus::NoAmmo) => (Glow::Full, format!("NO AMMO / {}", agent_label(sim, a))),
            (Some(Occupant::Sprite(a)), _) if kind.ammo.is_some() => (Glow::Working, format!("MANNED / {}", agent_label(sim, a))),
            (Some(Occupant::Sprite(a)), _) => (Glow::Working, format!("WORKING / {}", agent_label(sim, a))),
            (None, BuildingStatus::Site) => (Glow::Idle, "BEING BUILT".to_string()),
            (None, _) => match &b.claimed_by {
                Some(a) => (Glow::Idle, format!("IDLE / {} COMING", agent_label(sim, a))),
                None => (Glow::Idle, "IDLE".to_string()),
            },
        };
        // The building the local elder possesses says when it can Burst.
        let status = match burst.as_ref().filter(|(b, _)| *b == i) {
            Some((_, burst)) => format!("{status}
{burst}"),
            None => status,
        };
        // The lamp flickers while the building works, flares as a unit
        // comes out (more, and for longer, at a Burst), and pulses slowly
        // while the output waits for a pickup.
        let fading = |at: Option<&u64>, seconds: f32| at.map_or(0.0, |&at| (1.0 - (sim.time() - at as f32) / (seconds * HZ)).max(0.0));
        let flare = fading(state.produced_at.get(&i), FLARE_SECONDS);
        let burst_flare = fading(state.burst_at.get(&i), BURST_FLARE_SECONDS);
        let size = match glow {
            Glow::Working | Glow::Elder => 0.13 + 0.015 * (seconds * 11.0 + i as f32).sin(),
            Glow::Full => 0.12 + 0.03 * (seconds * 3.0).sin(),
            _ => 0.08,
        } * (1.0 + flare * 0.9 + burst_flare * 2.2);
        let lamp = center + Vec3::Y * (shape.height * grown.max(0.4) + 0.3);
        glows.push((format!("lamp/{i}"), lamp, size, glow));
        // A gatherer's product, or a ballista's ammo: what its pile holds.
        let stored = kind.product.clone().or_else(|| kind.ammo.as_ref().map(|(item, _)| item.clone())).unwrap_or_default();
        let product = sim.config().items.iter().find(|it| it.id == stored).map_or(stored.to_uppercase(), |it| it.label.clone());
        let held = b.output.total();
        let status_line = match b.status {
            BuildingStatus::Working if kind.ammo.is_some() => "Manned, shooting the nearest enemy in range".into(),
            BuildingStatus::Working => format!("Working, next unit {:.0}%", b.progress * 100.0),
            BuildingStatus::Full => "Output full: waiting for a pickup".into(),
            BuildingStatus::NoAmmo => "No ammo: bring it stone".into(),
            BuildingStatus::Idle => "Idle: nobody inside".into(),
            BuildingStatus::Site => "Being built".into(),
        };
        let detail = match &kind.ammo {
            Some(_) => format!(
                "{}\nShoots the nearest enemy in range while someone mans it, 1 {} a shot\nAmmo: {}/{} {}\nStatus: {}",
                kind.label,
                product.to_lowercase(),
                held,
                b.cap,
                product.to_lowercase(),
                status_line
            ),
            None => format!(
                "{}\nMakes 1 {} every {:.1}s while someone works it\nOutput: {}/{} {}\nStatus: {}",
                kind.label,
                product.to_lowercase(),
                kind.period,
                held,
                b.cap,
                product.to_lowercase(),
                status_line
            ),
        };
        // A built ballista's card says how much ammo it has, or that it has
        // none (B2).
        let status = match &kind.ammo {
            Some(_) if b.site.is_none() && held == 0 && b.status != BuildingStatus::NoAmmo => format!("{status}\nNO AMMO"),
            Some(_) if b.site.is_none() && held > 0 => format!("{status}\nAMMO {held}/{} {product}", b.cap),
            _ => status,
        };
        // A site's card says what its next part takes and how much is in
        // ("FOUNDATION 1/2 WOOD"); its output pile isn't there yet.
        let (status, detail) = match &b.site {
            Some(site) => {
                let part = kind.parts.get(site.part).map_or_else(|| format!("PART {}", site.part + 1), |p| p.label.clone());
                let label = |id: &str| sim.config().items.iter().find(|it| it.id == id).map_or_else(|| id.to_uppercase(), |it| it.label.clone());
                let materials: Vec<String> = site.materials.iter().map(|(item, have, need)| format!("{have}/{need} {}", label(item))).collect();
                let next = if site.ready { "READY TO INSTALL".to_string() } else { materials.join(", ") };
                let detail = format!("{}\nBeing built: part {} of {}, {}\nNeeds: {}", kind.label, site.part + 1, site.parts, part.to_lowercase(), materials.join(", ").to_lowercase());
                (format!("{status}\n{part} {next}"), detail)
            }
            None => (status, detail),
        };
        // A site's card hangs where a built one's does, clear of the
        // interact prompt over its pile.
        let card = center + Vec3::Y * (shape.height + 0.6);
        town_caption(world, state, &format!("building/{i}"), card, format!("{}\n{}", kind.label, status), glow.text(), detail);
        let tint = if b.status == BuildingStatus::Full { Glow::Full.text() } else { Color::srgb(0.98, 0.92, 0.70) };
        let out = out + Vec3::Y * (ground(world, out) + 0.45);
        if b.site.is_none() {
            town_caption(world, state, &format!("output/{i}"), out, format!("{held}/{} {product}", b.cap), tint, String::new());
        } else if let Some(e) = state.labels.remove(&format!("output/{i}")) {
            world.despawn(e);
        }
    }
    // Dropped bundles: stacked on the ground in their item's skin, as an
    // output's units are, with their count ("3 WOOD").
    let mut piles = Vec::new();
    for p in &sim.ground_piles {
        let at = Vec3::new(p.position[0], 0., p.position[1]);
        inventory(desired, sim, &format!("ground/{}", p.id), &p.stock, at, 0.0, &OUTPUT_LAYOUT);
        let Some((id, units)) = p.stock.items.first_key_value() else { continue };
        let label = sim.config().items.iter().find(|it| it.id == *id).map_or_else(|| id.to_uppercase(), |it| it.label.clone());
        let key = format!("ground/{}", p.id);
        let above = at + Vec3::Y * (ground(world, at) + 0.45);
        town_caption(world, state, &key, above, format!("{units} {label}"), Color::srgb(0.98, 0.92, 0.70), String::new());
        piles.push(key);
    }
    let gone: Vec<String> = state.labels.keys().filter(|k| k.starts_with("ground/") && !piles.contains(k)).cloned().collect();
    for k in gone {
        if let Some(e) = state.labels.remove(&k) {
            world.despawn(e);
        }
    }
    for (index, sprite) in sim.config().sprites.iter().enumerate() {
        let (Some(a), Some(s)) = (sim.agents.get(&sprite.id), sim.sprites.get(&sprite.id)) else { continue };
        let p = Vec3::new(a.position[0], 0., a.position[1]);
        let p = p + Vec3::Y * ground(world, p);
        let glow = match s.activity {
            SpriteActivity::Working => Glow::Working,
            SpriteActivity::Moving => Glow::Moving,
            SpriteActivity::Waiting => Glow::Waiting,
            SpriteActivity::Idle => Glow::Idle,
        };
        // A flame over its head: it dances while working, bobs on the walk,
        // breathes slowly while waiting and smoulders when idle.
        let phase = index as f32 * 1.7;
        let (size, lift) = match s.activity {
            SpriteActivity::Working => (0.13 + 0.03 * (seconds * 13.0 + phase).sin(), 0.04 * (seconds * 9.0 + phase).sin()),
            SpriteActivity::Moving => (0.11, 0.04 * (seconds * 6.0 + phase).sin()),
            SpriteActivity::Waiting => (0.09 + 0.03 * (seconds * 2.0 + phase).sin(), 0.0),
            SpriteActivity::Idle => (0.07, 0.0),
        };
        let head = p + Vec3::Y * 1.12;
        glows.push((format!("flame/{}", sprite.id), head + Vec3::Y * lift, size, glow));
        let (job, short) = job_labels(sim, s.job.as_ref());
        let doing = match s.activity {
            SpriteActivity::Working => "WORKING",
            SpriteActivity::Moving if s.job.is_none() => "GOING HOME",
            SpriteActivity::Moving => "ON THE WAY",
            SpriteActivity::Waiting => "WAITING",
            SpriteActivity::Idle => "IDLE",
        };
        // Declutter: a sprite working inside has no tag (its building's card
        // names it), and the others show their name alone (the flame's color
        // says what they are doing) until the elder stands beside them.
        let near = Vec2::new(p.x - player.x, p.z - player.z).length() <= TAG_UNFOLD;
        let brief = match s.activity {
            SpriteActivity::Working if s.building.is_some() => String::new(),
            _ if !near => sprite.label.clone(),
            _ if job.is_empty() => format!("{}\n{doing}", sprite.label),
            _ => format!("{}\n{short} / {doing}", sprite.label),
        };
        let detail = format!(
            "{} (sprite)\nJob: {}\nNow: {}",
            sprite.label,
            if job.is_empty() { "none" } else { job.as_str() },
            match s.activity {
                SpriteActivity::Working => "working inside",
                SpriteActivity::Moving => "walking",
                SpriteActivity::Waiting => "waiting for a free building",
                SpriteActivity::Idle => "standing at home",
            }
        );
        // Every other tag stands a little higher, so neighbors standing side
        // by side (as the sprites start) don't write over each other, while
        // staying clear of the tree's piles behind them.
        let above = 0.3 + (index % 2) as f32 * 0.4;
        town_caption(world, state, &format!("sprite/{}", sprite.id), head + Vec3::Y * above, brief, glow.text(), detail);
    }
    glows
}
/// How near the elder (in meters) a sprite's tag unfolds to its job and
/// what it is doing: about the interact reach, so the sprite it is about to
/// set a job on reads in full while the crowd stays one line each.
const TAG_UNFOLD: f32 = 2.0;
/// The town's interact (`input::town`): the prompt over the highlighted
/// target ("E  PICK UP 3 WOOD"), a refusal over the elder's head for a
/// moment, and the job menu's options in a ring around its sprite, the
/// focused one lit. The target's ring is the HUD's (`input::hud`).
/// An elder possessing a building (D3) is the building now: its lamp turns
/// violet and its card says "POSSESSED" (`town`), so its own rig, the local
/// player's or a peer's, is hidden until it leaves (`Possessing` marks the
/// ones hidden here). The camera keeps following the hidden body, which
/// stands at the building's slot spot.
fn hide_possessing(world: &mut World) {
    let (possessing, me) = {
        let Some(service) = world.get_resource::<crate::lockstep::SessionService>() else { return };
        let Some(s) = service.settlement() else { return };
        let possessing: Vec<u16> = (0..s.agents.len()).filter(|&a| s.seat(a).is_some_and(|seat| seat.building().is_some())).map(|a| a as u16).collect();
        (possessing, service.player())
    };
    let mut bodies = world.query_filtered::<(Entity, &mut Visibility, Option<&crate::peers::Peer>, Has<Possessing>), Or<(With<crate::character::Player>, With<crate::peers::Peer>)>>();
    let mut changes = Vec::new();
    for (entity, mut visibility, peer, marked) in bodies.iter_mut(world) {
        let agent = peer.map_or(me, |p| Some(p.agent()));
        let hidden = agent.is_some_and(|a| possessing.contains(&a));
        if hidden && !marked {
            *visibility = Visibility::Hidden;
            changes.push((entity, true));
        } else if !hidden && marked {
            *visibility = Visibility::Inherited;
            changes.push((entity, false));
        }
    }
    for (entity, hide) in changes {
        if hide {
            world.entity_mut(entity).insert(Possessing);
        } else {
            world.entity_mut(entity).remove::<Possessing>();
        }
    }
}

/// While the local elder possesses a building: the building's index and
/// whether it can Burst now, by the sim's own check
/// (`Settlement::burst_check`), with the key ("BURST READY (R)"), or the
/// seconds left ("BURST 12 s"). For its card and the HUD line.
fn burst_indicator(world: &World) -> Option<(usize, String)> {
    let service = world.get_resource::<crate::lockstep::SessionService>()?;
    let (s, sim_world, agent) = (service.settlement()?, service.world()?, service.player()? as usize);
    let building = s.seat(agent)?.building()?;
    let key = match world.get_resource::<crate::input::InputService>().map(|i| i.frame().owner) {
        Some(crate::input::Device::Gamepad) => "RT",
        _ => "R",
    };
    let text = match s.burst_check(agent, sim_world.tick()) {
        Ok(_) => format!("BURST READY ({key})"),
        Err(sim::settlement::BurstRefusal::Cooling { ticks }) => format!("BURST {} s", ticks.div_ceil(content::TICKS_PER_SECOND as u64)),
        Err(sim::settlement::BurstRefusal::OutputFull) => "BURST: OUTPUT FULL".to_string(),
        Err(sim::settlement::BurstRefusal::NotPossessing | sim::settlement::BurstRefusal::NoBurst) => return None,
    };
    Some((building as usize, text))
}

/// The controls help line in a town: the town's keys, or while the elder
/// possesses a building, the keys that work there (leaving and Burst, or
/// in a ballista firing).
/// `style::apply` writes the first when the world starts.
fn town_help(world: &mut World) {
    let help = if crate::input::town::possessing_ballista(world) {
        crate::style::TOWN_BALLISTA_HELP
    } else if crate::input::town::possessing(world) {
        crate::style::TOWN_POSSESSED_HELP
    } else {
        crate::style::TOWN_HELP
    };
    for mut text in world.query_filtered::<&mut Text, With<crate::scene::ControlHelp>>().iter_mut(world) {
        if text.0 != help {
            text.0 = help.into();
        }
    }
}

/// A player's body hidden while it possesses a building (`hide_possessing`).
#[derive(Component)]
struct Possessing;

fn town_interact(world: &mut World, state: &mut Settlement, sim: &Simulation, player: Vec3) {
    use crate::input::{gameplay::Dispatcher, InputService, Panel};
    let ground = |world: &World, p: Vec3| crate::jetpack::height(&world.resource::<crate::scene_view::SceneView>().pack, p).unwrap_or(0.);
    let mut shown: Vec<String> = Vec::new();
    let town = world.get_resource::<Dispatcher>().map(|d| d.town.clone()).unwrap_or_default();
    let key = match world.get_resource::<InputService>().map(|i| i.frame().owner) {
        Some(crate::input::Device::Gamepad) => "A",
        _ => "E",
    };
    let prompt = Color::srgb(0.55, 1.0, 0.88);
    let menu = match world.get_resource::<InputService>().and_then(|i| i.panel()) {
        Some(Panel::Radial { options, focus, .. }) => Some((options.clone(), *focus)),
        _ => None,
    };
    if let Some(h) = town.highlight.as_ref().filter(|_| menu.is_none()) {
        // Above the pile's own count or label, which hang lower.
        let at = h.position + Vec3::Y * (ground(world, h.position) + 1.3);
        let text = format!("{key}  {}", h.prompt);
        town_caption(world, state, "interact/prompt", at, text.clone(), prompt, text);
        shown.push("interact/prompt".into());
    }
    if let (Some((options, focus)), Some(menu)) = (menu, town.menu.as_ref()) {
        // Around the sprite, in screen directions (`radial_angle`).
        let sprite = world.resource::<crate::lockstep::SessionService>().settlement().and_then(|s| s.agents.get(menu.sprite as usize)).map(|a| a.body.position);
        if let Some(sprite) = sprite {
            let center = world.resource::<crate::lockstep::SessionService>().to_view(sprite).with_y(0.0);
            let center = center + Vec3::Y * (ground(world, center) + 0.6);
            for (i, option) in options.iter().enumerate() {
                let angle = crate::input::radial_angle(i, options.len());
                let at = center + crate::input::gameplay::camera_relative(Vec2::new(angle.cos(), angle.sin())) * 1.1;
                let (text, tint) = if i == focus { (format!("[ {option} ]"), prompt) } else { (option.clone(), Color::srgba(0.85, 0.85, 0.7, 0.75)) };
                let key = format!("interact/menu/{i}");
                town_caption(world, state, &key, at, text.clone(), tint, text);
                shown.push(key);
            }
        }
    }
    if let Some((reason, at)) = state.refusal.clone() {
        if sim.time() - at < REFUSAL_SECONDS * HZ {
            let head = player + Vec3::Y * 1.9;
            town_caption(world, state, "interact/refusal", head, reason.clone(), Color::srgb(1.0, 0.6, 0.35), reason);
            shown.push("interact/refusal".into());
        } else {
            state.refusal = None;
        }
    }
    // Drop the ones not shown this frame.
    let stale: Vec<String> = state.labels.keys().filter(|k| k.starts_with("interact/") && !shown.contains(k)).cloned().collect();
    for k in stale {
        if let Some(e) = state.labels.remove(&k) {
            world.despawn(e);
        }
    }
}
/// A blueprint's ghost (`input::town`): the kind's skin where it would
/// stand, see-through and tinted green while it may go down there and red
/// while it may not, with its name and the sim's reason over it. Its rings
/// on the ground are the HUD's (`input::hud`).
fn town_ghost(world: &mut World, state: &mut Settlement, sim: &Simulation) {
    let ghost = world.get_resource::<crate::input::gameplay::Dispatcher>().and_then(|d| d.town.ghost.clone());
    let kind = ghost.as_ref().and_then(|g| world.resource::<crate::lockstep::SessionService>().settlement()?.building_kinds.get(g.kind as usize).map(|k| k.id.clone()));
    let kind = kind.and_then(|id| sim.config().building_kinds.iter().find(|k| k.id == id).cloned());
    let (Some(ghost), Some(kind)) = (ghost, kind) else {
        if let Some((_, rig, fade)) = state.ghost.take() {
            fade.restore(world);
            world.despawn(rig.root);
        }
        if let Some(e) = state.labels.remove("ghost/caption") {
            world.despawn(e);
        }
        return;
    };
    if state.ghost.as_ref().is_some_and(|(skin, _, _)| *skin != kind.skin) {
        let (_, rig, fade) = state.ghost.take().unwrap();
        fade.restore(world);
        world.despawn(rig.root);
    }
    if state.ghost.is_none() {
        let rig = state.templates[&kind.skin].spawn(world);
        let fade = Fade::new(world, rig.root);
        state.ghost = Some((kind.skin.clone(), rig, fade));
    }
    let (valid, invalid) = (Color::srgb(0.35, 1.0, 0.55), Color::srgb(1.0, 0.3, 0.25));
    let tint = if ghost.verdict.is_ok() { valid } else { invalid };
    let shape = state.building_shapes[&kind.id];
    let template_scale = state.templates[&kind.skin].scale;
    let at = ghost.center + Vec3::Y * crate::jetpack::height(&world.resource::<crate::scene_view::SceneView>().pack, ghost.center).unwrap_or(0.);
    let heading = facing([ghost.center.x, ghost.center.z], [ghost.output.x, ghost.output.z]);
    let (_, rig, fade) = state.ghost.as_mut().unwrap();
    fade.tint(world, tint, 0.45);
    rig.pose(world, at, heading, 0.0, template_scale * shape.scale, 0.0, 0.0, 0.0, 0.0, crate::rig::Strike::default(), None, None);
    let text = match ghost.verdict {
        Ok(()) => format!("{}\nE  PLACE", kind.label),
        Err(reason) => format!("{}\n{}", kind.label, reason.message()),
    };
    let key = match world.get_resource::<crate::input::InputService>().map(|i| i.frame().owner) {
        Some(crate::input::Device::Gamepad) => text.replace("E  PLACE", "A  PLACE"),
        _ => text,
    };
    town_caption(world, state, "ghost/caption", at + Vec3::Y * (shape.height + 0.6), key.clone(), tint, key);
}
/// Fades the tree's canopy while it hides the elder, a site or the ghost
/// from the camera (`canopy`). View only.
fn town_canopy(world: &mut World, state: &mut Settlement, sim: &Simulation, player: Vec3) {
    if state.canopy.is_none() {
        state.canopy = crate::canopy::Canopy::split(world);
    }
    let ghost = world.get_resource::<crate::input::gameplay::Dispatcher>().and_then(|d| d.town.ghost.as_ref().map(|g| g.center));
    let sites = sim.buildings.iter().filter(|b| b.site.is_some()).map(|b| Vec3::new(b.position[0], 0.0, b.position[1]));
    // Each at about half its height, where the eye looks for it.
    let targets: Vec<Vec3> = std::iter::once(player).chain(ghost).chain(sites).map(|p| p + Vec3::Y * 0.8).collect();
    let dt = world.resource::<Time>().delta_secs();
    if let Some(canopy) = state.canopy.as_mut() {
        canopy.fade(world, &targets, dt);
    }
}
/// Draws `glows` (key, position, radius, state), making and dropping their
/// entities as they come and go.
fn sync_glows(world: &mut World, state: &mut Settlement, glows: Vec<(String, Vec3, f32, Glow)>) {
    let Some(assets) = &state.glow_assets else { return };
    let keep: std::collections::BTreeSet<&str> = glows.iter().map(|g| g.0.as_str()).collect();
    let gone: Vec<String> = state.glows.keys().filter(|k| !keep.contains(k.as_str())).cloned().collect();
    for key in gone {
        world.despawn(state.glows.remove(&key).unwrap());
    }
    for (key, position, size, glow) in glows {
        let transform = Transform::from_translation(position).with_scale(Vec3::splat(size));
        let material = assets.materials[glow as usize].clone();
        match state.glows.get(&key) {
            Some(&e) => {
                if *world.get::<Transform>(e).unwrap() != transform {
                    *world.get_mut::<Transform>(e).unwrap() = transform;
                }
                if world.get::<MeshMaterial3d<StandardMaterial>>(e).unwrap().0 != material {
                    world.entity_mut(e).insert(MeshMaterial3d(material));
                }
            }
            None => {
                let e = world
                    .spawn((Mesh3d(assets.mesh.clone()), MeshMaterial3d(material), transform, bevy::light::NotShadowCaster))
                    .id();
                state.glows.insert(key, e);
            }
        }
    }
}
pub fn snapshot(world: &World) -> serde_json::Value {
    let fingerprint = world.get_resource::<crate::lockstep::WorldMirror>().map_or_else(String::new, |m| m.fingerprint());
    world.get_resource::<Settlement>().and_then(|state| state.simulation.as_ref()).map_or(serde_json::Value::Null, |s| {
        let mut out = serde_json::json!({"simulation": s, "replay_fingerprint": fingerprint, "balances": s.balances(), "scenario": s.config(), "seconds": s.seconds()});
        // The town's interact target and job menu (`input::town`).
        if !s.config().building_kinds.is_empty() {
            out["interact"] = crate::input::town::snapshot(world);
            // The tree's canopy: 1 whole, less while it is see-through.
            let canopy = world.get_resource::<Settlement>().and_then(|state| state.canopy.as_ref()).map_or(1.0, crate::canopy::Canopy::opacity);
            out["canopy_opacity"] = canopy.into();
            // The local elder's Burst: the seconds its cooldown has left
            // (0 when over), and whether one would go off now, with the
            // units it adds or the sim's reason why not.
            let service = world.get_resource::<crate::lockstep::SessionService>();
            let burst = service.and_then(|service| {
                let (s, sim_world, agent) = (service.settlement()?, service.world()?, service.player()? as usize);
                let ready = s.agents.get(agent)?.avatar()?.burst_ready;
                let cooldown = ready.saturating_sub(sim_world.tick()) as f64 / content::TICKS_PER_SECOND as f64;
                Some(match s.burst_check(agent, sim_world.tick()) {
                    Ok(units) => serde_json::json!({"cooldown": cooldown, "ready": true, "units": units, "reason": null}),
                    Err(reason) => serde_json::json!({"cooldown": cooldown, "ready": false, "units": null, "reason": reason.message()}),
                })
            });
            out["burst"] = burst.unwrap_or(serde_json::Value::Null);
        }
        // The local player's seat: a vehicle it drives or a building it
        // possesses, by index, or null.
        let service = world.get_resource::<crate::lockstep::SessionService>();
        let seat = service.and_then(|service| service.settlement()?.seat(service.player()? as usize));
        out["seat"] = match seat {
            Some(sim::settlement::Seat::Vehicle(v)) => serde_json::json!({"vehicle": v}),
            Some(sim::settlement::Seat::Building(b)) => serde_json::json!({"building": b}),
            None => serde_json::Value::Null,
        };
        out
    })
}
/// Pin the same hover card for deterministic capture or remote inspection.
/// Passing None restores mouse-driven hover.
pub fn inspect_building(world: &mut World, id: Option<&str>) -> Result<(), String> {
    let mut state = world.resource_mut::<Settlement>();
    let key = if let Some(id) = id {
        [format!("producer/{id}"), format!("defender/{id}"), format!("building/{id}")]
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
        &ComputedNode,
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
                    .filter_map(|(e, p, _, _, _, _, _, _, _)| {
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
    for (e, p, mut node, mut visibility, mut text, mut font, mut color, mut background, computed) in
        &mut labels
    {
        let Ok(v) = camera.0.world_to_viewport(camera.1, p.position) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let expanded = hovered == Some(e);
        let folded = !p.building || !p.brief.is_empty();
        *visibility = if expanded || (hovered.is_none() && folded) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let content = if expanded {
            if p.building {
                p.detail.clone()
            } else {
                format!("{}\nLMB take / Ctrl+LMB deposit", p.detail)
            }
        } else if !p.brief.is_empty() {
            p.brief.clone()
        } else {
            p.units.to_string()
        };
        if text.0 != content {
            text.0 = content;
        }
        font.font_size = FontSize::Px(if expanded { 14. } else if !p.brief.is_empty() { 12. } else { 10. });
        color.0 = if expanded {
            Color::srgb(0.95, 0.89, 0.72)
        } else if !p.brief.is_empty() {
            p.tint
        } else {
            Color::srgba(0.85, 0.85, 0.65, 0.65)
        };
        // A brief gets a faint backing, so it reads over grass and foliage.
        background.0 = if expanded {
            Color::srgba(0.025, 0.045, 0.03, 0.94)
        } else if !p.brief.is_empty() {
            Color::srgba(0.025, 0.045, 0.03, 0.55)
        } else {
            Color::NONE
        };
        node.padding = UiRect::all(px(if expanded { 8. } else if !p.brief.is_empty() { 2. } else { 0. }));
        let size = windows
            .iter()
            .next()
            .map_or(Vec2::new(1440., 960.), |w| Vec2::new(w.width(), w.height()));
        // A folded brief stands centered on top of its point, so it doesn't
        // cover what it labels; anything else hangs from it.
        let (left, top) = if !expanded && !p.brief.is_empty() {
            let text = computed.size() * computed.inverse_scale_factor();
            (v.x - text.x * 0.5, v.y - text.y)
        } else {
            (v.x - if expanded { 45. } else { 3. }, v.y)
        };
        node.left = px(left.clamp(8., (size.x - if expanded { 330. } else { 20. }).max(8.)));
        node.top = px(top.clamp(8., (size.y - if expanded { 300. } else { 20. }).max(8.)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx::{Fx, FxVec2};

    /// An arc coil shot through three ghouls: the shot arcs to the first,
    /// and the chain on from it to the second and third. A shot prediction
    /// already drew isn't drawn again, but its hits still chain from it, not
    /// from the player.
    /// A predicted arrow flies at half pace from the bow until its confirmed
    /// projectile, an input lead behind, catches up; the projectile that
    /// takes it over is drawn no further back than the arrow was, then as
    /// confirmed. Withdrawn, it is gone; never confirmed, it is dropped once
    /// it has come down.
    #[test]
    fn a_predicted_arrow_is_handed_over_to_its_projectile() {
        let arrow = |id: u64| PredictedArrow { id, fired: 100, from: [0.0, 0.0], to: [0.0, 9.0], height: 0.6, flight: 20, projectile: None };
        let mut arrows = PredictedArrows(vec![arrow(0), arrow(1), arrow(2)]);
        assert_eq!(arrows.0[0].age(99.0), 0.0, "not before it is fired");
        assert_eq!(arrows.0[0].age(108.0), 4.0);
        // Confirmed 8 ticks behind the drawn time: the projectile takes the
        // arrow over where it was drawn.
        assert!(arrows.take_over(0, 7));
        assert!(!arrows.take_over(0, 8), "taken over once");
        assert_eq!(arrows.projectile_age(7, 0.0, 108.0), 4.0);
        assert_eq!(arrows.projectile_age(7, 2.0, 110.0), 5.0);
        // It meets the projectile after twice the lead, then is drawn with it.
        assert_eq!(arrows.projectile_age(7, 8.0, 116.0), 8.0);
        assert_eq!(arrows.projectile_age(7, 10.0, 118.0), 10.0);
        // Other projectiles are drawn as confirmed.
        assert_eq!(arrows.projectile_age(9, 3.0, 118.0), 3.0);
        // A shot the confirmed world never fires is no longer drawn.
        arrows.withdraw(1);
        assert_eq!(arrows.0.iter().map(|a| a.id).collect::<Vec<_>>(), [0, 2]);
        // The taken-over arrow goes with its projectile; an unconfirmed one
        // once it has come down.
        arrows.retain_flying(139.0, |_| true);
        assert_eq!(arrows.0.len(), 2);
        arrows.retain_flying(140.0, |_| true);
        assert_eq!(arrows.0.iter().map(|a| (a.id, a.projectile)).collect::<Vec<_>>(), [(0, Some(7))]);
        arrows.retain_flying(140.0, |_| false);
        assert!(arrows.0.is_empty());
    }

    #[test]
    fn an_arc_shot_drawn_early_still_starts_its_chain() {
        let player = Vec3::new(0.0, 0.75, 0.0);
        let (from, first, second, third) = (Vec3::new(0.0, ARC_HEIGHT, 0.0), Vec3::X, Vec3::X * 2.0, Vec3::new(2.0, 0.0, 1.0));
        for presented in [false, true] {
            let mut arcs = ArcChain::new(player);
            let shot = arcs.fired(from, first, presented);
            assert_eq!(shot, (!presented).then_some((from, first)));
            assert_eq!(arcs.hit(first), None, "the shot's arc reached the first ghoul");
            assert_eq!(arcs.hit(second), Some((first, second)));
            assert_eq!(arcs.hit(third), Some((second, third)));
        }
        // Arc hits without a shot this frame chain from the player.
        let mut arcs = ArcChain::new(player);
        assert_eq!(arcs.hit(first), Some((player, first)));
        // A second shot in the same frame starts a new chain.
        assert_eq!(arcs.fired(from, second, true), None);
        assert_eq!(arcs.hit(second), None);
        assert_eq!(arcs.hit(third), Some((second, third)));
    }

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
        let world = sim::World::new_settlement(1, &content::Content::builtin(), &scenario, &obstacles).unwrap();
        let center = world.terrain().center;
        let mut game = sim::Simulation::from_world(world, 2);
        let config = crate::legacy::Scenario::new(&scenario);
        let hz = content::TICKS_PER_SECOND as u64;
        for tick in 0..100 * hz {
            game.step(&[]);
            let s = game.world().settlement().unwrap();
            s.validate().unwrap();
            if tick == 60 * hz - 1 {
                assert_eq!(s.health, 20, "first minute defense failed");
            }
            let a = s.agent("bramble");
            if let Some(t) = &a.hauler().unwrap().trailer {
                assert!(sim::nav::distance(a.body.position, t.position) >= Fx::from_ratio(29, 20), "ox overlapped the cart at tick {tick}");
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

    /// The forest outpost before its first tick, with one ghoul of its first
    /// wave beside the first agent.
    fn forest_with_a_ghoul() -> (crate::legacy::Scenario, sim::settlement::Settlement, FxVec2) {
        let library = SceneLibrary::new();
        let scenario = library.scenes[6].scene.scenario.clone().unwrap();
        let world = sim::World::new_settlement(1, &content::Content::builtin(), &scenario, &[]).unwrap();
        let center = world.terrain().center;
        let mut s = world.settlement().unwrap().clone();
        let position = s.agents[0].body.position + FxVec2::new(Fx::from_ratio(1, 2), Fx::ZERO);
        let character = s.waves[0].character;
        s.threats.push(sim::settlement::Threat {
            id: 7,
            wave: 0,
            character,
            position,
            heading: Fx::ZERO,
            travelled: Fx::ZERO,
            hp: 3,
            armor: 0,
            pierced: false,
            shocked_until: 0,
            route: vec![position],
            stagger_until: 0,
            knockback: FxVec2::ZERO,
            windup: None,
            ready_at: 0,
        });
        (crate::legacy::Scenario::new(&scenario), s, center)
    }

    #[test]
    fn the_view_carries_a_threats_windup_until_its_drawn_tick_passes_it() {
        let (config, idle, center) = forest_with_a_ghoul();
        let attack = idle.characters[idle.waves[0].character as usize].attack.expect("forest ghouls attack");
        let agent = idle.agents[0].id.clone();
        let with = |windup: Option<u64>, ready_at: u64| {
            let mut s = idle.clone();
            s.threats[0].windup = windup.map(|lands| sim::settlement::combat::Windup { agent: 0, lands });
            s.threats[0].ready_at = ready_at;
            s
        };
        let view = |previous: &sim::settlement::Settlement, current: &sim::settlement::Settlement| {
            Simulation::new(&config, Some(previous), current, 100, 0.5, center, &[]).threats[&7].clone()
        };
        assert_eq!(Simulation::new(&config, None, &idle, 100, 0.5, center, &[]).drawn_time(), 99.5);
        let fresh = view(&idle, &idle);
        assert_eq!((fresh.windup, fresh.swung), (None, None), "no windup, no swing yet");
        let winding = view(&with(Some(110), 0), &with(Some(110), 0));
        let expected = crate::legacy::Windup { agent: agent.clone(), lands: 110, ticks: attack.windup };
        assert_eq!(winding.windup, Some(expected));
        // Drawn between ticks 99 and 100, a swing that landed on 100 still
        // shows its windup finishing, and the landed tick is known.
        let landed = view(&with(Some(100), 0), &with(None, 101 + attack.cooldown));
        assert_eq!(landed.windup.map(|w| w.lands), Some(100));
        assert_eq!(landed.swung, Some(100));
        // A stagger cancelled this one before it landed: it just goes away.
        let cancelled = view(&with(Some(105), 0), &with(None, 0));
        assert_eq!((cancelled.windup, cancelled.swung), (None, None));
    }

    #[test]
    fn a_threats_swing_event_names_the_player_and_where_the_threat_stands() {
        let (config, s, center) = forest_with_a_ghoul();
        let events = [
            sim::settlement::Event::ThreatWindup { threat: 7, agent: 0, lands: 127 },
            sim::settlement::Event::ThreatSwing { threat: 7, agent: 0, hit: true, damage: 1, killed: false },
        ];
        let view = Simulation::new(&config, None, &s, 100, 0.0, center, &events);
        let [crate::legacy::Event::ThreatSwing { threat, agent, from, hit, killed }] = view.events.as_slice() else {
            panic!("expected one swing, got {:?}", view.events);
        };
        assert_eq!((*threat, agent.as_str(), *hit, *killed), (7, s.agents[0].id.as_str(), true, false));
        assert_eq!(*from, Some(view.threats[&7].position));
    }

    fn drawn_threat(windup: Option<(u64, u64)>, swung: Option<u64>) -> crate::legacy::Threat {
        crate::legacy::Threat {
            id: 7,
            wave: 0,
            position: [0.0, 0.0],
            heading: 0.0,
            travelled: 0.0,
            hp: 3,
            armor: 0,
            pierced: false,
            shocked_until: 0,
            stagger_until: 0,
            knockback: [0.0, 0.0],
            windup: windup.map(|(lands, ticks)| crate::legacy::Windup { agent: "player".into(), lands, ticks }),
            swung,
        }
    }

    #[test]
    fn a_threats_windup_builds_with_its_drawn_tick_then_its_swing_follows_through() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let winding = drawn_threat(Some((127, 27)), None);
        for (drawn, expected) in [(90.0, 0.0), (100.0, 0.0), (113.5, 0.5), (126.0, 26.0 / 27.0)] {
            let strike = threat_strike(&winding, drawn);
            assert!(close(strike.windup, expected) && strike.swing == 0.0, "at {drawn}: {strike:?}");
        }
        // The landing frame: the windup (carried from the previous tick)
        // finishes, and the swing starts once the drawn tick reaches it.
        let landing = drawn_threat(Some((127, 27)), Some(127));
        assert!(close(threat_strike(&landing, 126.5).windup, 26.5 / 27.0));
        assert_eq!(threat_strike(&landing, 126.5).swing, 0.0);
        assert_eq!(threat_strike(&landing, 127.0), crate::rig::Strike { windup: 0.0, swing: 1.0 });
        let swung = drawn_threat(None, Some(127));
        let half = threat_strike(&swung, 127.0 + SWING_TICKS / 2.0);
        assert!(close(half.swing, 0.75) && half.windup == 0.0, "{half:?}");
        assert_eq!(threat_strike(&swung, 127.0 + SWING_TICKS), crate::rig::Strike::default());
        assert_eq!(threat_strike(&swung, 400.0), crate::rig::Strike::default());
    }
}

/// Reserve an exact transfer and walk to the dock. Returns the cargo the
/// player will carry once it completes.
pub fn player_transfer(world: &mut World, depot: &str, item: &str, units: u32, deposit: bool) -> Result<u32, String> {
    let state = world.resource::<Settlement>();
    let sim = state.simulation.as_ref().ok_or("No settlement loaded")?;
    let actor = world.resource::<crate::lockstep::WorldMirror>().player_id().ok_or("No controlled actor")?;
    let dock = sim.config().depots.iter().find(|d| d.id == depot).ok_or("Unknown depot")?.position;
    let before = sim.agents[&actor].cargo.total();
    let service = world.resource::<crate::lockstep::SessionService>();
    let current = service.settlement().ok_or("No settlement loaded")?;
    let (Some(agent), Some(d), Some(i)) = (service.player(), current.depot_index(depot), current.item_index(item)) else {
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
    world.resource_scope(|world, mut control: Mut<crate::lockstep::PlayerControl>| {
        let mut service = world.resource_mut::<crate::lockstep::SessionService>();
        service.command(sim::CommandKind::Transfer { agent, depot: d, item: i, units, deposit });
        control.go_to(&mut service, Vec3::new(dock[0], 0., dock[1]), false);
    });
    world.resource_mut::<Settlement>().notice = format!(
        "{} {units} {item} {} {depot}",
        if deposit { "Delivering" } else { "Collecting" },
        if deposit { "to" } else { "from" }
    );
    Ok(expected)
}
/// Starts the player's bundle action on `other` (`Action::Bundle`): walks
/// there and sends it, sized by the sim's own rule (`bundle_job`). A refusal
/// fails with its words (`BundleRefusal::message`). Returns the depot items
/// the cargo will hold once the units land, as `player_transfer` does.
pub fn player_bundle(world: &mut World, other: sim::settlement::InventoryRef, direction: sim::settlement::Direction) -> Result<u32, String> {
    let service = world.resource::<crate::lockstep::SessionService>();
    let (current, sim_world) = (service.settlement().ok_or("No settlement loaded")?, service.world().ok_or("No settlement loaded")?);
    let agent = service.player().ok_or("No controlled actor")?;
    let job = current.bundle_job(agent, other, direction, sim_world.tick(), sim_world.terrain()).map_err(|r| r.message().to_string())?;
    let before: u32 = current.depot_cargo(&current.agents[agent as usize]).iter().sum();
    // Stand at a depot's dock, or just in front of an output pile, away from
    // its building (an output pile is that close to the footprint).
    let target = match other {
        sim::settlement::InventoryRef::Output(b) => {
            let b = &current.buildings[b as usize];
            b.output_position + (b.output_position - b.position).normalize_or_zero() * fx::Fx::from_ratio(2, 5)
        }
        sim::settlement::InventoryRef::Depot(d) => current.depots[d as usize].position,
        _ => return Err("Not a depot or an output".into()),
    };
    let target = service.to_view(target);
    let expected = match direction {
        sim::settlement::Direction::Pickup => before + job.remaining,
        sim::settlement::Direction::Deposit => before - job.remaining,
    };
    world.resource_scope(|world, mut control: Mut<crate::lockstep::PlayerControl>| {
        let mut service = world.resource_mut::<crate::lockstep::SessionService>();
        service.command(sim::CommandKind::Bundle { agent, other, direction });
        control.go_to(&mut service, target, false);
    });
    Ok(expected)
}
/// Clicking a depot collects from it (Ctrl: deposits). Keyboard E and
/// cancelling a transfer by moving are shared actions
/// (`input::gameplay`); this is the mouse's targeting only.
pub(crate) fn depot_at<'a>(sim: &'a Simulation, p: Vec3) -> Option<&'a crate::legacy::Depot> {
    sim.config().depots.iter()
        .filter(|d| Vec2::new(d.position[0] - p.x, d.position[1] - p.z).length() < 0.9)
        .min_by(|a, b| {
            Vec2::new(a.position[0] - p.x, a.position[1] - p.z)
                .length_squared()
                .total_cmp(&Vec2::new(b.position[0] - p.x, b.position[1] - p.z).length_squared())
        })
}

pub fn input(world: &mut World) {
    if world.resource::<Settlement>().simulation.is_none() {
        return;
    }
    if !crate::input::gameplay::pointer_allowed(world.resource::<crate::input::InputService>()) {
        return;
    }
    let input = world.resource::<debug_ui::GameInput>();
    let keys = input.keys();
    let deposit = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !input.mouse().just_pressed(MouseButton::Left) {
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
    let Some(actor) = world.resource::<crate::lockstep::WorldMirror>().player_id() else {
        return;
    };
    let Some(depot) = depot_at(sim, p) else {
        return;
    };
    let stock = if deposit {
        &sim.agents[&actor].cargo
    } else {
        &sim.depots[&depot.id]
    };
    let game = world.resource::<crate::lockstep::SessionService>();
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
        let library = SceneLibrary::new();
        let mut app = crate::test_app(options, library, hub, crate::flow::Launch::single_player(6));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 30.),
        ));
        crate::start_world(&mut app);
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
        crate::switch_world(&mut app, 0);
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

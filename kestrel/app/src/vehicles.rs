//! Vehicles: every vehicle the settlement parks is drawn as a rover, a
//! chassis on four wheels. The simulation moves it (`sim::settlement::vehicle`);
//! this only draws it.
//!
//! The local player's own vehicle is drawn under them, from the player's
//! drawn transform: that already holds the prediction, its smoothing, and
//! the newest confirmed pose in single player, so driver and rover never
//! drift apart. Other vehicles are interpolated between the last two ticks.
//! Each wheel sits on the ground under its drawn position, eased so a
//! terrace step lifts it over a few frames, and the chassis tilts to them.
//!
//! A crawler (a vehicle with a `crawler` section) is drawn from its rig
//! (`crawler::Rig`, `actors/crawler.ron`): its body level at the height the
//! simulation gives it, and each leg solved from its hip to its foot
//! (`crawler::solve`). Feet stand where the simulation put them and swing
//! along arcs between its spots (`crawler::swing_point`), interpolated
//! between the last two ticks as the body is. While the local player pilots
//! one, the camera pulls back (`Vehicles::pull`).
//!
//! A crawler with a gun (TU.5 of the turret) carries the rig's swivel
//! ballista at the gun's mount on the cab's roof, turning (eased) to its aim
//! ([`gun_aim`]): the local gunner's reticle while they aim, another elder
//! gunner's confirmed reticle, else the last shot's aim (a sprite's or an
//! elder's), else where it last pointed (forward to begin with).

use bevy::prelude::*;
use fx::Fx;
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};
use sim::settlement::vehicle::wheel_points;
use sim::settlement::VehicleState;

use crate::character::Player;
use crate::lockstep::{SessionService, WorldMirror};

const WHEEL_RADIUS: f32 = 0.34;
const WHEEL_WIDTH: f32 = 0.26;
/// The chassis's middle above the wheels' contact.
const CHASSIS_LIFT: f32 = 0.46;
/// How quickly a wheel eases onto new ground, per second.
const WHEEL_EASE: f32 = 22.0;
/// Higher than the wheels' ground by this, the rover is in the air.
const AIRBORNE: f32 = 0.05;
/// How much farther the camera stands while the local player pilots a
/// crawler: its view's height grows by this.
pub const PILOT_PULL: f32 = 1.4;
/// How quickly the camera pulls back or eases in, per second.
const PULL_EASE: f32 = 1.6;
/// A drawn foot whose spot moves faster than this (m/s) jumped: the
/// simulation aimed its swing again. It glides there instead, the gap
/// halving every `FOOT_GLIDE` seconds.
const FOOT_JUMP: f32 = 30.0;
const FOOT_GLIDE: f32 = 0.06;

/// The rovers drawn in this world.
#[derive(Resource)]
pub struct Vehicles {
    parts: Option<Parts>,
    /// How far the camera is pulled back: 1 on foot, easing toward
    /// `PILOT_PULL` while the local player pilots a crawler.
    pull: f32,
}

impl Default for Vehicles {
    fn default() -> Self {
        Vehicles { parts: None, pull: 1.0 }
    }
}

impl Vehicles {
    /// The camera's pull (`effects::camera_feedback` scales its view by it).
    pub fn pull(&self) -> f32 {
        self.pull
    }
}

struct Parts {
    chassis: Handle<Mesh>,
    hood: Handle<Mesh>,
    bar: Handle<Mesh>,
    wheel: Handle<Mesh>,
    paint: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
    tire: Handle<StandardMaterial>,
    /// A unit cube and a unit cylinder, scaled to a crawler's pieces, and
    /// its finishes (`crawler::Finish::ALL` order).
    block: Handle<Mesh>,
    drum: Handle<Mesh>,
    finishes: Vec<Handle<StandardMaterial>>,
}

/// A crawler's drawn legs: each leg's hip, thigh, shin and foot, and where
/// each sole was drawn last frame.
struct DrawnLegs {
    def: content::character::CrawlerDef,
    segments: [[Entity; 4]; 4],
    /// The gun's pieces, on a crawler with a gun, and its drawn turn from
    /// the body's forward (`crawler::gun_yaw`), eased toward its aim.
    gun: Option<(Entity, f32)>,
    /// Each foot's spot last frame, and how far from it the foot is drawn
    /// while it glides over a jump.
    last: Option<([Vec3; 4], [Vec3; 4])>,
}

/// A drawn rover: its root turns with the heading, the chassis tilts.
#[derive(Component)]
pub struct Rover {
    index: usize,
    chassis: Entity,
    wheels: [Entity; 4],
    /// A crawler's legs: drawn from its rig.
    crawler: Option<DrawnLegs>,
    /// Each wheel's drawn contact height, eased toward the ground.
    contact: Option<[f32; 4]>,
    /// How far each wheel has turned, in radians.
    spin: f32,
    last: Option<Vec3>,
}

#[cfg(test)]
impl Rover {
    /// A crawler's gun's drawn turn from the body's forward, radians toward
    /// its right, if it has a gun.
    pub fn gun_yaw(&self) -> Option<f32> {
        self.crawler.as_ref()?.gun.map(|(_, yaw)| yaw)
    }

    /// Which vehicle it draws.
    pub fn index(&self) -> usize {
        self.index
    }
}

impl Service for Vehicles {
    type Layer = WorldLayer;
    type Deps = (WorldMirror, crate::character::Avatar);

    fn init(_world: &mut World) -> Self {
        Vehicles::default()
    }

    fn systems(app: &mut App) {
        app.init_resource::<crate::prediction::DrawnAvatar>()
            .add_service_systems::<Self, _>(Phase::Present, (draw, footfalls).run_if(in_state(WorldState::Active)));
    }
}

/// How many more bursts ring a foot that stomped a threat, or shoved a
/// body, and how far out from its middle (the pad is 0.7 m across).
const STOMP_DUST: usize = 6;
const SHOVE_DUST: usize = 3;
const DUST_RING: f32 = 0.45;

/// A crawler's foot coming down (CR.5, `Event::Footfall`, the tick its
/// swing is planted): a puff of dust under it and a step's thud; a stomp
/// on a ghoul throws a ring of dust round it and lands heavily, a shove a
/// smaller ring.
fn footfalls(mut commands: Commands, mirror: Res<WorldMirror>, assets: Option<Res<crate::effects::FxAssets>>, sound: Option<Res<crate::sound::Sound>>) {
    let Some(assets) = assets else { return };
    for event in &mirror.events {
        let sim::settlement::Event::Footfall { vehicle, leg, position, floor, shoved, stomped } = *event else { continue };
        let at = mirror.to_view(position).with_y(floor.to_f32() + 0.05);
        let seed = (mirror.tick as u32).wrapping_mul(2_654_435_761).wrapping_add(vehicle as u32 * 7 + leg as u32);
        crate::effects::impact(&mut commands, &assets, at, Vec3::Y, seed, voxel_assets::ImpactResponse::Stone);
        let ring = if stomped > 0 { STOMP_DUST } else if shoved > 0 { SHOVE_DUST } else { 0 };
        for i in 0..ring {
            let a = std::f32::consts::TAU * i as f32 / ring as f32 + leg as f32;
            let p = at + Vec3::new(a.cos(), 0.0, a.sin()) * DUST_RING;
            crate::effects::impact(&mut commands, &assets, p, (Vec3::new(a.cos(), 2.0, a.sin())).normalize(), seed.wrapping_add(i as u32 * 977), voxel_assets::ImpactResponse::Stone);
        }
        if let Some(sound) = &sound {
            sound.play(if stomped > 0 { crate::sound::Cue::Land } else { crate::sound::Cue::Step });
            if stomped > 0 {
                sound.play(crate::sound::Cue::Stone);
            }
        }
    }
}

fn parts(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Parts {
    let material = |materials: &mut Assets<StandardMaterial>, r, g, b, roughness| {
        materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, ..default() })
    };
    Parts {
        chassis: meshes.add(Cuboid::new(1.0, 0.3, 2.0)),
        hood: meshes.add(Cuboid::new(0.95, 0.28, 0.6)),
        bar: meshes.add(Cuboid::new(1.0, 0.7, 0.08)),
        wheel: meshes.add(Cylinder::new(WHEEL_RADIUS, WHEEL_WIDTH)),
        paint: material(materials, 0.93, 0.68, 0.10, 0.55),
        trim: material(materials, 0.18, 0.18, 0.2, 0.7),
        tire: material(materials, 0.08, 0.08, 0.08, 0.95),
        block: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        drum: meshes.add(Cylinder::new(0.5, 1.0)),
        finishes: crate::crawler::Finish::ALL.iter().map(|f| materials.add(f.material())).collect(),
    }
}

fn spawn(commands: &mut Commands, parts: &Parts, index: usize, wheelbase: f32, track: f32, crawler: Option<content::character::CrawlerDef>) -> Entity {
    if let Some(c) = crawler {
        return spawn_crawler(commands, parts, index, c);
    }
    let chassis = commands
        .spawn((Mesh3d(parts.chassis.clone()), MeshMaterial3d(parts.paint.clone()), Transform::default()))
        .with_children(|c| {
            c.spawn((Mesh3d(parts.hood.clone()), MeshMaterial3d(parts.paint.clone()), Transform::from_xyz(0.0, 0.25, 0.65)));
            c.spawn((Mesh3d(parts.bar.clone()), MeshMaterial3d(parts.trim.clone()), Transform::from_xyz(0.0, 0.5, -0.55)));
        })
        .id();
    let local = [(-track, wheelbase), (track, wheelbase), (-track, -wheelbase), (track, -wheelbase)].map(|(x, z)| Vec3::new(x / 2.0, 0.0, z / 2.0));
    let wheels = local.map(|at| commands.spawn((Mesh3d(parts.wheel.clone()), MeshMaterial3d(parts.tire.clone()), Transform::from_translation(at))).id());
    let mut root = commands.spawn((
        Rover { index, chassis, wheels, crawler: None, contact: None, spin: 0.0, last: None },
        WorldScoped::default(),
        Transform::default(),
        Visibility::default(),
    ));
    root.add_child(chassis);
    for wheel in wheels {
        root.add_child(wheel);
    }
    root.id()
}

/// A crawler from its rig: the body under its root, and each leg's hip and
/// segments, placed by `draw_crawler`.
fn spawn_crawler(commands: &mut Commands, parts: &Parts, index: usize, c: content::character::CrawlerDef) -> Entity {
    let rig = crate::crawler::builtin();
    let piece = |p: &crate::crawler::Piece| {
        let mesh = match p.shape {
            crate::crawler::Shape::Box => parts.block.clone(),
            crate::crawler::Shape::Cylinder => parts.drum.clone(),
        };
        let finish = crate::crawler::Finish::ALL.iter().position(|&f| f == p.material).expect("every finish");
        (Mesh3d(mesh), MeshMaterial3d(parts.finishes[finish].clone()), p.transform())
    };
    let mut group = |pieces: &[crate::crawler::Piece]| {
        commands
            .spawn((Transform::default(), Visibility::default()))
            .with_children(|g| {
                for p in pieces {
                    g.spawn(piece(p));
                }
            })
            .id()
    };
    let chassis = group(&rig.body);
    // The gun on the cab's roof, under the body, at its mount.
    let gun = c.gun.as_ref().map(|g| {
        let gun = group(&rig.gun);
        (gun, crate::crawler::gun_local(g))
    });
    let leg = &rig.leg;
    let segments = [(); 4].map(|_| [&leg.hip[..], &leg.thigh.pieces[..], &leg.shin.pieces[..], &leg.foot.pieces[..]].map(&mut group));
    if let Some((gun, at)) = gun {
        commands.entity(gun).insert(Transform::from_translation(at));
        commands.entity(chassis).add_child(gun);
    }
    let drawn = DrawnLegs { def: c, segments, gun: gun.map(|(e, _)| (e, 0.0)), last: None };
    let mut root = commands.spawn((
        Rover { index, chassis, wheels: [chassis; 4], crawler: Some(drawn), contact: None, spin: 0.0, last: None },
        WorldScoped::default(),
        Transform::default(),
        Visibility::default(),
    ));
    root.add_child(chassis);
    for entity in segments.iter().flatten() {
        root.add_child(*entity);
    }
    root.id()
}

/// Where a vehicle is drawn this frame: arena position on the ground, the
/// chassis base height, and its heading.
struct Pose {
    at: Vec3,
    base: f32,
    heading: f32,
}

fn interpolated(mirror: &WorldMirror, index: usize, now: &VehicleState) -> Pose {
    let before = mirror.previous.as_ref().and_then(|p| p.vehicles.get(index)).unwrap_or(now);
    let t = mirror.alpha;
    let at = mirror.to_view(before.position).lerp(mirror.to_view(now.position), t);
    let base = before.height.to_f32() + (now.height - before.height).to_f32() * t;
    let (a, b) = (before.heading.to_f32(), now.heading.to_f32());
    let turn = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    Pose { at, base, heading: a + turn * t }
}

fn draw(
    mut commands: Commands,
    mut vehicles: ResMut<Vehicles>,
    mirror: Res<WorldMirror>,
    service: Res<SessionService>,
    drawn: Res<crate::prediction::DrawnAvatar>,
    player: Query<&Transform, (With<Player>, Without<Rover>)>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rovers: Query<(Entity, &mut Rover)>,
    mut transforms: Query<&mut Transform, Without<Player>>,
    dispatcher: Option<Res<crate::input::gameplay::Dispatcher>>,
) {
    let (Some(current), Some(terrain)) = (&mirror.current, service.world().map(sim::World::terrain)) else { return };
    if current.vehicles.is_empty() {
        return;
    }
    // The vehicle the local player is drawn sitting in, if any, driving,
    // riding (CR.4) or at the gun (TU.1 of the turret), and a rider's or
    // the gunner's spot on the deck.
    let seat = drawn.0.as_ref().and_then(|d| d.avatar().seat);
    let mine = seat.and_then(sim::settlement::Seat::aboard).map(usize::from);
    let spot = seat.and_then(sim::settlement::crawler::deck_spot);
    let dt = time.delta_secs();
    // Riders and the gunner get the pilot's wider view too: they see what
    // it steps over.
    let aboard = mine.and_then(|i| current.vehicles.get(i)).is_some_and(|v| current.characters[v.character as usize].crawler.is_some());
    vehicles.pull = eased_pull(vehicles.pull, aboard, dt);
    let parts = vehicles.parts.get_or_insert_with(|| parts(&mut meshes, &mut materials));
    let tuning = |v: &VehicleState| current.characters[v.character as usize].vehicle.expect("a vehicle character");
    let drawn_count = rovers.iter().count();
    for (index, v) in current.vehicles.iter().enumerate().skip(drawn_count) {
        let t = tuning(v);
        spawn(&mut commands, parts, index, t.wheelbase.to_f32(), t.track.to_f32(), current.characters[v.character as usize].crawler.clone());
    }
    for (entity, mut rover) in &mut rovers {
        let Some(v) = current.vehicles.get(rover.index) else { continue };
        let t = tuning(v);
        let pose = match (mine == Some(rover.index), player.single()) {
            (true, Ok(p)) => {
                let (yaw, _, _) = p.rotation.to_euler(EulerRot::YXZ);
                // A rider or gunner sits off the middle: the body is that
                // far back.
                let off = spot.map_or(Vec3::ZERO, |o| deck_offset(yaw, o));
                Pose { at: Vec3::new(p.translation.x, 0.0, p.translation.z) - off, base: p.translation.y - t.seat.to_f32(), heading: yaw }
            }
            _ => interpolated(&mirror, rover.index, v),
        };
        if rover.crawler.is_some() && v.legs.is_some() {
            let reticle = dispatcher.as_ref().and_then(|d| d.town.reticle.as_ref());
            let aim = gun_aim(current, rover.index, v, &mirror, reticle);
            draw_crawler(&mut transforms, &mut rover, entity, &mirror, &pose, v, aim, dt);
            continue;
        }
        // The ground under each wheel where it is drawn: in a volume, the
        // floor it rolls on, as `vehicle::self_update` reads it.
        let points = wheel_points(mirror.to_sim(pose.at), Fx::quantize(pose.heading), &t);
        let reach = Fx::quantize(pose.base) + t.climb;
        let mut ground = points.map(|p| terrain.floor(p, reach).map_or(pose.base, |g| g.to_f32()));
        let mean = ground.iter().sum::<f32>() / 4.0;
        if pose.base > mean + AIRBORNE {
            // In the air the wheels hang at the chassis.
            ground = [pose.base; 4];
        }
        let jumped = rover.last.is_none_or(|last| last.distance(pose.at) > 1.5);
        let ease = 1.0 - (-WHEEL_EASE * dt).exp();
        let contact = match rover.contact {
            Some(c) if !jumped => std::array::from_fn(|i| c[i] + (ground[i] - c[i]) * ease),
            _ => ground,
        };
        rover.contact = Some(contact);
        if let Some(last) = rover.last.filter(|_| !jumped) {
            let forward = Vec3::new(pose.heading.sin(), 0.0, pose.heading.cos());
            rover.spin += (pose.at - last).dot(forward) / WHEEL_RADIUS;
        }
        rover.last = Some(pose.at);

        let (front, rear) = ((contact[0] + contact[1]) / 2.0, (contact[2] + contact[3]) / 2.0);
        let (left, right) = ((contact[0] + contact[2]) / 2.0, (contact[1] + contact[3]) / 2.0);
        let pitch = ((front - rear) / t.wheelbase.to_f32()).atan();
        let roll = ((left - right) / t.track.to_f32()).atan();
        let lift = (contact.iter().sum::<f32>() / 4.0 + CHASSIS_LIFT).max(pose.base + CHASSIS_LIFT);
        if let Ok(mut root) = transforms.get_mut(entity) {
            *root = Transform::from_translation(pose.at).with_rotation(Quat::from_rotation_y(pose.heading));
        }
        if let Ok(mut chassis) = transforms.get_mut(rover.chassis) {
            chassis.translation.y = lift;
            chassis.rotation = Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(-roll);
        }
        for (i, &wheel) in rover.wheels.iter().enumerate() {
            if let Ok(mut w) = transforms.get_mut(wheel) {
                w.translation.y = contact[i] + WHEEL_RADIUS;
                w.rotation = Quat::from_rotation_x(rover.spin) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            }
        }
    }
}

/// Where someone at `o` on a crawler's deck (a rider or the gunner,
/// `crawler::deck_spot`) sits from its middle, in the view, the crawler
/// facing `heading`.
fn deck_offset(heading: f32, o: content::character::Offset) -> Vec3 {
    let forward = Vec3::new(heading.sin(), 0.0, heading.cos());
    let right = Vec3::new(heading.cos(), 0.0, -heading.sin());
    forward * o.along.to_f32() + right * o.across.to_f32()
}

/// Where vehicle `index`'s gun aims now, in view coordinates: the local
/// gunner's reticle while they aim (`input::town::aim_reticle`), else an
/// elder gunner's confirmed reticle (an offset from the mount, longer than
/// the meter-long facing they boarded with), else its last shot's aim
/// (`Turret::aim`, a sprite's or an elder's); `None` before any.
fn gun_aim(current: &sim::settlement::Settlement, index: usize, v: &VehicleState, mirror: &WorldMirror, reticle: Option<&crate::input::town::Reticle>) -> Option<Vec3> {
    if let Some(r) = reticle.filter(|r| r.from == crate::input::town::Aimer::Gun(index as u16)) {
        return Some(r.point);
    }
    let (c, gun) = sim::settlement::gun::gun_of(&current.characters[v.character as usize])?;
    let g = v.gun.as_ref()?;
    let aimed = current.gunner_of(index as u16).and_then(|a| current.agents.get(a)?.avatar()?.aim).filter(|aim| aim.length() > Fx::ONE + Fx::from_ratio(1, 100));
    if let Some(aim) = aimed {
        let (mount, _) = sim::settlement::gun::mount(v, c, gun);
        return Some(mirror.to_view(mount + aim));
    }
    g.turret.aim.map(|p| mirror.to_view(p))
}

/// The camera's pull after `dt` seconds more, easing toward `PILOT_PULL`
/// while aboard a crawler (piloting or riding) and back to 1 after.
pub fn eased_pull(pull: f32, aboard: bool, dt: f32) -> f32 {
    let target = if aboard { PILOT_PULL } else { 1.0 };
    pull + (target - pull) * (1.0 - (-PULL_EASE * dt).exp())
}

/// A crawler: its body level at the height the simulation gives it, its
/// deck `deck` over that, and each leg solved from its hip to its foot.
/// Feet are interpolated between the last two ticks; a foot whose spot
/// jumps (`FOOT_JUMP`) glides there.
#[allow(clippy::too_many_arguments)]
fn draw_crawler(
    transforms: &mut Query<&mut Transform, Without<Player>>,
    rover: &mut Rover,
    entity: Entity,
    mirror: &WorldMirror,
    pose: &Pose,
    v: &VehicleState,
    aim: Option<Vec3>,
    dt: f32,
) {
    let (index, chassis) = (rover.index, rover.chassis);
    let drawn = rover.crawler.as_mut().expect("a crawler");
    let c = drawn.def.clone();
    let legs = v.legs.as_ref().expect("a crawler's legs");
    let deck = pose.base + c.deck.to_f32();
    let root = Transform::from_translation(pose.at).with_rotation(Quat::from_rotation_y(pose.heading));
    if let Ok(mut t) = transforms.get_mut(entity) {
        *t = root;
    }
    if let Ok(mut t) = transforms.get_mut(chassis) {
        *t = Transform::from_xyz(0.0, deck, 0.0);
    }
    // The gun turns to its aim from where its mount is drawn, eased; with
    // no aim it holds its turn.
    if let (Some((gun, yaw)), Some(def)) = (drawn.gun.as_mut(), c.gun.as_ref()) {
        let local = crate::crawler::gun_local(def) + Vec3::Y * deck;
        let mount = root.transform_point(local);
        if let Some(target) = aim.and_then(|aim| crate::crawler::gun_yaw(pose.heading, mount, aim)) {
            *yaw = crate::crawler::ease_yaw(*yaw, target, dt);
        }
        if let Ok(mut t) = transforms.get_mut(*gun) {
            *t = Transform::from_translation(crate::crawler::gun_local(def)).with_rotation(Quat::from_rotation_y(*yaw));
        }
    }
    let view = |p| mirror.to_view(p);
    let before = mirror.previous.as_ref().and_then(|p| p.vehicles.get(index)).and_then(|b| b.legs.as_ref()).unwrap_or(legs);
    let target: [Vec3; 4] = std::array::from_fn(|i| {
        let a = crate::crawler::foot_point(&before.feet[i], before, &c, view);
        a.lerp(crate::crawler::foot_point(&legs.feet[i], legs, &c, view), mirror.alpha)
    });
    let dt = dt.max(0.0);
    let jump = (FOOT_JUMP * dt).max(1.0);
    let offset: [Vec3; 4] = match drawn.last {
        // A crawler that moved far (a new scene, a respawn) snaps.
        Some((last, offset)) if last.iter().zip(&target).all(|(l, t)| l.distance(*t) < 6.0) => std::array::from_fn(|i| {
            let glide = offset[i] * 0.5f32.powf(dt / FOOT_GLIDE);
            if last[i].distance(target[i]) > jump { glide + last[i] - target[i] } else { glide }
        }),
        _ => [Vec3::ZERO; 4],
    };
    drawn.last = Some((target, offset));
    let shown: [Vec3; 4] = std::array::from_fn(|i| target[i] + offset[i]);
    let to_local = root.compute_affine().inverse();
    let turn = root.rotation.inverse();
    let rig = crate::crawler::builtin();
    for (leg, segments) in drawn.segments.iter().enumerate() {
        let (hip, out) = crate::crawler::hip_local(&c, leg, deck);
        let out = root.rotation * out;
        let solved = crate::crawler::solve(&rig.leg, root.transform_point(hip), out, shown[leg]);
        for (frame, &segment) in solved.frames(out).iter().zip(segments) {
            if let Ok(mut t) = transforms.get_mut(segment) {
                *t = Transform::from_translation(to_local.transform_point3(frame.translation)).with_rotation(turn * frame.rotation);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local rider's crawler is drawn back from the rider by their place
    /// as the simulation seats them (`VehicleState::seat_at`), at any
    /// heading.
    #[test]
    fn a_riders_offset_matches_their_seat() {
        let content = crate::lockstep::builtin_content();
        let ix = content.names.character_index("crawler").unwrap();
        let def = &content.rules.characters[ix as usize];
        for degrees in [0, 37, 90, 200, 300] {
            let heading = Fx::from_ratio(degrees * 31_416, 1_800_000);
            let v = VehicleState { character: ix, position: fx::FxVec2::ZERO, heading, speed: Fx::ZERO, height: Fx::ZERO, fall: Fx::ZERO, wheels: [Fx::ZERO; 4], control: Default::default(), legs: None, gun: None };
            let seats = [sim::settlement::Seat::Rider { vehicle: 0, place: 0 }, sim::settlement::Seat::Rider { vehicle: 0, place: 1 }, sim::settlement::Seat::Gunner { vehicle: 0 }];
            for place in seats {
                let o = sim::settlement::crawler::deck_spot(place).unwrap();
                let (seat, _) = v.seat_at(def, Some(o));
                let off = deck_offset(heading.to_f32(), o);
                assert!((off - Vec3::new(seat.x.to_f32(), 0.0, seat.y.to_f32())).length() < 1e-3, "{place:?} at {degrees} degrees: {off} vs {seat:?}");
            }
        }
    }

    #[test]
    fn the_pull_eases_out_while_piloting_and_back_after() {
        let mut pull = 1.0;
        for _ in 0..90 {
            pull = eased_pull(pull, true, 1.0 / 30.0);
        }
        assert!((pull - PILOT_PULL).abs() < 0.05 * PILOT_PULL, "pulled back in 3 s: {pull}");
        let half = eased_pull(1.0, true, 0.2);
        assert!(half > 1.0 && half < 1.0 + (PILOT_PULL - 1.0) / 2.0, "eased, not snapped: {half}");
        for _ in 0..90 {
            pull = eased_pull(pull, false, 1.0 / 30.0);
        }
        assert!((pull - 1.0).abs() < 0.03, "back in: {pull}");
    }

    /// In the Crawler Lab the camera pulls back once the elder pilots the
    /// crawler, and eases back in once they climb down.
    #[test]
    fn the_camera_pulls_back_while_piloting_a_crawler() {
        let script = serde_json::json!({
            "version": 1, "scene": "crawler-lab", "fps": 30, "width": 640, "height": 480, "max_frames": 600,
            "steps": [
                { "op": "wait", "frames": 30 },
                { "op": "walk", "position": [0.6, 0] },
                { "op": "wait", "frames": 20 },
                { "op": "board" },
                { "op": "wait", "frames": 120 },
                { "op": "board" },
                { "op": "wait", "frames": 150 }
            ]
        });
        let options = crate::control::Options { simulation: true, script: Some(serde_json::from_value(script).unwrap()), ..default() };
        let library = crate::assets::SceneLibrary::new();
        let index = library.index("crawler-lab").unwrap();
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, library, hub, crate::flow::Launch::single_player(index));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 30.0)));
        crate::start_world(&mut app);
        let (mut most, mut piloted, mut last) = (1.0f32, false, 1.0);
        for _ in 0..500 {
            app.update();
            let drawn = app.world().resource::<crate::prediction::DrawnAvatar>().0.as_ref().and_then(|d| d.avatar().seat);
            piloted |= drawn.is_some();
            last = app.world().get_resource::<Vehicles>().map_or(1.0, Vehicles::pull);
            most = most.max(last);
            if app.world().resource::<crate::control::Hub>().finished.is_some() {
                break;
            }
        }
        assert!(piloted, "the elder boarded");
        assert!(most > PILOT_PULL - 0.05, "pulled back to {most}");
        assert!(last < 1.05, "eased back in to {last}");
    }

    /// A scripted guest pilots the crawler (`guest_drive`, TU.6 of the
    /// crawler turret) while the local elder mans its gun and fires: the
    /// guest's stick walks it south, the elder's doesn't steer it.
    #[test]
    fn a_scripted_guest_pilots_while_the_elder_mans_the_gun() {
        let script = serde_json::json!({
            "version": 1, "scene": "crawler-lab", "fps": 30, "width": 640, "height": 480, "max_frames": 600,
            "steps": [
                { "op": "guest_join", "guest": 1 },
                { "op": "wait", "frames": 10 },
                { "op": "guest_walk", "guest": 1, "position": [0.2, -1.4] },
                { "op": "wait", "frames": 14 },
                { "op": "walk", "position": [0.6, 0] },
                { "op": "wait", "frames": 20 },
                { "op": "guest_board", "guest": 1 },
                { "op": "wait", "frames": 10 },
                { "op": "board" },
                { "op": "wait", "frames": 20 },
                { "op": "guest_drive", "guest": 1, "direction": [0, 1] },
                { "op": "settlement_fire", "x": 0.0, "y": 5.0 },
                { "op": "wait", "frames": 150 },
                { "op": "guest_drive", "guest": 1, "direction": [0, 0] },
                { "op": "wait", "frames": 30 }
            ]
        });
        let options = crate::control::Options { simulation: true, script: Some(serde_json::from_value(script).unwrap()), ..default() };
        let library = crate::assets::SceneLibrary::new();
        let index = library.index("crawler-lab").unwrap();
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, library, hub, crate::flow::Launch::single_player(index));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 30.0)));
        crate::start_world(&mut app);
        let (mut start, mut state, mut bolts) = (None, serde_json::Value::Null, 0);
        for _ in 0..560 {
            app.update();
            let hub = app.world().resource::<crate::control::Hub>();
            assert!(hub.failed.is_none(), "{}", hub.report(0));
            if hub.finished.is_some() {
                break;
            }
            state = crate::settlement::snapshot(app.world());
            let guest = state["simulation"]["agents"]["player-1"]["position"][1].as_f64();
            if state["seat"]["gunner"] == true {
                start = start.or(guest);
            }
            bolts = bolts.max(state["simulation"]["projectiles"].as_object().map_or(0, |p| p.len()));
        }
        assert_eq!(state["seat"], serde_json::json!({"vehicle": 0, "gunner": true}), "the elder mans the gun");
        let moved = state["simulation"]["agents"]["player-1"]["position"][1].as_f64().unwrap() - start.expect("the elder boarded");
        assert!(moved > 1.0, "the guest walked it south: {moved} m");
        assert_eq!(bolts, 1, "the elder fired once");
    }

    /// The control protocol's sprite job `"crew"` (TU.4 of the crawler
    /// turret) sends a lab sprite to the crawler's gun, as a demo script
    /// does: the view reads it as at the gun and draws it up on the deck.
    #[test]
    fn a_scripted_sprite_crews_the_crawler() {
        let script = serde_json::json!({
            "version": 1, "scene": "crawler-lab", "fps": 30, "width": 640, "height": 480, "max_frames": 1500,
            "steps": [
                { "op": "wait", "frames": 10 },
                { "op": "settlement_sprite_job", "sprite": "sprite_ash", "job": "crew" },
                { "op": "wait", "frames": 1400 }
            ]
        });
        let options = crate::control::Options { simulation: true, script: Some(serde_json::from_value(script).unwrap()), ..default() };
        let library = crate::assets::SceneLibrary::new();
        let index = library.index("crawler-lab").unwrap();
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, library, hub, crate::flow::Launch::single_player(index));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 30.0)));
        crate::start_world(&mut app);
        let mut state = serde_json::Value::Null;
        for _ in 0..1450 {
            app.update();
            state = crate::settlement::snapshot(app.world());
            if state["simulation"]["sprites"]["sprite_ash"]["activity"] == "gunning" {
                break;
            }
        }
        // A few frames on, past the tick it climbed up on.
        for _ in 0..5 {
            app.update();
        }
        state = crate::settlement::snapshot(app.world());
        let ash = &state["simulation"]["sprites"]["sprite_ash"];
        assert_eq!((ash["job"].as_str(), ash["activity"].as_str()), (Some("crew"), Some("gunning")), "{ash}");
        // Drawn on the deck, about 3 m up, not on the ground.
        let feet = state["simulation"]["agents"]["sprite_ash"]["feet"].as_f64().unwrap();
        assert!(feet > 2.0, "its feet at {feet} m");
        // A sprite gunner gives the elder on foot no reticle (TU.5); the
        // drawn gun turns to its shots once it fires.
        assert_eq!(app.world().resource::<crate::input::gameplay::Dispatcher>().town.reticle, None);
        let mut rovers = app.world_mut().query::<&Rover>();
        assert!(rovers.iter(app.world()).any(|r| r.gun_yaw().is_some()), "the crawler carries its gun");
    }
}

use crate::{
    effects::{self, Feedback, FxAssets},
    impact::{self, ImpactTarget},
    navigation::{self, flat, Disc},
    sound::{Cue, Sound},
};
use bevy::prelude::*;

pub const WALK_SPEED: f32 = 3.8;
pub const WINDUP: f32 = 0.16;
pub const ACTIVE_END: f32 = 0.29;
pub const ATTACK_END: f32 = 0.58;
pub const REBOUND_END: f32 = 0.42;
pub const REST_ANGLE: f32 = 0.45;

#[derive(Component)]
pub struct Player;
#[derive(Component, Clone, Copy)]
pub struct Obstacle(pub Disc);
#[derive(Component)]
pub struct Part {
    pub kind: PartKind,
    pub rest: Transform,
}
#[derive(Clone, Copy)]
pub enum PartKind {
    Body,
    Leg(f32),
    Sword,
}

#[derive(Clone, Copy, Debug)]
pub struct Attack {
    pub serial: u64,
    pub age: f32,
    pub aim: Vec3,
    pub impact_angle: Option<f32>,
    pub whooshed: bool,
    pub weapon: content::scenario::Weapon,
}
impl Attack {
    pub fn new(aim: Vec3, weapon: content::scenario::Weapon) -> Self {
        Self {
            serial: 0,
            age: 0.0,
            aim: flat(aim).normalize_or(Vec3::Z),
            impact_angle: None,
            whooshed: false,
            weapon,
        }
    }
    pub fn angle(&self) -> f32 {
        if self.weapon == content::scenario::Weapon::Spear {
            return 0.05;
        }
        if let Some(angle) = self.impact_angle {
            if self.age < 0.15 {
                lerp(angle, angle - 0.9, ease_out(self.age / 0.15))
            } else {
                lerp(
                    angle - 0.9,
                    REST_ANGLE,
                    smooth((self.age - 0.15) / (REBOUND_END - 0.15)),
                )
            }
        } else {
            match self.weapon {
                content::scenario::Weapon::Sword => swing_angle(self.age),
                content::scenario::Weapon::Spear => -0.15 + 0.25 * smooth(self.age / 0.22),
                content::scenario::Weapon::Bow => -0.9 + 0.4 * smooth(self.age / 0.22),
                content::scenario::Weapon::ArcCoil => -0.5 + 0.8 * smooth(self.age / 0.22),
            }
        }
    }
    pub fn finished(&self) -> bool {
        self.age
            >= if self.impact_angle.is_some() {
                REBOUND_END
            } else {
                ATTACK_END
            }
    }
}

#[derive(Resource, Default)]
pub struct Controls {
    pub attack_serial: u64,
    pub weapon: content::scenario::Weapon,
    pub route: Vec<Vec3>,
    pub velocity: Vec3,
    pub stride: f32,
    pub foot_distance: f32,
    pub attack: Option<Attack>,
    pub queued: Option<(Vec3, f32)>,
    pub repath: f32,
    pub impacts: u32,
    pub last_material: Option<voxel_assets::ImpactResponse>,
}
impl Controls {
    pub fn strike(&mut self, aim: Vec3) {
        if self.attack.is_none() {
            self.attack_serial += 1;
            let mut attack = Attack::new(aim, self.weapon);
            attack.serial = self.attack_serial;
            self.attack = Some(attack);
            self.route.clear();
        } else {
            // A short recovery buffer accepts the next deliberate click, not a held button.
            self.queued = Some((aim, 0.18));
        }
    }
}

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn swing_angle(age: f32) -> f32 {
    if age < WINDUP {
        lerp(REST_ANGLE, -1.7, smooth(age / WINDUP))
    } else if age < ACTIVE_END {
        lerp(
            -1.7,
            1.4,
            ((age - WINDUP) / (ACTIVE_END - WINDUP)).powf(1.6),
        )
    } else {
        lerp(
            1.4,
            REST_ANGLE,
            smooth((age - ACTIVE_END) / (ATTACK_END - ACTIVE_END)),
        )
    }
}

/// Pull the shaft in, drive the point forward over eleven frames, then recover.
/// On contact recovery starts from full extension so the weapon never snaps home.
pub fn spear_pose(age: f32, contacted: bool) -> (f32, f32) {
    if contacted {
        let f = smooth(age / 0.33);
        return (1.05 * (1.0 - f), -0.14 + 0.22 * f);
    }
    if age < 0.12 {
        let f = smooth(age / 0.12);
        (-0.38 * f, 0.17 + 0.18 * f)
    } else if age < 0.23 {
        let f = ease_out((age - 0.12) / 0.11);
        (-0.38 + 1.43 * f, 0.35 - 0.49 * f)
    } else if age < 0.30 {
        (1.05, -0.14)
    } else {
        let f = smooth((age - 0.30) / 0.28);
        (1.05 * (1.0 - f), -0.14 + 0.31 * f)
    }
}

pub fn cursor_ground(
    camera: &Camera,
    transform: &GlobalTransform,
    window: &Window,
) -> Option<Vec3> {
    let ray = camera
        .viewport_to_world(transform, window.cursor_position()?)
        .ok()?;
    Some(ray.get_point(ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?))
}

/// Earliest contact along a swept blade, sampled in angle space (not frame time).
/// This prevents a fast cut from tunneling through a rock at low frame rates.
#[cfg(test)]
pub fn swept_hit(
    origin: Vec3,
    facing: Quat,
    from: f32,
    to: f32,
    obstacles: &[Disc],
) -> Option<(usize, f32, Vec3)> {
    let samples = ((to - from).abs() / 0.025).ceil().max(1.0) as usize;
    for sample in 0..=samples {
        let angle = lerp(from, to, sample as f32 / samples as f32);
        let pivot = origin + facing * Vec3::new(0.4, 0.9, 0.0);
        let blade = facing * Quat::from_rotation_y(angle) * Vec3::Z;
        let (base, tip) = blade_segment(origin, facing, angle, 1.0);
        let hit = obstacles
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                o.radius > 0.0
                    && navigation::segment_distance(o.center, base, tip) <= o.radius + 0.10
            })
            .min_by(|(_, a), (_, b)| {
                a.center
                    .distance_squared(pivot)
                    .total_cmp(&b.center.distance_squared(pivot))
            });
        if let Some((index, disc)) = hit {
            let nearest = flat(disc.center - base).dot(blade).clamp(0.0, 1.3);
            return Some((index, angle, base + blade * nearest));
        }
    }
    None
}

fn blade_segment(origin: Vec3, facing: Quat, angle: f32, scale: f32) -> (Vec3, Vec3) {
    let pivot = origin + facing * Vec3::new(0.4, 0.9, 0.0) * scale;
    let blade = facing * Quat::from_rotation_y(angle) * Vec3::Z;
    (pivot + blade * 0.20 * scale, pivot + blade * 1.50 * scale)
}

/// The local player's body (World layer): placed where the simulation has
/// the avatar, animated, and its swings and hits played.
#[derive(Resource)]
pub struct Avatar;

impl service::Service for Avatar {
    type Layer = service::WorldLayer;
    type Deps = (crate::lockstep::WorldMirror, crate::scene_view::SceneView);

    fn init(_world: &mut World) -> Self {
        Avatar
    }

    fn systems(app: &mut App) {
        use service::{AppServices, Phase, WorldState};
        let active = in_state(WorldState::Active);
        app.add_service_systems::<Self, _>(Phase::Mirror, crate::lockstep::follow_avatar.run_if(active.clone()))
            .add_service_systems::<Self, _>(
                Phase::Present,
                (
                    (combat, animate_character, crate::style::select_weapon).chain(),
                    draw_navigation.run_if(crate::control::interactive),
                )
                    .chain()
                    .run_if(active),
            );
    }
}

pub fn combat(
    mut commands: Commands,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    mut obstacles: Query<(&Obstacle, Option<&mut ImpactTarget>)>,
    mut feedback: ResMut<Feedback>,
    assets: Res<FxAssets>,
    sound: Res<Sound>,
) {
    let dt = time.delta_secs().min(0.1);
    if let Some((_, remaining)) = &mut controls.queued {
        *remaining -= dt;
        if *remaining <= 0.0 {
            controls.queued = None;
        }
    }
    if feedback.hit_stop > 0.0 {
        return;
    }
    let Some(mut attack) = controls.attack else {
        return;
    };
    if attack.age < 0.05 {
        player.rotation = Quat::from_rotation_y(attack.aim.x.atan2(attack.aim.z));
    }
    let previous_age = attack.age;
    attack.age += dt;
    if attack.weapon == content::scenario::Weapon::Sword
        && attack.impact_angle.is_none()
        && attack.age >= WINDUP
        && previous_age < ACTIVE_END
    {
        if !attack.whooshed {
            sound.play(Cue::Swing);
            attack.whooshed = true;
        }
        let from = swing_angle(previous_age.max(WINDUP));
        let to = swing_angle(attack.age.min(ACTIVE_END));
        let discs: Vec<_> = obstacles
            .iter()
            .map(|(o, rock)| Disc {
                center: o.0.center,
                radius: rock.as_ref().map_or(o.0.radius, |r| r.contact_radius),
            })
            .collect();
        // Broad-phase discs reject distant targets; exact prefab triangles select
        // the contacted material and prevent feedback in empty parts of the bounds.
        let samples = ((to - from).abs() / 0.025).ceil().max(1.0) as usize;
        let mut contact = None;
        for sample in 0..=samples {
            let angle = lerp(from, to, sample as f32 / samples as f32);
            let blade = player.rotation * Quat::from_rotation_y(angle) * Vec3::Z;
            let (base, tip) =
                blade_segment(player.translation, player.rotation, angle, player.scale.x);
            let mut candidates = Vec::new();
            for (index, (_, target)) in obstacles.iter().enumerate() {
                let disc = discs[index];
                if disc.radius <= 0.0
                    || navigation::segment_distance(disc.center, base, tip) > disc.radius + 0.10
                {
                    continue;
                }
                if let Some(target) = target {
                    if let Some((t, material, normal)) = target.contact(base, tip) {
                        candidates.push((t, index, material, base.lerp(tip, t), normal));
                    }
                } else {
                    let nearest = flat(disc.center - base)
                        .dot(blade)
                        .clamp(0.0, base.distance(tip));
                    let position = base + blade * nearest;
                    let normal = flat(position - disc.center).normalize_or(-attack.aim);
                    candidates.push((
                        base.distance(position) / base.distance(tip),
                        index,
                        0,
                        position,
                        normal,
                    ));
                }
            }
            if let Some((_, index, material, position, normal)) =
                candidates.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))
            {
                contact = Some((index, angle, material, position, normal));
                break;
            }
        }
        if let Some((index, angle, material, position, normal)) = contact {
            let mut kind = voxel_assets::ImpactResponse::Metal;
            if let Some((_, Some(mut target))) = obstacles.iter_mut().nth(index) {
                let slot = &mut target.slots[material];
                kind = slot.kind;
                slot.flash = 1.0;
                target.kick = -normal * impact::response(kind).kick;
            }
            controls.last_material = Some(kind);
            let response = impact::response(kind);
            attack.impact_angle = Some(angle);
            attack.age = 0.0;
            controls.velocity = Vec3::ZERO;
            controls.impacts += 1;
            feedback.hit_stop = response.hit_stop;
            feedback.trauma = response.trauma;
            feedback.flash = 1.0;
            effects::impact(
                &mut commands,
                &assets,
                position,
                normal,
                controls.impacts,
                kind,
            );
            sound.play(match kind {
                voxel_assets::ImpactResponse::Stone => Cue::Stone,
                voxel_assets::ImpactResponse::Metal => Cue::Clang,
                voxel_assets::ImpactResponse::Crystal => Cue::Crystal,
                voxel_assets::ImpactResponse::Polymer => Cue::Polymer,
            });
        } else {
            // Short-lived afterimages follow the actual blade sweep.
            for fraction in [0.25, 0.65, 1.0] {
                let angle = lerp(from, to, fraction);
                effects::blade_echo(
                    &mut commands,
                    &assets,
                    player.translation,
                    player.rotation,
                    angle,
                    player.scale.x,
                );
            }
        }
    }
    if attack.finished() {
        controls.attack = None;
        if let Some((aim, _)) = controls.queued.take() {
            controls.strike(aim);
        }
    } else {
        controls.attack = Some(attack);
    }
}

pub fn animate_character(
    time: Res<Time>,
    controls: Res<Controls>,
    feedback: Res<Feedback>,
    flight: Option<Res<crate::jetpack::Flight>>,
    mirror: Res<crate::lockstep::WorldMirror>,
    mut parts: Query<(&Part, &mut Transform), Without<Player>>,
) {
    if feedback.hit_stop > 0.0
        && !controls
            .attack
            .is_some_and(|a| a.impact_angle.is_some() && a.age == 0.0)
    {
        return;
    }
    let airborne = flight.as_ref().is_some_and(|f| f.airborne);
    let speed = if airborne {
        0.0
    } else {
        (controls.velocity.length() / WALK_SPEED).min(1.4)
    };
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    let dig = mirror.player().zip(mirror.current.as_ref()).and_then(|(i, s)| s.agents[i as usize].avatar.dig.as_ref()).map(|d| dig_age(&mirror, d));
    let pose = Pose {
        stride: controls.stride,
        speed,
        breathe,
        airborne,
        attack: controls.attack,
        dig,
    };
    for (part, mut transform) in &mut parts {
        *transform = pose.place(part.kind, part.rest);
    }
}

/// What a character is doing this frame, for posing its parts.
pub struct Pose {
    /// Walk cycle phase.
    pub stride: f32,
    /// Walking speed relative to `WALK_SPEED`.
    pub speed: f32,
    pub breathe: f32,
    pub airborne: bool,
    pub attack: Option<Attack>,
    /// Seconds into a dig.
    pub dig: Option<f32>,
}

/// One chop of a dig: the tool comes down every half second, when the
/// dust flies (`digging::feedback`).
const CHOP: f32 = 0.5;

/// Seconds into a dig, at render time.
pub fn dig_age(mirror: &crate::lockstep::WorldMirror, dig: &sim::settlement::avatar::Dig) -> f32 {
    let now = mirror.tick as f32 - 1.0 + mirror.alpha;
    (now - dig.started as f32).max(0.0) / content::TICKS_PER_SECOND as f32
}

/// The tool's pitch through a chop (positive points it at the ground), and
/// how far down the swing is (0 raised overhead, 1 in the ground). It rises
/// slowly for most of the beat and drives down at the end.
fn chop(age: f32) -> (f32, f32) {
    const DOWN: f32 = 0.95;
    const UP: f32 = -1.9;
    const RISE: f32 = 0.7;
    let beat = (age / CHOP).fract();
    let pitch = if beat < RISE {
        lerp(DOWN, UP, smooth(beat / RISE))
    } else {
        lerp(UP, DOWN, ((beat - RISE) / (1.0 - RISE)).powi(2))
    };
    (pitch, (pitch - UP) / (DOWN - UP))
}

impl Pose {
    /// A part's rest transform, posed.
    pub fn place(&self, kind: PartKind, rest: Transform) -> Transform {
        if let Some(age) = self.dig {
            return self.digging(kind, rest, age);
        }
        let mut transform = rest;
        match kind {
            PartKind::Body => {
                transform.translation.y += self.breathe + self.stride.sin().abs() * 0.035 * self.speed;
                transform.rotation = Quat::from_rotation_x(0.06 * self.speed);
                if let Some(attack) = self.attack {
                    let twist = if attack.weapon == content::scenario::Weapon::Spear {
                        let (extension, _) = spear_pose(attack.age, attack.impact_angle.is_some());
                        transform.rotation *= Quat::from_rotation_x(-0.14 * extension.max(0.0));
                        transform.translation.z += extension.max(0.0) * 0.16;
                        -0.10 * extension
                    } else if attack.impact_angle.is_some() {
                        -0.18 * (1.0 - smooth(attack.age / REBOUND_END))
                    } else {
                        (attack.angle() - REST_ANGLE) * 0.12
                    };
                    transform.rotation *= Quat::from_rotation_y(twist);
                    transform.translation.y -= (twist.abs() * 0.12).min(0.06);
                }
            }
            PartKind::Leg(side) => {
                transform.rotation =
                    Quat::from_rotation_x(self.stride.sin() * 0.55 * self.speed * side);
                if self.airborne {
                    transform.rotation = Quat::from_rotation_x(-0.25);
                } else if self.attack.is_some() {
                    transform.rotation = Quat::from_rotation_x(-0.12 * side);
                }
            }
            PartKind::Sword => {
                let angle = self.attack.map_or(REST_ANGLE, |a| a.angle());
                let tilt = self.attack.map_or(-0.6, |a| {
                    if a.impact_angle.is_some() {
                        -0.1 - 0.5 * smooth(a.age / REBOUND_END)
                    } else if a.age < WINDUP {
                        lerp(-0.6, -0.08, smooth(a.age / WINDUP))
                    } else if a.age < ACTIVE_END {
                        -0.08
                    } else {
                        lerp(
                            -0.08,
                            -0.6,
                            smooth((a.age - ACTIVE_END) / (ATTACK_END - ACTIVE_END)),
                        )
                    }
                });
                transform.rotation = Quat::from_rotation_y(angle) * Quat::from_rotation_x(tilt);
                if let Some(attack) = self.attack {
                    if attack.weapon == content::scenario::Weapon::Spear {
                        let (extension, pitch) =
                            spear_pose(attack.age, attack.impact_angle.is_some());
                        transform.translation.z += extension;
                        transform.translation.y -= extension.max(0.0) * 0.04;
                        transform.rotation = Quat::from_rotation_x(pitch);
                    }
                }
                transform.translation.y += self.breathe;
            }
        }
        transform
    }

    /// Digging: crouched over the tile, feet apart, chopping into the ground
    /// in front, leaning into each strike.
    fn digging(&self, kind: PartKind, rest: Transform, age: f32) -> Transform {
        let (pitch, down) = chop(age);
        let mut transform = rest;
        match kind {
            PartKind::Body => {
                transform.translation.y += self.breathe - 0.09 - 0.05 * down;
                transform.rotation = Quat::from_rotation_x(0.22 + 0.2 * down);
            }
            PartKind::Leg(side) => {
                transform.rotation = Quat::from_rotation_x(-0.3 * side) * Quat::from_rotation_z(0.12 * side);
            }
            PartKind::Sword => {
                transform.translation.x *= 0.4;
                transform.translation.z += 0.18;
                transform.translation.y -= 0.05 + 0.1 * down;
                transform.rotation = Quat::from_rotation_x(pitch);
            }
        }
        transform
    }
}

pub fn draw_navigation(
    controls: Res<Controls>,
    player: Single<&Transform, With<Player>>,
    mut gizmos: Gizmos,
    scene: Res<crate::scene_view::SceneView>,
    flight: Res<crate::jetpack::Flight>,
    time: Res<Time>,
) {
    if let Some(goal) = controls.route.last() {
        let color = Color::srgba(0.25, 0.95, 1.0, 0.7);
        let y = Vec3::Y * 0.035;
        let waypoint = |p: Vec3| {
            Vec3::new(
                p.x,
                crate::jetpack::height(&scene.pack, p).unwrap_or(flight.support)
                    + if flight.airborne {
                        crate::jetpack::HOVER
                    } else {
                        0.0
                    },
                p.z,
            )
        };
        let r = 0.24 + 0.025 * (time.elapsed_secs() * 7.0).sin();
        gizmos.circle(
            Isometry3d::new(
                waypoint(*goal) + y,
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
            r,
            color,
        );
        let mut previous = player.translation + y;
        for point in &controls.route {
            let end = waypoint(*point) + y;
            let delta = end - previous;
            let count = (delta.length() / 0.3) as usize;
            for i in 0..count {
                let a = previous + delta.normalize_or_zero() * i as f32 * 0.3;
                gizmos.line(
                    a,
                    a + delta.normalize_or_zero() * 0.08,
                    Color::srgba(0.22, 0.65, 0.72, 0.3),
                );
            }
            previous = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rock_contact_rebounds_once_freezes_then_cleans_up_effects() {
        let mut app = App::new();
        let fx = FxAssets {
            cube: default(),
            spark: std::array::from_fn(|_| default()),
            debris: std::array::from_fn(|_| default()),
            trail: default(),
            blood: std::array::from_fn(|_| default()),
        };
        app.init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .init_resource::<Feedback>()
            .init_resource::<Assets<StandardMaterial>>()
            .insert_resource(fx)
            .insert_resource(Sound::silent())
            .add_systems(
                Update,
                (combat, effects::update_effects).chain(),
            );
        let player = app.world_mut().spawn((Player, Transform::default())).id();
        app.world_mut().spawn((Obstacle(Disc {
            center: Vec3::new(0.3, 0.0, 1.65),
            radius: 0.4,
        }),));
        app.world_mut().resource_mut::<Controls>().strike(Vec3::Z);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
        for _ in 0..70 {
            app.update();
            if app.world().resource::<Controls>().impacts > 0 {
                break;
            }
        }
        assert_eq!(app.world().resource::<Controls>().impacts, 1);
        assert!(app.world().resource::<Feedback>().hit_stop > 0.0);
        let age = app.world().resource::<Controls>().attack.unwrap().age;
        app.update();
        assert_eq!(app.world().resource::<Controls>().attack.unwrap().age, age);
        for _ in 0..180 {
            app.update();
        }
        assert_eq!(app.world().resource::<Controls>().impacts, 1);
        assert!(app.world().resource::<Controls>().attack.is_none());
        assert_eq!(
            app.world_mut()
                .query::<&effects::Particle>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query::<&effects::Flash>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn combat_uses_the_struck_quadrant_at_different_frame_rates() {
        use crate::impact::{surfaces, ImpactSlot, KINDS};
        use voxel_assets::{generate_boulder, BoulderRecipe, MaterialPattern};
        let model = generate_boulder(&BoulderRecipe {
            radius: 1.15,
            height: 1.6,
            voxel_size: 0.25,
            taper: 0.15,
            debris: 0,
            material_pattern: MaterialPattern::Quadrants,
            ..default()
        })
        .unwrap();
        for dt in [1.0 / 30.0, 1.0 / 120.0] {
            for (slot, (x, z)) in [(-1.8, -1.8), (1.8, -1.8), (-1.8, 1.8), (1.8, 1.8)]
                .into_iter()
                .enumerate()
            {
                let mut app = App::new();
                app.init_resource::<Time>()
                    .init_resource::<Controls>()
                    .init_resource::<Feedback>()
                    .insert_resource(Sound::silent())
                    .insert_resource(FxAssets {
                        cube: default(),
                        spark: std::array::from_fn(|_| default()),
                        debris: std::array::from_fn(|_| default()),
                        trail: default(),
                        blood: std::array::from_fn(|_| default()),
                    })
                    .add_systems(Update, combat);
                let position = Vec3::new(x, 0.0, z);
                let aim = -position.normalize();
                app.world_mut().spawn((
                    Player,
                    Transform::from_translation(position)
                        .with_rotation(Quat::from_rotation_y(aim.x.atan2(aim.z))),
                ));
                let target = app
                    .world_mut()
                    .spawn((
                        Obstacle(Disc {
                            center: Vec3::ZERO,
                            radius: model.collision.radius,
                        }),
                        ImpactTarget {
                            base: Vec3::ZERO,
                            contact_radius: model.collision.strike_radius_at(0.9),
                            kick: Vec3::ZERO,
                            slots: KINDS
                                .map(|kind| ImpactSlot {
                                    handle: default(),
                                    rest_emissive: LinearRgba::BLACK,
                                    kind,
                                    flash: 0.0,
                                })
                                .into(),
                            surfaces: surfaces(&model),
                            local_from_world: bevy::math::Affine3A::IDENTITY,
                        },
                    ))
                    .id();
                app.world_mut().resource_mut::<Controls>().strike(aim);
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs_f32(dt));
                for _ in 0..60 {
                    app.update();
                    if app.world().resource::<Controls>().impacts > 0 {
                        break;
                    }
                }
                let controls = app.world().resource::<Controls>();
                assert_eq!(controls.impacts, 1, "quadrant {slot} at dt {dt}");
                assert_eq!(controls.last_material, Some(KINDS[slot]));
                assert_eq!(
                    app.world().resource::<Feedback>().hit_stop,
                    impact::response(KINDS[slot]).hit_stop
                );
                let target = app.world().get::<ImpactTarget>(target).unwrap();
                for (index, material) in target.slots.iter().enumerate() {
                    assert_eq!(material.flash, if index == slot { 1.0 } else { 0.0 });
                }
            }
        }
    }

    #[test]
    fn blade_sweep_hits_only_during_forward_cut_and_at_reach() {
        let rock = Disc {
            center: Vec3::new(0.3, 0.0, 1.65),
            radius: 0.4,
        };
        let hit = swept_hit(Vec3::ZERO, Quat::IDENTITY, -1.7, 1.4, &[rock]).unwrap();
        assert!(hit.1 < 0.3 && hit.1 > -1.0);
        assert!(swept_hit(Vec3::ZERO, Quat::IDENTITY, -1.7, -1.6, &[rock]).is_none());
        assert!(swept_hit(
            Vec3::ZERO,
            Quat::IDENTITY,
            -1.7,
            1.4,
            &[Disc {
                center: Vec3::Z * 4.0,
                radius: 0.5
            }]
        )
        .is_none());
        assert!(swept_hit(
            Vec3::ZERO,
            Quat::IDENTITY,
            -1.7,
            1.4,
            &[Disc {
                center: -Vec3::Z * 2.0,
                radius: 0.4
            }]
        )
        .is_none());
    }
    #[test]
    fn swept_hit_is_consistent_when_cut_is_split_into_frames() {
        let rock = Disc {
            center: Vec3::new(0.3, 0.0, 1.65),
            radius: 0.4,
        };
        let full = swept_hit(Vec3::ZERO, Quat::IDENTITY, -1.7, 1.4, &[rock]).unwrap();
        let split = (0..31)
            .find_map(|i| {
                swept_hit(
                    Vec3::ZERO,
                    Quat::IDENTITY,
                    -1.7 + i as f32 * 0.1,
                    -1.6 + i as f32 * 0.1,
                    &[rock],
                )
            })
            .unwrap();
        assert!((full.1 - split.1).abs() < 0.03);
    }
    #[test]
    fn heavy_cut_has_anticipation_rebound_and_recovery() {
        assert!((swing_angle(0.0) - REST_ANGLE).abs() < 1e-5);
        assert!((swing_angle(WINDUP) + 1.7).abs() < 1e-5);
        assert!((swing_angle(ACTIVE_END) - 1.4).abs() < 1e-5);
        assert!((swing_angle(ATTACK_END) - REST_ANGLE).abs() < 1e-5);
        let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
        attack.impact_angle = Some(0.0);
        attack.age = 0.10;
        assert!(attack.angle() < -0.5);
        attack.age = REBOUND_END;
        assert!(attack.finished());
        assert!((attack.angle() - REST_ANGLE).abs() < 1e-5);
    }
    #[test]
    fn spear_draws_back_accelerates_forward_and_recovers_from_contact() {
        assert!(spear_pose(0.10, false).0 < -0.2);
        assert!(spear_pose(0.20, false).0 > 0.5);
        assert!((spear_pose(0.24, false).0 - 1.05).abs() < 0.01);
        assert!((spear_pose(0.0, true).0 - 1.05).abs() < 0.01);
        assert!(spear_pose(0.25, true).0 < 0.3);
        assert!(spear_pose(0.58, false).0.abs() < 0.01);
    }

}

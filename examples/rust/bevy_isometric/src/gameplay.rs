use crate::{
    effects::{self, Feedback, FxAssets},
    impact::{self, ImpactTarget},
    navigation::{self, flat, Disc, EDGE, RADIUS},
    sound::{Cue, Sound},
};
use bevy::prelude::*;

pub const WALK_SPEED: f32 = 3.8;
pub const WINDUP: f32 = 0.24;
pub const ACTIVE_END: f32 = 0.38;
pub const ATTACK_END: f32 = 0.76;
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
    pub age: f32,
    pub aim: Vec3,
    pub impact_angle: Option<f32>,
    pub whooshed: bool,
}
impl Attack {
    pub fn new(aim: Vec3) -> Self {
        Self {
            age: 0.0,
            aim: flat(aim).normalize_or(Vec3::Z),
            impact_angle: None,
            whooshed: false,
        }
    }
    pub fn angle(&self) -> f32 {
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
            swing_angle(self.age)
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
            self.attack = Some(Attack::new(aim));
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

pub fn mouse_commands(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    mut controls: ResMut<Controls>,
    player: Single<&Transform, With<Player>>,
    obstacles: Query<&Obstacle>,
) {
    controls.repath -= time.delta_secs();
    let Some(point) = cursor_ground(camera.0, camera.1, &window) else {
        return;
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if buttons.just_pressed(MouseButton::Left) && shift {
        controls.strike(point - player.translation);
    } else if buttons.pressed(MouseButton::Right)
        && controls.attack.is_none()
        && controls.repath <= 0.0
    {
        controls.repath = 0.12;
        if controls
            .route
            .last()
            .is_none_or(|last| last.distance(point) > 0.25)
        {
            let discs: Vec<_> = obstacles.iter().map(|o| o.0).collect();
            let route = navigation::path(player.translation, point, &discs);
            if !route.is_empty() {
                controls.route = route;
            }
        }
    }
}

fn approach(current: Vec3, target: Vec3, max_change: f32) -> Vec3 {
    current + (target - current).clamp_length_max(max_change)
}

pub fn slide(mut position: Vec3, delta: Vec3, obstacles: &[Disc]) -> Vec3 {
    let steps = (delta.length() / 0.06).ceil().max(1.0) as usize;
    for _ in 0..steps {
        let previous = position;
        position += delta / steps as f32;
        position.x = position.x.clamp(-EDGE, EDGE);
        position.z = position.z.clamp(-EDGE, EDGE);
        for _ in 0..3 {
            for disc in obstacles {
                let separation = flat(position - disc.center);
                let clearance = disc.radius + RADIUS + 0.02;
                if separation.length_squared() < clearance * clearance {
                    position +=
                        separation.normalize_or(Vec3::X) * (clearance - separation.length());
                }
            }
        }
        // A prop near the perimeter must not push the body off the deck.
        if position.x.abs() > EDGE || position.z.abs() > EDGE {
            position = previous;
        }
    }
    position
}

pub fn move_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    obstacles: Query<&Obstacle>,
    feedback: Res<Feedback>,
    sound: Res<Sound>,
) {
    let dt = time.delta_secs().min(0.1);
    if keys.just_pressed(KeyCode::KeyR) {
        player.translation = Vec3::new(0.0, 0.0, 2.0);
        player.rotation = Quat::from_rotation_y(std::f32::consts::PI);
        *controls = Controls::default();
        return;
    }
    if feedback.hit_stop > 0.0 {
        return;
    }
    if let Some(attack) = controls.attack {
        controls.velocity = approach(controls.velocity, Vec3::ZERO, dt * 35.0);
        if attack.impact_angle.is_none() && attack.age < WINDUP {
            let target = Quat::from_rotation_y(attack.aim.x.atan2(attack.aim.z));
            player.rotation = player.rotation.slerp(target, 1.0 - (-28.0 * dt).exp());
        }
        return;
    }
    let pressed = |a, b| {
        if keys.pressed(a) || keys.pressed(b) {
            1.0
        } else {
            0.0
        }
    };
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = Vec3::new(-1.0, 0.0, -1.0).normalize();
    let input = Vec2::new(
        pressed(KeyCode::KeyD, KeyCode::ArrowRight) - pressed(KeyCode::KeyA, KeyCode::ArrowLeft),
        pressed(KeyCode::KeyW, KeyCode::ArrowUp) - pressed(KeyCode::KeyS, KeyCode::ArrowDown),
    )
    .normalize_or_zero();
    let speed = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        5.6
    } else {
        WALK_SPEED
    };
    let mut desired = (right * input.x + up * input.y) * speed;
    if input != Vec2::ZERO {
        controls.route.clear();
    } else if !controls.route.is_empty() {
        while controls.route.len() > 1
            && flat(controls.route[0] - player.translation).length() < 0.18
        {
            controls.route.remove(0);
        }
        let delta = flat(controls.route[0] - player.translation);
        let braking_speed = if controls.route.len() == 1 {
            (2.0 * 28.0 * delta.length()).sqrt()
        } else {
            speed
        };
        desired = delta.normalize_or_zero() * speed.min(braking_speed);
        if delta.length() < 0.055 {
            player.translation = controls.route[0];
            controls.velocity = Vec3::ZERO;
            controls.route.clear();
            desired = Vec3::ZERO;
        }
    }
    controls.velocity = approach(
        controls.velocity,
        desired,
        dt * if desired == Vec3::ZERO { 32.0 } else { 26.0 },
    );
    let discs: Vec<_> = obstacles.iter().map(|o| o.0).collect();
    let previous = player.translation;
    let mut delta = controls.velocity * dt;
    if controls.route.len() == 1 {
        let remaining = flat(controls.route[0] - previous);
        if delta.length() > remaining.length() {
            delta = remaining;
            controls.velocity = Vec3::ZERO;
        }
    }
    player.translation = slide(previous, delta, &discs);
    let actual = flat(player.translation - previous);
    controls.stride += actual.length() * 8.5;
    controls.foot_distance += actual.length();
    if controls.foot_distance > 0.72 {
        controls.foot_distance -= 0.72;
        sound.play(Cue::Step);
    }
    if actual.length_squared() > 1e-6 {
        let target = Quat::from_rotation_y(actual.x.atan2(actual.z));
        player.rotation = player.rotation.slerp(target, 1.0 - (-20.0 * dt).exp());
    }
}

/// Earliest contact along a swept blade, sampled in angle space (not frame time).
/// This prevents a fast cut from tunneling through a rock at low frame rates.
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
        let pivot = origin + facing * Vec3::new(0.38, 0.9, 0.0);
        let blade = facing * Quat::from_rotation_y(angle) * Vec3::Z;
        let (base, tip) = (pivot + blade * 0.25, pivot + blade * 1.95);
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
            let nearest = flat(disc.center - base).dot(blade).clamp(0.0, 1.7);
            return Some((index, angle, base + blade * nearest));
        }
    }
    None
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
    let previous_age = attack.age;
    attack.age += dt;
    if attack.impact_angle.is_none() && attack.age >= WINDUP && previous_age < ACTIVE_END {
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
            let pivot = player.translation + player.rotation * Vec3::new(0.38, 0.9, 0.0);
            let blade = player.rotation * Quat::from_rotation_y(angle) * Vec3::Z;
            let (base, tip) = (pivot + blade * 0.25, pivot + blade * 1.95);
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
                } else if let Some((_, _, position)) =
                    swept_hit(player.translation, player.rotation, angle, angle, &[disc])
                {
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
            let base = player.translation;
            let body_discs: Vec<_> = obstacles.iter().map(|(o, _)| o.0).collect();
            player.translation = slide(base, -attack.aim * 0.075, &body_discs);
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
    mut parts: Query<(&Part, &mut Transform), Without<Player>>,
) {
    if feedback.hit_stop > 0.0
        && !controls
            .attack
            .is_some_and(|a| a.impact_angle.is_some() && a.age == 0.0)
    {
        return;
    }
    let speed = (controls.velocity.length() / WALK_SPEED).min(1.4);
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    for (part, mut transform) in &mut parts {
        *transform = part.rest;
        match part.kind {
            PartKind::Body => {
                transform.translation.y += breathe + controls.stride.sin().abs() * 0.035 * speed;
                transform.rotation = Quat::from_rotation_x(0.06 * speed);
                if let Some(attack) = controls.attack {
                    let twist = if attack.impact_angle.is_some() {
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
                    Quat::from_rotation_x(controls.stride.sin() * 0.55 * speed * side);
                if controls.attack.is_some() {
                    transform.rotation = Quat::from_rotation_x(-0.12 * side);
                }
            }
            PartKind::Sword => {
                let angle = controls.attack.map_or(REST_ANGLE, |a| a.angle());
                let tilt = controls.attack.map_or(-0.6, |a| {
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
                transform.translation.y += breathe;
            }
        }
    }
}

pub fn draw_navigation(
    controls: Res<Controls>,
    player: Single<&Transform, With<Player>>,
    mut gizmos: Gizmos,
    time: Res<Time>,
) {
    if let Some(goal) = controls.route.last() {
        let color = Color::srgba(0.25, 0.95, 1.0, 0.7);
        let y = Vec3::Y * 0.035;
        let r = 0.24 + 0.025 * (time.elapsed_secs() * 7.0).sin();
        gizmos.circle(
            Isometry3d::new(
                *goal + y,
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
            r,
            color,
        );
        let mut previous = player.translation + y;
        for point in &controls.route {
            let end = *point + y;
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
                (move_player, combat, effects::update_effects).chain(),
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
        assert!(app.world().get::<Transform>(player).unwrap().translation.z < 0.0);
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
        let mut attack = Attack::new(Vec3::Z);
        attack.impact_angle = Some(0.0);
        attack.age = 0.10;
        assert!(attack.angle() < -0.5);
        attack.age = REBOUND_END;
        assert!(attack.finished());
        assert!((attack.angle() - REST_ANGLE).abs() < 1e-5);
    }
    #[test]
    fn movement_slides_without_tunneling_and_stops_smoothly() {
        let rock = Disc {
            center: Vec3::ZERO,
            radius: 0.8,
        };
        let p = slide(Vec3::new(-3.0, 0.0, 0.0), Vec3::X * 5.0, &[rock]);
        assert!(p.x < -1.1);
        assert!(p.distance(rock.center) >= rock.radius + RADIUS);
        let p = slide(Vec3::new(-1.2, 0.0, 0.0), Vec3::new(0.5, 0.0, 1.0), &[rock]);
        assert!(p.z > 0.5);
        let edge_rock = Disc {
            center: Vec3::new(7.7, 0.0, 0.0),
            radius: 0.72,
        };
        let edge = slide(Vec3::new(7.8, 0.0, 2.0), -Vec3::Z * 2.0, &[edge_rock]);
        assert!(edge.x.abs() <= EDGE && edge.z.abs() <= EDGE);
        assert!(edge.distance(edge_rock.center) >= edge_rock.radius + RADIUS);
        let stopped = approach(Vec3::X * 0.1, Vec3::ZERO, 0.2);
        assert_eq!(stopped, Vec3::ZERO);
    }
}

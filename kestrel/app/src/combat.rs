//! Authoritative attack timing, swept collision and impact response.
use crate::{
    character::Player,
    effects::{self, Feedback, FxAssets},
    gameplay::Controls,
    impact::{self, ImpactTarget},
    navigation::{self, flat, Disc},
    sound::{Cue, Sound},
};
use bevy::prelude::*;

pub const WINDUP: f32 = 0.16;
pub const ACTIVE_END: f32 = 0.29;
pub const ATTACK_END: f32 = 0.58;
pub const REBOUND_END: f32 = 0.42;
pub const REST_ANGLE: f32 = 0.45;

/// Visual action family. Combat remains authoritative over hit timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionMotion {
    Sweep,
    Thrust,
    Draw,
    Cast,
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
    pub fn motion(&self) -> ActionMotion {
        match self.weapon {
            content::scenario::Weapon::Sword => ActionMotion::Sweep,
            content::scenario::Weapon::Spear => ActionMotion::Thrust,
            content::scenario::Weapon::Bow => ActionMotion::Draw,
            content::scenario::Weapon::ArcCoil => ActionMotion::Cast,
        }
    }
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

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
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

#[derive(Component, Clone, Copy)]
pub struct Obstacle(pub Disc);

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
            .add_systems(Update, (combat, effects::update_effects).chain());
        app.world_mut().spawn((Player, Transform::default()));
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

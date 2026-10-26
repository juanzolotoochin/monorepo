//! The local swing as the view plays it: predicted attack timing, swept blade
//! contact for rebound and hit-stop, and impact response. The simulation
//! decides hits and damage.
use crate::{
    character::Player,
    effects::{self, Feedback, FxAssets},
    gameplay::Controls,
    impact::{self, ImpactTarget},
    navigation::{self, flat, Disc},
    sound::{Cue, Sound},
};
use bevy::prelude::*;
use sim::settlement::avatar::SwordStroke;

use crate::lockstep::seconds;

// The active ends are view-only blade timing. The other swing phases come
// from the player character's definition, below.
pub const ACTIVE_END: f32 = 0.29;
pub const SWORD_ACTIVE_END: f32 = 0.22;
pub const REST_ANGLE: f32 = 0.45;

/// Every avatar's swing is posed with this character's timings, until
/// characters carry their own view (step 4 in `docs/character-definition.md`).
/// Its variants share them.
const TIMING_CHARACTER: &str = "exosuit";

/// The player character's swing phases in view seconds.
struct SwingSeconds {
    windup: f32,
    attack_end: f32,
    sword_windup: f32,
    sword_end: f32,
    rebound_end: f32,
    combo_window: f32,
}

fn swing_seconds() -> &'static SwingSeconds {
    static SWING: std::sync::LazyLock<SwingSeconds> = std::sync::LazyLock::new(|| {
        let content = crate::lockstep::builtin_content();
        let ix = content.names.character_index(TIMING_CHARACTER).expect("the timing character");
        let avatar = content.rules.characters[ix as usize].avatar.as_ref().expect("player characters have avatar tuning");
        let attack = avatar.attack;
        SwingSeconds {
            windup: seconds(attack.aim),
            attack_end: seconds(attack.swing),
            sword_windup: seconds(attack.sword_windup),
            sword_end: seconds(attack.sword_swing),
            rebound_end: seconds(attack.rebound),
            combo_window: seconds(attack.combo_window),
        }
    });
    &SWING
}

/// The visible windup, while the avatar turns toward the aim.
pub fn windup() -> f32 {
    swing_seconds().windup
}
/// A swing that hits nothing ends here.
pub fn attack_end() -> f32 {
    swing_seconds().attack_end
}
pub fn sword_windup() -> f32 {
    swing_seconds().sword_windup
}
pub fn sword_end() -> f32 {
    swing_seconds().sword_end
}
/// A swing that lands rebounds this long after contact.
pub fn rebound_end() -> f32 {
    swing_seconds().rebound_end
}
/// Another sword press within this continues the combo.
pub fn combo_window() -> f32 {
    swing_seconds().combo_window
}

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
    pub stroke: SwordStroke,
    pub swift: bool,
    /// The tick the local player's `Attack` command asked for; `None` for
    /// a swing that isn't the local player's press (Studio, remote players).
    pub asked: Option<u64>,
    /// The drawn avatar swung for it (`follow_drawn_swing`).
    pub bound: bool,
}
impl Attack {
    pub fn timing(&self) -> (f32, f32, f32) {
        if self.weapon == content::scenario::Weapon::Laser {
            (0.03, 0.07, 0.13)
        } else if self.weapon == content::scenario::Weapon::Sword && self.swift {
            (sword_windup(), SWORD_ACTIVE_END, sword_end())
        } else {
            (windup(), ACTIVE_END, attack_end())
        }
    }
    pub fn motion(&self) -> ActionMotion {
        match self.weapon {
            content::scenario::Weapon::Sword => ActionMotion::Sweep,
            content::scenario::Weapon::Spear => ActionMotion::Thrust,
            content::scenario::Weapon::Bow => ActionMotion::Draw,
            content::scenario::Weapon::ArcCoil => ActionMotion::Cast,
            content::scenario::Weapon::Laser => ActionMotion::Cast,
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
            stroke: SwordStroke::Forehand,
            swift: false,
            asked: None,
            bound: false,
        }
    }
    pub fn angle(&self) -> f32 {
        if self.weapon == content::scenario::Weapon::Spear {
            return 0.05;
        }
        if let Some(angle) = self.impact_angle {
            let recoil = if self.stroke == SwordStroke::Backhand {
                0.9
            } else {
                -0.9
            };
            if self.age < 0.15 {
                lerp(angle, angle + recoil, ease_out(self.age / 0.15))
            } else {
                lerp(
                    angle + recoil,
                    REST_ANGLE,
                    smooth((self.age - 0.15) / (rebound_end() - 0.15)),
                )
            }
        } else {
            match self.weapon {
                content::scenario::Weapon::Sword => {
                    swing_angle_stroke_timed(self.age, self.stroke, self.swift)
                }
                content::scenario::Weapon::Spear => -0.15 + 0.25 * smooth(self.age / 0.22),
                content::scenario::Weapon::Bow => -0.9 + 0.4 * smooth(self.age / 0.22),
                content::scenario::Weapon::ArcCoil => -0.5 + 0.8 * smooth(self.age / 0.22),
                content::scenario::Weapon::Laser => -0.15 + 0.25 * smooth(self.age / 0.22),
            }
        }
    }
    pub fn finished(&self) -> bool {
        self.age
            >= if self.impact_angle.is_some() {
                rebound_end()
            } else {
                self.timing().2
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
    swing_angle_stroke(age, SwordStroke::Forehand)
}

pub fn swing_angle_stroke(age: f32, stroke: SwordStroke) -> f32 {
    swing_angle_stroke_timed(age, stroke, true)
}

pub fn swing_angle_stroke_timed(age: f32, stroke: SwordStroke, swift: bool) -> f32 {
    let (windup_time, active_end, attack_end) = if swift {
        (sword_windup(), SWORD_ACTIVE_END, sword_end())
    } else {
        (windup(), ACTIVE_END, attack_end())
    };
    let (windup, cut) = match stroke {
        SwordStroke::Forehand => (-1.7, 1.4),
        SwordStroke::Backhand => (1.6, -1.4),
        SwordStroke::Overhead => (-0.62, 0.68),
    };
    if age < windup_time {
        lerp(REST_ANGLE, windup, smooth(age / windup_time))
    } else if age < active_end {
        lerp(
            windup,
            cut,
            ((age - windup_time) / (active_end - windup_time)).powf(if swift { 1.35 } else { 1.6 }),
        )
    } else {
        lerp(
            cut,
            REST_ANGLE,
            smooth((age - active_end) / (attack_end - active_end)),
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

/// How many ticks past the tick its command asked for a local swing may
/// wait for the drawn avatar to swing, or to hold the press for after its
/// current swing. The command applies on that tick, or the replay applies
/// it on the next one, so by then the drawn avatar shows it unless the
/// simulation refused the attack (digging, dead, a weapon the scene doesn't
/// offer).
pub const UNSEEN_TICKS: f32 = 2.0;

/// Keeps the local player's swing to the drawn avatar's (D5 in
/// `ABILITY_PREDICTION_HANDOFF.md`). A press swings at once, before its
/// command applies. Once the drawn avatar swings for it, the swing takes
/// that swing's stroke and the combo follows the avatar's. If the drawn
/// avatar shows no swing or waiting press for it by `UNSEEN_TICKS` past
/// the tick it asked for, the simulation refused it, and it ends.
pub fn follow_drawn_swing(controls: &mut Controls, drawn: &crate::prediction::DrawnState) {
    let Some(attack) = controls.attack.as_mut() else { return };
    let Some(asked) = attack.asked.filter(|_| !attack.bound) else { return };
    let avatar = drawn.avatar();
    if let Some(swing) = avatar.swing.as_ref().filter(|swing| swing.pressed() >= asked) {
        attack.bound = true;
        attack.stroke = swing.stroke;
        controls.combo_next = avatar.combo_next;
        controls.combo_timer = (avatar.combo_until as f32 + 1.0 - drawn.time).max(0.0) / content::TICKS_PER_SECOND as f32;
        return;
    }
    let waiting = avatar.queued_at.is_some_and(|pressed| pressed >= asked);
    if !waiting && drawn.time >= asked as f32 + UNSEEN_TICKS {
        controls.attack = None;
        if let Some(aim) = controls.queued.take() {
            let asked = controls.queued_asked.take();
            controls.strike_at(aim, asked);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn combat(
    mut commands: Commands,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    drawn: Option<Res<crate::prediction::DrawnAvatar>>,
    mut player: Single<&mut Transform, With<Player>>,
    mut obstacles: Query<(&Obstacle, Option<&mut ImpactTarget>)>,
    mut feedback: ResMut<Feedback>,
    assets: Res<FxAssets>,
    sound: Res<Sound>,
) {
    let dt = time.delta_secs().min(0.1);
    controls.combo_timer = (controls.combo_timer - dt).max(0.0);
    if let Some(drawn) = drawn.as_ref().and_then(|drawn| drawn.0.as_ref()) {
        follow_drawn_swing(&mut controls, drawn);
    }
    if feedback.hit_stop > 0.0 {
        return;
    }
    let Some(mut attack) = controls.attack else {
        return;
    };
    if attack.age < 0.05 && attack.weapon != content::scenario::Weapon::Laser {
        player.rotation = Quat::from_rotation_y(attack.aim.x.atan2(attack.aim.z));
    }
    let previous_age = attack.age;
    attack.age += dt;
    if attack.weapon == content::scenario::Weapon::Sword
        && attack.impact_angle.is_none()
        && attack.age >= attack.timing().0
        && previous_age < attack.timing().1
    {
        if !attack.whooshed {
            sound.play(Cue::Swing);
            attack.whooshed = true;
        }
        let from = swing_angle_stroke_timed(
            previous_age.max(attack.timing().0),
            attack.stroke,
            attack.swift,
        );
        let to = swing_angle_stroke_timed(
            attack.age.min(attack.timing().1),
            attack.stroke,
            attack.swift,
        );
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
        }
    }
    if attack.finished() {
        controls.attack = None;
        if let Some(aim) = controls.queued.take() {
            let asked = controls.queued_asked.take();
            controls.strike_at(aim, asked);
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
            laser: default(),
            blood: std::array::from_fn(|_| default()),
        };
        app.init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .init_resource::<Feedback>()
            .init_resource::<effects::CoverContacts>()
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
                        laser: default(),
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
        assert!((swing_angle(sword_windup()) + 1.7).abs() < 1e-5);
        assert!((swing_angle(SWORD_ACTIVE_END) - 1.4).abs() < 1e-5);
        assert!((swing_angle(sword_end()) - REST_ANGLE).abs() < 1e-5);
        let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
        attack.impact_angle = Some(0.0);
        attack.age = 0.10;
        assert!(attack.angle() < -0.5);
        attack.age = rebound_end();
        assert!(attack.finished());
        assert!((attack.angle() - REST_ANGLE).abs() < 1e-5);
    }

    #[test]
    fn sword_strokes_have_distinct_sweeps_and_share_recovery() {
        let forehand = swing_angle_stroke(SWORD_ACTIVE_END, SwordStroke::Forehand);
        let backhand = swing_angle_stroke(SWORD_ACTIVE_END, SwordStroke::Backhand);
        let overhead = swing_angle_stroke(SWORD_ACTIVE_END, SwordStroke::Overhead);
        assert!(forehand > 1.0 && backhand < -1.0 && overhead > 0.0);
        for stroke in [
            SwordStroke::Forehand,
            SwordStroke::Backhand,
            SwordStroke::Overhead,
        ] {
            assert!((swing_angle_stroke(0.0, stroke) - REST_ANGLE).abs() < 1e-5);
            assert!((swing_angle_stroke(sword_end(), stroke) - REST_ANGLE).abs() < 1e-5);
        }
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

    #[test]
    fn the_local_swing_follows_the_drawn_avatar() {
        use content::scenario::Weapon;
        use fx::{Fx, FxVec2};
        use sim::settlement::avatar::Swing;
        let setup = sim::settlement::SceneSetup { player: "exosuit".into(), extent: Fx::from_int(8), ..default() };
        let world = sim::World::new_scene(1, crate::lockstep::builtin_content(), &setup).unwrap();
        let settlement = world.settlement().unwrap();
        let agent = settlement.player().unwrap();
        let state = settlement.agents[agent as usize].clone();
        // The drawn avatar at `time` (ticks), swinging `swing`, holding a
        // press from `queued_at` for after it.
        let drawn = |time: f32, swing: Option<Swing>, queued_at: Option<u64>| {
            let mut to = state.clone();
            to.avatar.swing = swing;
            to.avatar.queued_at = queued_at;
            to.avatar.combo_next = SwordStroke::Overhead;
            to.avatar.combo_until = 140;
            crate::prediction::DrawnState { agent, from: to.clone(), to, time }
        };
        let swing = |started: u64, stroke| Swing {
            weapon: Weapon::Sword,
            stroke,
            aim: FxVec2::new(Fx::ONE, Fx::ZERO),
            muzzle: [Fx::ZERO; 3],
            started,
            resolve_at: started + 13,
            until: started + 35,
            resolved: false,
            hits: 0,
            queued_at: None,
        };

        // A press whose command asked for tick 100 swings at once, with the
        // stroke the view predicts.
        let mut controls = Controls::default();
        controls.strike_at(Vec3::Z, Some(100));
        assert_eq!(controls.attack.unwrap().stroke, SwordStroke::Forehand);
        // Until the drawn time reaches it, an older swing doesn't count.
        follow_drawn_swing(&mut controls, &drawn(99.0, Some(swing(60, SwordStroke::Forehand)), None));
        assert!(controls.attack.is_some_and(|a| !a.bound));
        // The drawn avatar swings a backhand for it: so does the view, and
        // the combo goes on as the avatar's.
        follow_drawn_swing(&mut controls, &drawn(100.5, Some(swing(100, SwordStroke::Backhand)), None));
        let attack = controls.attack.unwrap();
        assert!(attack.bound);
        assert_eq!(attack.stroke, SwordStroke::Backhand);
        assert_eq!(controls.combo_next, SwordStroke::Overhead);
        assert!((controls.combo_timer - 40.5 / 60.0).abs() < 1e-5, "{}", controls.combo_timer);
        // Once matched, it plays out on its own clock.
        follow_drawn_swing(&mut controls, &drawn(140.0, None, None));
        assert!(controls.attack.is_some());

        // A press the drawn avatar shows no swing for, while another press
        // is queued behind it.
        let mut controls = Controls::default();
        controls.strike_at(Vec3::Z, Some(200));
        controls.strike_at(Vec3::X, Some(201));
        follow_drawn_swing(&mut controls, &drawn(201.5, Some(swing(150, SwordStroke::Forehand)), None));
        assert!(controls.attack.is_some_and(|a| a.asked == Some(200)), "it may still apply");
        // Past `UNSEEN_TICKS`, the simulation refused it: it ends, and the
        // queued press swings in its place.
        follow_drawn_swing(&mut controls, &drawn(200.0 + UNSEEN_TICKS, Some(swing(150, SwordStroke::Forehand)), None));
        let attack = controls.attack.unwrap();
        assert_eq!((attack.asked, attack.aim), (Some(201), Vec3::X));
        assert!(controls.queued.is_none());
        // A press the drawn avatar holds for after its current swing waits.
        follow_drawn_swing(&mut controls, &drawn(230.0, Some(swing(190, SwordStroke::Forehand)), Some(201)));
        assert!(controls.attack.is_some_and(|a| a.asked == Some(201) && !a.bound));

        // A swing that isn't the local player's press is left alone.
        let mut controls = Controls::default();
        controls.strike(Vec3::Z);
        follow_drawn_swing(&mut controls, &drawn(500.0, None, None));
        assert!(controls.attack.is_some());
    }
}

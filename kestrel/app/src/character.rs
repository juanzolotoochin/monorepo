//! Render-only character rig and continuous posing shared by local and peer avatars.
use crate::{
    assets::SceneLibrary,
    combat::{
        lerp, smooth, spear_pose, ActionMotion, Attack, ACTIVE_END, ATTACK_END, REBOUND_END,
        REST_ANGLE, WINDUP,
    },
    effects::Feedback,
    gameplay::{Controls, WALK_SPEED},
};
use bevy::prelude::*;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct Part {
    pub kind: PartKind,
    pub rest: Transform,
}
#[derive(Clone, Copy)]
pub enum PartKind {
    Body,
    Chest,
    Head,
    Arm(f32),
    Forearm(f32),
    Hand(f32),
    Leg(f32),
    Shin(f32),
    WeaponGrip,
}

/// Render-only inertia. Simulation position and hit timing stay authoritative.
#[derive(Default)]
pub struct MotionState {
    speed: f32,
    turn: f32,
    heading: Option<f32>,
}

impl MotionState {
    pub fn advance(&mut self, velocity: Vec3, heading: f32, dt: f32) -> (f32, f32) {
        let dt = dt.clamp(0.0, 0.05);
        let target = (velocity.length() / WALK_SPEED).min(1.5);
        let response = if target > self.speed { 18.0 } else { 11.0 };
        self.speed += (target - self.speed) * (1.0 - (-response * dt).exp());
        let delta = self.heading.map_or(0.0, |previous| {
            (heading - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI
        });
        self.heading = Some(heading);
        let target_turn = (delta / dt.max(1e-4) * 0.12).clamp(-0.65, 0.65);
        self.turn += (target_turn - self.turn) * (1.0 - (-14.0 * dt).exp());
        (self.speed, self.turn)
    }
}

pub fn animate_character(
    time: Res<Time>,
    controls: Res<Controls>,
    feedback: Res<Feedback>,
    flight: Option<Res<crate::jetpack::Flight>>,
    game: Res<crate::lockstep::Game>,
    library: Res<SceneLibrary>,
    player: Single<&Transform, With<Player>>,
    mut parts: Query<(&Part, &mut Transform), Without<Player>>,
    mut motion: Local<MotionState>,
) {
    if feedback.hit_stop > 0.0
        && !controls
            .attack
            .is_some_and(|a| a.impact_angle.is_some() && a.age == 0.0)
    {
        return;
    }
    let airborne = flight.as_ref().is_some_and(|f| f.airborne);
    let (speed, turn) = motion.advance(
        if airborne {
            Vec3::ZERO
        } else {
            controls.velocity
        },
        player.rotation.to_euler(EulerRot::YXZ).0,
        time.delta_secs(),
    );
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    let dig = game
        .player()
        .zip(game.current.as_ref())
        .and_then(|(i, s)| s.agents[i as usize].avatar.dig.as_ref())
        .map(|d| dig_age(&game, d));
    let pose = Pose {
        stride: controls.stride,
        speed,
        turn,
        breathe,
        airborne,
        attack: controls.attack,
        dig,
        motion: library.scenes[library.current]
            .scene
            .style
            .as_ref()
            .map_or_else(voxel_assets::CharacterMotion::default, |s| {
                s.character_motion
            }),
    };
    for (part, mut transform) in &mut parts {
        blend_pose(
            &mut transform,
            pose.place(part.kind, part.rest),
            time.delta_secs(),
            pose.motion.pose_response,
        );
    }
}

/// Interpolate joint transforms in render time, independently of simulation timing.
pub fn blend_pose(current: &mut Transform, target: Transform, dt: f32, response: f32) {
    let weight = 1.0 - (-response.max(0.0) * dt.clamp(0.0, 0.05)).exp();
    current.translation = current.translation.lerp(target.translation, weight);
    current.rotation = current.rotation.slerp(target.rotation, weight);
    current.scale = target.scale;
}

/// What a character is doing this frame, for posing its parts.
#[derive(Clone, Copy)]
pub struct Pose {
    pub motion: voxel_assets::CharacterMotion,
    /// Walk cycle phase.
    pub stride: f32,
    /// Walking speed relative to `WALK_SPEED`.
    pub speed: f32,
    /// Smoothed angular response in radians, including a short follow-through.
    pub turn: f32,
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
pub fn dig_age(game: &crate::lockstep::Game, dig: &sim::settlement::avatar::Dig) -> f32 {
    let now = game.tick as f32 - 1.0 + game.alpha;
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
        let action = self.place_unblended(kind, rest);
        let Some(attack) = self.attack else {
            return action;
        };
        if matches!(kind, PartKind::WeaponGrip) || self.dig.is_some() {
            return action;
        }
        let mut locomotion = *self;
        locomotion.attack = None;
        let base = locomotion.place_unblended(kind, rest);
        let weight = smooth(attack.age / (WINDUP * 0.65))
            * (1.0 - smooth((attack.age - ACTIVE_END) / (ATTACK_END - ACTIVE_END)));
        Transform {
            translation: base.translation.lerp(action.translation, weight),
            rotation: base.rotation.slerp(action.rotation, weight),
            scale: base.scale,
        }
    }

    fn place_unblended(&self, kind: PartKind, rest: Transform) -> Transform {
        if let Some(age) = self.dig {
            return self.digging(kind, rest, age);
        }
        let mut transform = rest;
        let travel = self.speed.clamp(0.0, 1.4);
        let running = ((travel - 0.75) / 0.65).clamp(0.0, 1.0);
        match kind {
            PartKind::Body => {
                transform.translation.y +=
                    self.breathe + (self.stride * 2.0).cos().abs() * 0.045 * travel;
                transform.translation.z -= 0.055 * travel;
                transform.translation.x += self.turn * 0.055;
                transform.rotation = Quat::from_rotation_y(-self.turn * 0.28)
                    * Quat::from_rotation_z(-self.turn * 0.22)
                    * Quat::from_rotation_x(0.05 * travel + 0.13 * running);
                if let Some(attack) = self.attack {
                    let twist = if attack.motion() == ActionMotion::Thrust {
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
            PartKind::Chest => {
                transform.rotation = Quat::from_rotation_y(self.turn * self.motion.torso_turn)
                    * Quat::from_rotation_z(self.turn * 0.18)
                    * Quat::from_rotation_x(-0.03 * travel + self.stride.sin() * 0.025 * travel);
                transform.translation.x -= self.turn * 0.025;
                if let Some(attack) = self.attack {
                    let anticipation = smooth(attack.age / WINDUP);
                    let release =
                        1.0 - smooth((attack.age - ACTIVE_END) / (ATTACK_END - ACTIVE_END));
                    let pitch = match attack.motion() {
                        ActionMotion::Thrust => -0.18 * anticipation * release,
                        ActionMotion::Sweep => 0.13 * anticipation * release,
                        ActionMotion::Draw | ActionMotion::Cast => -0.08 * anticipation * release,
                    };
                    transform.rotation *= Quat::from_rotation_x(pitch);
                }
            }
            PartKind::Head => {
                transform.translation.y += self.breathe * 0.6;
                transform.rotation = Quat::from_rotation_y(-self.turn * 0.34)
                    * Quat::from_rotation_z(-self.turn * 0.08)
                    * Quat::from_rotation_x(-0.04 * travel);
            }
            PartKind::Arm(side) => {
                let swing = self.stride.sin() * side * travel;
                transform.rotation = Quat::from_rotation_z(-0.08 * side + self.turn * 0.12)
                    * Quat::from_rotation_x(-swing * self.motion.arm_swing - 0.10 * travel);
                if let Some(attack) = self.attack {
                    let follow = if attack.motion() == ActionMotion::Thrust {
                        spear_pose(attack.age, attack.impact_angle.is_some()).0
                    } else {
                        attack.angle() - REST_ANGLE
                    };
                    if side > 0.0 {
                        let yaw = if attack.motion() == ActionMotion::Thrust {
                            0.0
                        } else {
                            follow * 0.65
                        };
                        transform.rotation = Quat::from_rotation_y(yaw)
                            * Quat::from_rotation_x(-0.18 - follow.abs() * 0.16);
                        transform.translation.z += follow.max(0.0) * 0.12;
                    } else {
                        transform.rotation *= Quat::from_rotation_x(-0.25 + follow * 0.10);
                    }
                }
            }
            PartKind::Forearm(side) => {
                let phase = (self.stride.sin() * side).max(0.0);
                let bend = 0.36 + phase * 0.16 * travel + 0.05 * running;
                transform.rotation = Quat::from_rotation_x(-bend);
                if let Some(attack) = self.attack {
                    let progress = smooth(attack.age / WINDUP);
                    transform.rotation = Quat::from_rotation_x(if side > 0.0 {
                        -0.52 - 0.36 * progress
                    } else {
                        -0.48
                    });
                }
            }
            PartKind::Hand(side) => {
                transform.rotation = Quat::from_rotation_x(if side > 0.0 { 0.24 } else { 0.08 });
                if let Some(attack) = self.attack {
                    transform.rotation = Quat::from_rotation_x(if side > 0.0 {
                        0.18 + 0.12 * smooth(attack.age / WINDUP)
                    } else {
                        -0.12
                    });
                }
            }
            PartKind::Leg(side) => {
                let phase = self.stride.sin() * side;
                transform.rotation = Quat::from_rotation_x(
                    phase * (self.motion.leg_swing + 0.10 * running) * travel,
                ) * Quat::from_rotation_y(-self.turn * (0.28 + 0.12 * side))
                    * Quat::from_rotation_z(-self.turn * 0.12);
                transform.translation.y += phase.max(0.0) * 0.045 * travel;
                transform.translation.x += side * self.turn.abs() * 0.045;
                if self.airborne {
                    transform.rotation = Quat::from_rotation_x(-0.25);
                } else if self.attack.is_some() {
                    transform.rotation =
                        Quat::from_rotation_x(-0.18 * side) * Quat::from_rotation_z(0.08 * side);
                }
            }
            PartKind::Shin(side) => {
                let swing = (self.stride.sin() * side).max(0.0);
                transform.rotation =
                    Quat::from_rotation_x(-(0.09 + swing * (0.42 + 0.20 * running) * travel));
                if self.airborne {
                    transform.rotation = Quat::from_rotation_x(-0.55);
                } else if self.attack.is_some() {
                    transform.rotation = Quat::from_rotation_x(-0.18);
                }
            }
            PartKind::WeaponGrip => {
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
                // The blade is a child of the right hand. Shoulder and torso
                // carry most of the cut; the wrist supplies the remaining yaw.
                let wrist_yaw = if self
                    .attack
                    .is_some_and(|a| a.motion() != ActionMotion::Thrust)
                {
                    REST_ANGLE + (angle - REST_ANGLE) * 0.23
                } else {
                    angle
                };
                transform.rotation =
                    Quat::from_rotation_y(wrist_yaw) * Quat::from_rotation_x(tilt + 0.35);
                if let Some(attack) = self.attack {
                    if attack.motion() == ActionMotion::Thrust {
                        let (extension, pitch) =
                            spear_pose(attack.age, attack.impact_angle.is_some());
                        transform.translation.z += extension;
                        transform.translation.y -= extension.max(0.0) * 0.04;
                        transform.rotation = Quat::from_rotation_x(pitch + 0.35);
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
            PartKind::Chest => {
                transform.rotation = Quat::from_rotation_x(0.13 + 0.1 * down);
            }
            PartKind::Head => {
                transform.rotation = Quat::from_rotation_x(-0.18 - 0.1 * down);
            }
            PartKind::Arm(side) => {
                transform.rotation =
                    Quat::from_rotation_x(if side > 0.0 { pitch * 0.48 } else { -0.55 });
            }
            PartKind::Forearm(side) => {
                transform.rotation = Quat::from_rotation_x(if side > 0.0 {
                    -0.55 - 0.18 * down
                } else {
                    -0.38
                });
            }
            PartKind::Hand(side) => {
                transform.rotation = Quat::from_rotation_x(if side > 0.0 { 0.16 } else { 0.0 });
            }
            PartKind::Leg(side) => {
                transform.rotation =
                    Quat::from_rotation_x(-0.3 * side) * Quat::from_rotation_z(0.12 * side);
            }
            PartKind::Shin(_) => {
                transform.rotation = Quat::from_rotation_x(-0.28);
            }
            PartKind::WeaponGrip => {
                transform.translation.z += 0.18;
                transform.translation.y -= 0.04 * down;
                transform.rotation = Quat::from_rotation_x(pitch + 0.35);
            }
        }
        transform
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::Attack;

    #[test]
    fn motion_blends_quickly_and_turns_across_the_angle_wrap() {
        let mut motion = MotionState::default();
        motion.advance(Vec3::ZERO, std::f32::consts::PI - 0.04, 1.0 / 60.0);
        let (speed, turn) = motion.advance(
            Vec3::Z * WALK_SPEED,
            -std::f32::consts::PI + 0.04,
            1.0 / 60.0,
        );
        assert!(speed > 0.2 && speed < 1.0);
        assert!(turn > 0.0 && turn < 0.5);
        for _ in 0..30 {
            motion.advance(Vec3::ZERO, -std::f32::consts::PI + 0.04, 1.0 / 60.0);
        }
        assert!(motion.speed < 0.01);
        assert!(motion.turn.abs() < 0.01);
    }

    #[test]
    fn articulated_run_and_attack_keep_symmetric_rest_pivots() {
        let rest = Transform::from_xyz(0.42, 1.27, 0.0);
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion::default(),
            stride: std::f32::consts::FRAC_PI_2,
            speed: 1.2,
            turn: 0.25,
            breathe: 0.0,
            airborne: false,
            attack: None,
            dig: None,
        };
        let left = pose.place(PartKind::Arm(-1.0), rest);
        let right = pose.place(PartKind::Arm(1.0), rest);
        let elbow_rest = Transform::from_xyz(0.0, -0.29, 0.015);
        let idle_elbow = pose.place(PartKind::Forearm(1.0), elbow_rest);
        assert_ne!(left.rotation, right.rotation);
        assert_eq!(left.translation, rest.translation);
        let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
        attack.age = WINDUP;
        pose.attack = Some(attack);
        let attacking = pose.place(PartKind::Arm(1.0), rest);
        assert_ne!(attacking.rotation, right.rotation);
        let attack_elbow = pose.place(PartKind::Forearm(1.0), elbow_rest);
        assert_ne!(attack_elbow.rotation, idle_elbow.rotation);
        let knee = pose.place(PartKind::Shin(1.0), Transform::default());
        assert_ne!(knee.rotation, Quat::IDENTITY);
    }

    #[test]
    fn action_layer_enters_and_leaves_locomotion_continuously() {
        let rest = Transform::from_xyz(0.0, 0.94, 0.0);
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion::default(),
            stride: 0.6,
            speed: 0.8,
            turn: 0.2,
            breathe: 0.0,
            airborne: false,
            attack: None,
            dig: None,
        };
        let walking = pose.place(PartKind::Body, rest);
        let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
        pose.attack = Some(attack);
        let beginning = pose.place(PartKind::Body, rest);
        assert!(beginning.rotation.angle_between(walking.rotation) < 1e-5);
        attack.age = WINDUP;
        pose.attack = Some(attack);
        let engaged = pose.place(PartKind::Body, rest);
        assert!(engaged.rotation.angle_between(walking.rotation) > 0.05);
        attack.age = ATTACK_END;
        pose.attack = Some(attack);
        let ending = pose.place(PartKind::Body, rest);
        assert!(ending.rotation.angle_between(walking.rotation) < 1e-5);
    }

    #[test]
    fn hand_held_blade_follows_the_swept_attack_direction() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion::default(),
            stride: 0.0,
            speed: 0.0,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            attack: None,
            dig: None,
        };
        for age in [0.0, WINDUP, 0.22, ACTIVE_END, ATTACK_END] {
            let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
            attack.age = age;
            pose.attack = Some(attack);
            let joints = [
                (PartKind::Body, Transform::from_xyz(0.0, 0.94, 0.0)),
                (PartKind::Arm(1.0), Transform::from_xyz(0.43, 0.33, 0.0)),
                (
                    PartKind::Forearm(1.0),
                    Transform::from_xyz(0.0, -0.29, 0.015),
                ),
                (PartKind::Hand(1.0), Transform::from_xyz(0.0, -0.28, 0.015)),
                (PartKind::WeaponGrip, Transform::from_xyz(-0.04, 0.08, 0.05)),
            ];
            let blade = joints.iter().fold(Mat4::IDENTITY, |parent, (kind, rest)| {
                parent * pose.place(*kind, *rest).to_matrix()
            });
            let forward = blade.transform_vector3(Vec3::Z).with_y(0.0).normalize();
            let visual = forward.x.atan2(forward.z);
            let error = (visual - attack.angle() + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            assert!(error.abs() < 0.35, "blade diverged at {age}: {error}");
        }
    }
}

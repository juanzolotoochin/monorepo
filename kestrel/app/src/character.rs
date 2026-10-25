//! Render-only character rig and continuous posing shared by local and peer avatars.
use crate::{
    assets::SceneLibrary,
    combat::{
        lerp, smooth, spear_pose, ActionMotion, Attack, ACTIVE_END, ATTACK_END, REBOUND_END,
        REST_ANGLE, WINDUP,
    },
    effects::Feedback,
    gameplay::Controls,
};
use bevy::prelude::*;
use std::collections::HashMap;
use sim::settlement::avatar::{JumpPhase, SwordStroke, JUMP_LAND_TICKS, JUMP_PREP_TICKS};

#[derive(Component)]
pub struct Player;
#[derive(Component)]
pub struct LocalBlade;

#[derive(Component)]
pub struct Part {
    pub kind: PartKind,
    /// Original local bind transform, restored when a style omits a pivot.
    pub bind: Transform,
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
    Knee(f32),
    Shin(f32),
    Foot(f32),
    WeaponGrip,
}

/// Distance-driven gait phase and contacts, shared by local and peer avatars.
#[derive(Clone, Copy, Default)]
pub struct GaitClock {
    pub phase: f32,
    contact_distance: f32,
}

pub(crate) fn step_distance_for_speed(base: f32, relative_speed: f32) -> f32 {
    let running = ((relative_speed - 1.02) / 0.38).clamp(0.0, 1.0);
    base * (1.0 + 0.20 * running)
}

impl GaitClock {
    pub fn advance(&mut self, traveled: f32, step_distance: f32, grounded: bool) -> u32 {
        let step = step_distance.clamp(0.2, 4.0);
        let traveled = traveled.max(0.0);
        self.phase =
            (self.phase + traveled * std::f32::consts::PI / step).rem_euclid(std::f32::consts::TAU);
        if !grounded {
            self.contact_distance = 0.0;
            return 0;
        }
        self.contact_distance += traveled;
        let contacts = (self.contact_distance / step).floor() as u32;
        self.contact_distance -= contacts as f32 * step;
        contacts
    }
}

/// Render-only inertia. Simulation position and hit timing stay authoritative.
#[derive(Default)]
pub struct MotionState {
    speed: f32,
    turn: f32,
    visual_heading: Option<f32>,
    angular_speed: f32,
}

impl MotionState {
    pub fn advance(&mut self, velocity: Vec3, heading: f32, dt: f32) -> (f32, f32) {
        self.advance_with_profile(
            velocity,
            heading,
            dt,
            voxel_assets::CharacterMotion::default(),
        )
    }

    pub fn advance_with_profile(
        &mut self,
        velocity: Vec3,
        heading: f32,
        dt: f32,
        profile: voxel_assets::CharacterMotion,
    ) -> (f32, f32) {
        let dt = dt.clamp(0.0, 0.05);
        let target = (velocity.length() / profile.walk_speed.max(0.1)).min(1.5);
        let response = if target > self.speed { 18.0 } else { 11.0 };
        self.speed += (target - self.speed) * (1.0 - (-response * dt).exp());
        let visual = self.visual_heading.get_or_insert(heading);
        let error = (heading - *visual + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let frequency = profile.turn_inertia.clamp(2.0, 30.0);
        // A damped angular spring keeps the body leaning through a change of
        // heading, even after the simulation's root has already turned.
        self.angular_speed +=
            (error * frequency * frequency - self.angular_speed * frequency * 1.35) * dt;
        *visual += self.angular_speed * dt;
        self.turn = (error * (0.7 + 0.3 * self.speed)).clamp(-0.9, 0.9);
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
    viewer: Res<crate::viewer::Viewer>,
    player: Single<(Entity, &Transform), With<Player>>,
    mut parts: Query<(Entity, &Part, &mut Transform, Option<&ChildOf>), Without<Player>>,
    mut motion: Local<MotionState>,
) {
    if !viewer.active && feedback.hit_stop > 0.0
        && !controls
            .attack
            .is_some_and(|a| a.impact_angle.is_some() && a.age == 0.0)
    {
        return;
    }
    let studio_jump = viewer.active.then(|| viewer.jump().0).flatten();
    let airborne = if viewer.active {
        studio_jump.is_some()
    } else {
        flight.as_ref().is_some_and(|f| f.airborne)
    };
    let profile = library.scenes[library.current]
        .scene
        .style
        .as_ref()
        .map_or_else(voxel_assets::CharacterMotion::default, |s| {
            s.character_motion
        });
    let (speed, turn) = if viewer.active {
        let velocity = if viewer.clip == crate::viewer::Clip::Run {
            profile.run_speed
        } else {
            controls.velocity.length()
        };
        ((velocity / profile.walk_speed.max(0.1)).min(1.5), viewer.turn_lean())
    } else {
        motion.advance_with_profile(
            if airborne { Vec3::ZERO } else { controls.velocity },
            player.1.rotation.to_euler(EulerRot::YXZ).0,
            time.delta_secs(),
            profile,
        )
    };
    let breathe = if viewer.active {
        if viewer.clip == crate::viewer::Clip::Idle {
            (viewer.frame as f32 * std::f32::consts::TAU
                / viewer.clip.frame_count() as f32)
                .sin()
                * 0.012
        } else {
            0.0
        }
    } else {
        (time.elapsed_secs() * 2.0).sin() * 0.012
    };
    let dig = game
        .player()
        .zip(game.current.as_ref())
        .and_then(|(i, s)| s.agents[i as usize].avatar.dig.as_ref())
        .map(|d| dig_age(&game, d));
    let mut pose = Pose {
        stride: controls.gait.phase,
        speed,
        turn,
        breathe,
        airborne,
        jump: jump_motion(
            flight.as_ref().map_or(JumpPhase::Grounded, |f| f.jump),
            game.tick as f32 - 1.0 + game.alpha,
            player.1.translation.y,
            flight.as_ref().map_or(0.0, |f| f.support),
        ),
        attack: controls.attack,
        dig,
        motion: profile,
    };
    if viewer.active {
        pose.jump = studio_jump;
        pose.airborne = airborne;
        pose.attack = controls.attack;
        pose.dig = None;
    }
    let mut joints = HashMap::new();
    for (entity, part, mut transform, parent) in &mut parts {
        let target = pose.place(part.kind, part.rest);
        if viewer.active || pose.motion.plant_feet
            && matches!(part.kind, PartKind::Body)
            && pose.speed > 0.6
            && pose.jump.is_none()
            && pose.attack.is_none()
            && pose.dig.is_none()
        {
            *transform = target;
        } else {
            blend_pose(
                &mut transform,
                target,
                time.delta_secs(),
                pose.motion.pose_response,
            );
        }
        joints.insert(
            entity,
            (part.kind, part.rest, *transform, parent.map(ChildOf::parent)),
        );
    }
    // For rigs that opt into planted feet, solve the complete leg after body
    // smoothing so a planted ankle stays fixed against the
    // traveling ground instead of lagging behind the hip animation.
    if pose.motion.plant_feet
        && pose.jump.is_none()
        && !pose.airborne
        && pose.attack.is_none()
        && pose.dig.is_none()
        && (!viewer.active
            || matches!(viewer.clip, crate::viewer::Clip::Walk | crate::viewer::Clip::Run))
    {
        lock_grounded_feet(&mut parts, &joints, player.0, &pose);
    }
}

type JointSnapshot = (PartKind, Transform, Transform, Option<Entity>);

fn lock_grounded_feet(
    parts: &mut Query<(Entity, &Part, &mut Transform, Option<&ChildOf>), Without<Player>>,
    joints: &HashMap<Entity, JointSnapshot>,
    player: Entity,
    pose: &Pose,
) {
    let Some((&body_entity, &(_, _, body, _))) = joints
        .iter()
        .find(|(_, (kind, _, _, parent))| matches!(kind, PartKind::Body) && *parent == Some(player))
    else {
        return;
    };
    let running = ((pose.speed - 1.02) / 0.38).clamp(0.0, 1.0);
    let stance = stance_fraction(running, true);
    let step = step_distance_for_speed(pose.motion.step_distance, pose.speed).clamp(0.2, 4.0);
    let weight = smooth(pose.speed / 0.6);
    for (&foot_entity, &(kind, foot_rest, _, shin_entity)) in joints {
        if !matches!(kind, PartKind::Foot(_)) {
            continue;
        }
        let Some(shin_entity) = shin_entity else {
            continue;
        };
        let Some(&(PartKind::Shin(side), shin_rest, _, Some(leg_entity))) = joints.get(&shin_entity)
        else {
            continue;
        };
        let Some(&(PartKind::Leg(leg_side), leg_rest, _, Some(parent))) = joints.get(&leg_entity)
        else {
            continue;
        };
        if parent != body_entity || side != leg_side {
            continue;
        }
        let phase = (pose.stride + if side > 0.0 { 0.0 } else { std::f32::consts::PI })
            .rem_euclid(std::f32::consts::TAU);
        let (offset, lift) = foot_path(phase, step, stance, pose.motion.step_lift * (1.0 + 1.2 * running));
        let foot_origin = body_rest_position(joints[&body_entity].1, leg_rest, shin_rest, foot_rest);
        let target = foot_origin + Vec3::new(0.0, lift, offset);
        let local_target =
            body.rotation.inverse() * (target - body.translation) - leg_rest.translation;
        let upper = shin_rest.translation.length();
        let lower = foot_rest.translation.length();
        let (thigh_angle, knee_angle) = solve_leg(local_target.y, local_target.z, upper, lower);
        if let Ok((_, _, mut leg, _)) = parts.get_mut(leg_entity) {
            leg.translation = leg.translation.lerp(leg_rest.translation, weight);
            leg.rotation = leg.rotation.slerp(Quat::from_rotation_x(thigh_angle), weight);
        }
        if let Ok((_, _, mut shin, _)) = parts.get_mut(shin_entity) {
            shin.translation = shin.translation.lerp(shin_rest.translation, weight);
            shin.rotation = shin.rotation.slerp(Quat::from_rotation_x(knee_angle), weight);
        }
        if let Ok((_, _, mut foot, _)) = parts.get_mut(foot_entity) {
            foot.translation = foot.translation.lerp(foot_rest.translation, weight);
            foot.rotation = foot.rotation.slerp(
                (body.rotation * Quat::from_rotation_x(thigh_angle + knee_angle)).inverse(),
                weight,
            );
        }
    }
}

fn body_rest_position(body: Transform, leg: Transform, shin: Transform, foot: Transform) -> Vec3 {
    body.translation + leg.translation + shin.translation + foot.translation
}

fn stance_fraction(running: f32, natural: bool) -> f32 {
    if natural {
        0.55 - 0.21 * running
    } else {
        0.66 - 0.11 * running
    }
}

fn support_pose(stride: f32, step: f32, running: f32, reach: f32) -> (f32, f32) {
    // Stand tall as the support foot passes under the hip, then lower just
    // enough to reach it near touchdown and toe-off. Reach comes from the
    // authored character motion profile.
    let stance = stance_fraction(running, true);
    let mut foot_offsets = [0.0; 2];
    let mut weights = [0.0; 2];
    for (index, phase) in [stride, stride + std::f32::consts::PI].into_iter().enumerate() {
        let phase = phase.rem_euclid(std::f32::consts::TAU);
        let swing_progress = (phase / std::f32::consts::TAU - stance) / (1.0 - stance);
        weights[index] = if phase < std::f32::consts::TAU * stance {
            1.0
        } else {
            smooth((swing_progress - 0.65) / 0.30)
        };
        // A foot nearing touchdown already needs leg reach. Fade its support
        // in near landing so the hip does not drop at the loop seam.
        foot_offsets[index] = foot_path(phase, step, stance, 0.0).0;
    }
    // When both feet can support weight, center the hip between them. This
    // preserves leg reach without forcing a deep knee bend before touchdown.
    let center = (foot_offsets[0] + foot_offsets[1]) * 0.5 * weights[0] * weights[1];
    let offset = foot_offsets
        .into_iter()
        .zip(weights)
        .map(|(foot, weight)| (foot - center).abs() * weight)
        .fold(0.0_f32, f32::max);
    // Reserve a little reach for torso pitch and breathing.
    let offset = (offset + 0.03).min(reach - 0.01);
    (reach - (reach * reach - offset * offset).sqrt() + 0.008, center)
}

fn foot_path(phase: f32, step: f32, stance: f32, height: f32) -> (f32, f32) {
    let span = step * stance;
    if phase < std::f32::consts::TAU * stance {
        (span - phase * step / std::f32::consts::PI, 0.0)
    } else {
        let progress = (phase / std::f32::consts::TAU - stance) / (1.0 - stance);
        (
            -span + 2.0 * span * smooth(progress),
            height * (progress * std::f32::consts::PI).sin(),
        )
    }
}

fn solve_leg(y: f32, z: f32, upper: f32, lower: f32) -> (f32, f32) {
    let reach = Vec2::new(-y, -z);
    let length = reach
        .length()
        .clamp((upper - lower).abs() + 0.001, upper + lower - 0.001);
    let direction = reach.y.atan2(reach.x);
    let opening = ((length * length + upper * upper - lower * lower) / (2.0 * length * upper))
        .clamp(-1.0, 1.0)
        .acos();
    let knee = ((length * length - upper * upper - lower * lower) / (2.0 * upper * lower))
        .clamp(-1.0, 1.0)
        .acos();
    (direction - opening, knee)
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
    /// Walking speed relative to the scene's authored walk speed.
    pub speed: f32,
    /// Smoothed angular response in radians, including a short follow-through.
    pub turn: f32,
    pub breathe: f32,
    pub airborne: bool,
    pub jump: Option<JumpMotion>,
    pub attack: Option<Attack>,
    /// Seconds into a dig.
    pub dig: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub enum JumpMotion {
    Preparing(f32),
    Pushing(f32),
    Rising(f32),
    Falling(f32),
    Landing(f32),
}

pub fn jump_motion(phase: JumpPhase, ticks: f32, height: f32, support: f32) -> Option<JumpMotion> {
    match phase {
        JumpPhase::Grounded => None,
        JumpPhase::Preparing { started } => {
            let progress = ((ticks - started as f32) / JUMP_PREP_TICKS as f32).clamp(0.0, 1.0);
            if progress < 0.5 {
                Some(JumpMotion::Preparing(smooth(progress * 2.0)))
            } else {
                Some(JumpMotion::Pushing(smooth((progress - 0.5) * 2.0)))
            }
        }
        JumpPhase::Airborne { velocity, .. } if velocity > fx::Fx::ZERO => {
            let speed = velocity.to_f32() * content::TICKS_PER_SECOND as f32;
            Some(JumpMotion::Rising((1.0 - speed / 4.0).clamp(0.0, 1.0)))
        }
        JumpPhase::Airborne { .. } => Some(JumpMotion::Falling(
            (1.0 - (height - support) / 0.6).clamp(0.0, 1.0),
        )),
        JumpPhase::Landing { started } => Some(JumpMotion::Landing(smooth(
            (ticks - started as f32) / JUMP_LAND_TICKS as f32,
        ))),
    }
}

/// A coordinated crouch, leg extension, shallow flight flex and landing overlay.
/// The root height remains simulation owned; these offsets only pose joints.
fn jump_pose(kind: PartKind, mut transform: Transform, phase: JumpMotion) -> Transform {
    let (compress, extension, float, reach, push) = match phase {
        JumpMotion::Preparing(p) => (p, 0.0, 0.0, 0.0, 0.0),
        JumpMotion::Pushing(p) => (1.0 - p, p, 0.0, 0.0, p),
        JumpMotion::Rising(p) => (0.0, 1.0 - smooth(p), smooth(p), 0.0, 1.0 - smooth(p)),
        JumpMotion::Falling(p) => (0.0, 0.0, 1.0 - smooth(p), smooth(p), 0.0),
        JumpMotion::Landing(p) => (1.0 - p, 0.0, 0.0, 0.0, 0.0),
    };
    match kind {
        PartKind::Body => {
            transform.translation.y += -0.16 * compress + 0.06 * push;
            transform.translation.z += 0.06 * float + 0.09 * reach;
            transform.rotation *=
                Quat::from_rotation_x(0.30 * compress + 0.12 * float + 0.22 * reach);
        }
        PartKind::Chest => {
            transform.rotation *=
                Quat::from_rotation_x(0.15 * compress + 0.08 * float + 0.13 * reach);
        }
        PartKind::Head => {
            transform.rotation *= Quat::from_rotation_x(-0.13 * compress + 0.08 * reach);
        }
        PartKind::Arm(side) => {
            transform.rotation *= Quat::from_rotation_x(
                0.30 * compress
                    - 0.28 * extension
                    - (0.40 + 0.08 * side) * float
                    - (0.35 - 0.08 * side) * reach,
            ) * Quat::from_rotation_z(side * 0.22 * reach);
        }
        PartKind::Forearm(_) => {
            transform.rotation *= Quat::from_rotation_x(-0.15 * compress - 0.12 * float);
        }
        PartKind::Leg(side) => {
            transform.translation.y += 0.05 * push;
            transform.rotation *= Quat::from_rotation_x(
                -0.34 * compress + 0.10 * extension - (0.10 + 0.19 * side) * float - 0.18 * reach,
            );
        }
        PartKind::Knee(_) => {}
        PartKind::Shin(side) => {
            transform.rotation *= Quat::from_rotation_x(
                0.58 * compress - 0.08 * extension + (0.35 + 0.08 * side) * float + 0.18 * reach,
            );
        }
        PartKind::Foot(side) => {
            transform.rotation *= Quat::from_rotation_x(
                0.12 * compress + 0.42 * push + (0.08 - 0.06 * side) * float + 0.06 * reach,
            );
        }
        PartKind::Hand(_) | PartKind::WeaponGrip => {}
    }
    transform
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

/// A stride follows stance, toe-off, swing, then landing. Adjacent joints
/// sample the same phase so their motion expresses one transfer of weight.
struct Gait {
    drive: f32,
    lift: f32,
    plant: f32,
}

impl Gait {
    fn sample(stride: f32, side: f32, running: f32, natural: bool) -> Self {
        let phase = (stride
            + if side > 0.0 {
                0.0
            } else {
                std::f32::consts::PI
            }).rem_euclid(std::f32::consts::TAU);
        let stance = std::f32::consts::TAU * stance_fraction(running, natural);
        let (drive, lift, plant) = if phase < stance {
            let progress = phase / stance;
            let contact = smooth(progress / 0.08) * smooth((1.0 - progress) / 0.08);
            (-((progress * std::f32::consts::PI).cos()), 0.0, contact)
        } else {
            let progress = (phase - stance) / (std::f32::consts::TAU - stance);
            ((progress * std::f32::consts::PI).cos(), (progress * std::f32::consts::PI).sin(), 0.0)
        };
        Self { drive, lift, plant }
    }
}

fn knee_angle(gait: &Gait, travel: f32, running: f32) -> f32 {
    // The swing shin folds behind the thigh; stance straightens for contact.
    0.06 + gait.lift * (0.42 + 0.10 * running) * travel - gait.plant * 0.06 * running
}

fn arm_drive(stride: f32, side: f32) -> f32 {
    // Shift the swing's center toward the back without changing its smooth
    // extrema. The elbow stays behind the torso longer on each run cycle.
    let phase = stride + if side > 0.0 { 0.0 } else { std::f32::consts::PI };
    let cosine = -phase.cos();
    (cosine - 0.2) / (1.0 - 0.2 * cosine)
}

impl Pose {
    /// A part's rest transform, posed.
    pub fn place(&self, kind: PartKind, rest: Transform) -> Transform {
        let action = self.place_unblended(kind, rest);
        let posed = if let Some(attack) = self
            .attack
            .filter(|_| !matches!(kind, PartKind::WeaponGrip) && self.dig.is_none())
        {
            let mut locomotion = *self;
            locomotion.attack = None;
            let base = locomotion.place_unblended(kind, rest);
            let (windup, active_end, end) = attack.timing();
            let weight = smooth(attack.age / (windup * 0.65))
                * (1.0 - smooth((attack.age - active_end) / (end - active_end)));
            Transform {
                translation: base.translation.lerp(action.translation, weight),
                rotation: base.rotation.slerp(action.rotation, weight),
                scale: base.scale,
            }
        } else {
            action
        };
        self.jump
            .map_or(posed, |phase| jump_pose(kind, posed, phase))
    }

    fn place_unblended(&self, kind: PartKind, rest: Transform) -> Transform {
        if let Some(age) = self.dig {
            return self.digging(kind, rest, age);
        }
        let mut transform = rest;
        let (windup, active_end, attack_end) = self
            .attack
            .map_or((WINDUP, ACTIVE_END, ATTACK_END), |a| a.timing());
        let travel = if matches!(self.jump, Some(JumpMotion::Rising(_) | JumpMotion::Falling(_))) {
            0.0
        } else {
            self.speed.clamp(0.0, 1.4)
        };
        let running = ((travel - 1.02) / 0.38).clamp(0.0, 1.0);
        let natural = self.motion.motion_style == voxel_assets::MotionStyle::Natural;
        let left = Gait::sample(self.stride, -1.0, running, natural);
        let right = Gait::sample(self.stride, 1.0, running, natural);
        let shoulder_twist = if natural {
            -arm_drive(self.stride, 1.0) * running * travel.min(1.0)
        } else {
            0.0
        };
        match kind {
            PartKind::Body => {
                if natural {
                    let (crouch, center) = support_pose(
                        self.stride,
                        step_distance_for_speed(self.motion.step_distance, self.speed),
                        running,
                        self.motion.leg_reach,
                    );
                    transform.translation.y += self.breathe * 0.3
                        - crouch * travel.min(1.0);
                    transform.translation.z += center * travel.min(1.0);
                } else {
                    transform.translation.y += self.breathe
                        + (self.stride * 2.0).cos().abs() * 0.033 * travel
                        - self.motion.run_crouch * running;
                }
                transform.translation.z -= (if natural { 0.005 } else { 0.06 }) * travel;
                transform.translation.x += self.turn * 0.09
                    + if natural {
                        0.0
                    } else {
                        (right.plant - left.plant) * 0.035 * travel
                    };
                transform.rotation = Quat::from_rotation_y(-self.turn * 0.28)
                    * Quat::from_rotation_z(-self.turn * (0.24 + 0.25 * travel))
                    * Quat::from_rotation_x(if natural { 0.01 * travel + 0.04 * running } else { 0.06 * travel + 0.18 * running });
                if let Some(attack) = self.attack {
                    if attack.motion() == ActionMotion::Sweep {
                        let drive = smooth((attack.age - windup) / (active_end - windup));
                        let recover =
                            1.0 - smooth((attack.age - active_end) / (attack_end - active_end));
                        transform.translation.z += 0.14 * drive * recover;
                        transform.rotation *= Quat::from_rotation_x(0.16 * drive * recover);
                    }
                    if attack.weapon == content::scenario::Weapon::Sword
                        && attack.stroke == SwordStroke::Overhead
                    {
                        let drive = smooth((attack.age - windup) / (active_end - windup));
                        transform.translation.z += 0.18 * drive;
                        transform.rotation *= Quat::from_rotation_x(0.18 * drive);
                    }
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
                transform.rotation = Quat::from_rotation_y(
                    self.turn * self.motion.torso_turn + shoulder_twist * 0.12,
                )
                    * Quat::from_rotation_z(self.turn * 0.18 + shoulder_twist * 0.018)
                    * Quat::from_rotation_x(
                        -0.02 * travel + 0.08 * running + self.stride.sin() * 0.025 * travel,
                    );
                if !natural {
                    transform.rotation *= Quat::from_rotation_y(-self.stride.sin() * 0.06 * travel);
                }
                transform.translation.x -= self.turn * 0.025;
                if let Some(attack) = self.attack {
                    let anticipation = smooth(attack.age / windup);
                    let release =
                        1.0 - smooth((attack.age - active_end) / (attack_end - active_end));
                    let pitch = match attack.motion() {
                        ActionMotion::Thrust => -0.18 * anticipation * release,
                        ActionMotion::Sweep => match attack.stroke {
                            SwordStroke::Forehand => 0.13 * anticipation * release,
                            SwordStroke::Backhand => -0.12 * anticipation * release,
                            SwordStroke::Overhead => 0.34 * anticipation * release,
                        },
                        ActionMotion::Draw | ActionMotion::Cast => -0.08 * anticipation * release,
                    };
                    transform.rotation *= Quat::from_rotation_x(pitch);
                }
            }
            PartKind::Head => {
                transform.translation.y += self.breathe * 0.6 - 0.06 * running;
                transform.rotation = Quat::from_rotation_y(-self.turn * 0.34 - shoulder_twist * 0.045)
                    * Quat::from_rotation_z(-self.turn * 0.08 - shoulder_twist * 0.01)
                    * Quat::from_rotation_x(-0.04 * travel - 0.09 * running);
                if let Some(attack) = self.attack.filter(|a| a.motion() == ActionMotion::Sweep) {
                    transform.rotation *=
                        Quat::from_rotation_y(-(attack.angle() - REST_ANGLE) * 0.10);
                }
            }
            PartKind::Arm(side) => {
                let gait = if side > 0.0 { &right } else { &left };
                let swing = if natural {
                    lerp(gait.drive, arm_drive(self.stride, side), running) * travel
                } else {
                    gait.drive * travel
                };
                let arm_swing = self.motion.arm_swing
                    * if natural {
                        (1.0 + 0.9 * running)
                            * (1.0 + 0.18 * running * smooth((-swing).clamp(0.0, 1.0)))
                    } else {
                        1.0
                    };
                transform.rotation = Quat::from_rotation_z(-0.08 * side + self.turn * 0.12)
                    * Quat::from_rotation_x(-swing * arm_swing - 0.08 * travel);
                transform.translation.z += swing
                    * (0.06 + if natural { 0.02 * running * smooth((-swing).clamp(0.0, 1.0)) } else { 0.0 });
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
                        if attack.motion() == ActionMotion::Sweep
                            && attack.stroke == SwordStroke::Overhead
                        {
                            transform.rotation *=
                                Quat::from_rotation_x(-0.48 * smooth(attack.age / windup));
                        }
                        transform.translation.z += follow.max(0.0) * 0.12;
                    } else {
                        transform.rotation *= Quat::from_rotation_x(-0.25 + follow * 0.10);
                    }
                }
            }
            PartKind::Forearm(side) => {
                let gait = if side > 0.0 { &right } else { &left };
                let elbow_cycle = if natural {
                    self.stride.sin().powi(2)
                } else {
                    gait.plant
                };
                let bend = 0.35
                    + elbow_cycle * self.motion.elbow_drive * travel
                    + gait.lift * 0.10 * travel
                    + 0.13 * running;
                transform.rotation = Quat::from_rotation_x(-bend);
                transform.translation.z += gait.drive * 0.03 * travel;
                if let Some(attack) = self.attack {
                    let progress = smooth(attack.age / windup);
                    transform.rotation = Quat::from_rotation_x(if side > 0.0 {
                        -0.52
                            - 0.36 * progress
                            - if attack.motion() == ActionMotion::Sweep
                                && attack.stroke == SwordStroke::Overhead
                            {
                                0.24 * progress
                            } else {
                                0.0
                            }
                    } else {
                        -0.48
                    });
                }
            }
            PartKind::Hand(side) => {
                transform.rotation = Quat::from_rotation_x(if side > 0.0 { 0.24 } else { 0.08 });
                if let Some(attack) = self.attack {
                    transform.rotation = Quat::from_rotation_x(if side > 0.0 {
                        0.18 + 0.12 * smooth(attack.age / windup)
                    } else {
                        -0.12
                    });
                }
            }
            PartKind::Leg(side) => {
                let gait = if side > 0.0 { &right } else { &left };
                transform.rotation = Quat::from_rotation_x(
                    gait.drive * (self.motion.leg_swing + 0.10 * running) * travel,
                ) * Quat::from_rotation_y(-self.turn * (0.28 + 0.12 * side))
                    * Quat::from_rotation_z(-self.turn * 0.12);
                transform.translation.y += gait.lift * self.motion.step_lift * travel;
                transform.translation.x += side * self.turn.abs() * 0.045;
                if self.airborne && self.jump.is_none() {
                    transform.rotation = Quat::from_rotation_x(-0.25);
                } else if let Some(attack) = self.attack {
                    let drive = smooth((attack.age - windup) / (active_end - windup));
                    let recover =
                        1.0 - smooth((attack.age - active_end) / (attack_end - active_end));
                    let lead = if attack.stroke == SwordStroke::Backhand {
                        -1.0
                    } else {
                        1.0
                    };
                    transform.translation.z +=
                        if side == lead { 0.08 } else { -0.04 } * drive * recover;
                    transform.rotation =
                        Quat::from_rotation_x(-0.18 * side - 0.10 * drive * recover)
                            * Quat::from_rotation_z(0.08 * side);
                }
            }
            PartKind::Knee(_) => {}
            PartKind::Shin(side) => {
                let gait = if side > 0.0 { &right } else { &left };
                transform.rotation = Quat::from_rotation_x(knee_angle(gait, travel, running));
                if self.airborne && self.jump.is_none() {
                    transform.rotation = Quat::from_rotation_x(-0.55);
                } else if self.attack.is_some() {
                    transform.rotation = Quat::from_rotation_x(-0.18);
                }
            }
            PartKind::Foot(side) => {
                let gait = if side > 0.0 { &right } else { &left };
                let thigh = gait.drive * (self.motion.leg_swing + 0.10 * running) * travel;
                let knee = knee_angle(gait, travel, running);
                // Counter-rotate against the hip and knee while planted.
                let planted = -(thigh + knee) + gait.plant * 0.13 * travel;
                let swinging = -gait.lift * 0.24 * travel;
                transform.rotation =
                    Quat::from_rotation_x(planted * gait.plant + swinging * (1.0 - gait.plant));
                if self.airborne && self.jump.is_none() {
                    transform.rotation = Quat::from_rotation_x(-0.22);
                }
            }
            PartKind::WeaponGrip => {
                let angle = self.attack.map_or(REST_ANGLE, |a| a.angle());
                // The run swing carries the wrist through a downward-forward
                // blade angle behind the body and an upward angle in front.
                let carry = if natural {
                    lerp(0.45, -0.10, running)
                } else {
                    -0.6 + running * 0.35
                };
                let tilt = self.attack.map_or(carry, |a| {
                    if a.motion() == ActionMotion::Sweep && a.stroke == SwordStroke::Overhead {
                        if a.age < windup {
                            return lerp(carry, -1.05, smooth(a.age / windup));
                        }
                        if a.age < active_end {
                            return lerp(
                                -1.05,
                                0.38,
                                smooth((a.age - windup) / (active_end - windup)),
                            );
                        }
                        return lerp(
                            0.38,
                            carry,
                            smooth((a.age - active_end) / (attack_end - active_end)),
                        );
                    }
                    if a.impact_angle.is_some() {
                        -0.1 - 0.5 * smooth(a.age / REBOUND_END)
                    } else if a.age < windup {
                        lerp(carry, -0.08, smooth(a.age / windup))
                    } else if a.age < active_end {
                        -0.08
                    } else {
                        lerp(
                            -0.08,
                            carry,
                            smooth((a.age - active_end) / (attack_end - active_end)),
                        )
                    }
                });
                // The blade is a child of the right hand. Shoulder and torso
                // carry most of the cut; the wrist supplies the remaining yaw.
                let carry_yaw = if natural { 0.35 } else { REST_ANGLE };
                let wrist_yaw = if self
                    .attack
                    .is_some_and(|a| a.motion() != ActionMotion::Thrust)
                {
                    carry_yaw + (angle - REST_ANGLE) * 0.23
                } else {
                    carry_yaw + angle - REST_ANGLE
                };
                let back_carry = if natural {
                    0.10 * running * smooth((-arm_drive(self.stride, 1.0)).clamp(0.0, 1.0))
                } else {
                    0.0
                };
                transform.rotation = Quat::from_rotation_y(wrist_yaw)
                    * Quat::from_rotation_x(tilt + 0.35 - back_carry);
                if natural {
                    // Set the hilt into the palm; the generic rig's high mount
                    // made this smaller character carry the sword by its guard.
                    transform.translation.y -= 0.02;
                    transform.translation.z -= 0.01;
                }
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
            PartKind::Knee(_) => {}
            PartKind::Shin(_) => {
                transform.rotation = Quat::from_rotation_x(-0.28);
            }
            PartKind::Foot(_) => {
                transform.rotation = Quat::from_rotation_x(0.14);
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
    fn moonlit_torso_stays_centered_through_walk_and_run() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion {
                step_distance: 0.44,
                motion_style: voxel_assets::MotionStyle::Natural,
                plant_feet: true,
                ..default()
            },
            stride: 0.0,
            speed: 1.0,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let body_rest = Transform::from_xyz(0.0, 0.94, 0.0);
        let chest_rest = Transform::from_xyz(0.0, 0.34, 0.0);
        for speed in [1.0, 1.4] {
            pose.speed = speed;
            for index in 0..=60 {
                pose.stride = std::f32::consts::TAU * index as f32 / 60.0;
                let body = pose.place(PartKind::Body, body_rest);
                let chest = pose.place(PartKind::Chest, chest_rest);
                assert!(body.translation.x.abs() < 1e-5, "body drift at phase {}", pose.stride);
                assert!(chest.translation.x.abs() < 1e-5, "chest drift at phase {}", pose.stride);
                assert!(body.rotation.y.abs() < 1e-5 && body.rotation.z.abs() < 1e-5);
                assert!(chest.rotation.y.abs() < 0.07 && chest.rotation.z.abs() < 0.02);
            }
        }
        pose.speed = 1.0;
        pose.stride = std::f32::consts::TAU * 32.0 / 33.0;
        let last_walk_height = pose.place(PartKind::Body, body_rest).translation.y;
        pose.stride = 0.0;
        let first_walk_height = pose.place(PartKind::Body, body_rest).translation.y;
        assert!(
            (first_walk_height - last_walk_height).abs() < 0.01,
            "walk loop body height changed from {last_walk_height} to {first_walk_height}"
        );
    }

    #[test]
    fn moonlit_walk_sword_elbow_moves_smoothly_through_every_frame() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion {
                step_distance: 0.44,
                motion_style: voxel_assets::MotionStyle::Natural,
                plant_feet: true,
                elbow_drive: 0.50,
                ..default()
            },
            stride: 0.0,
            speed: 1.0,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let rest = Transform::from_xyz(0.0, -0.29, 0.015);
        for frame in 0..33 {
            pose.stride = std::f32::consts::TAU * frame as f32 / 33.0;
            let current = pose.place(PartKind::Forearm(1.0), rest).rotation;
            pose.stride = std::f32::consts::TAU * ((frame + 1) % 33) as f32 / 33.0;
            let next = pose.place(PartKind::Forearm(1.0), rest).rotation;
            assert!(current.angle_between(next) < 0.15, "sword elbow jumped after walk frame {frame}");
        }
    }

    #[test]
    fn moonlit_run_sword_follows_the_arm_from_down_to_up() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion {
                step_distance: 0.44,
                motion_style: voxel_assets::MotionStyle::Natural,
                plant_feet: true,
                arm_swing: 0.43,
                ..default()
            },
            stride: 0.0,
            speed: 1.4,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let sample = |pose: &Pose| {
            let joints = [
                (PartKind::Body, Transform::from_xyz(0.0, 0.94, 0.0)),
                (PartKind::Chest, Transform::IDENTITY),
                (PartKind::Arm(1.0), Transform::from_xyz(0.43, 0.33, 0.0)),
                (PartKind::Forearm(1.0), Transform::from_xyz(0.0, -0.29, 0.015)),
                (PartKind::Hand(1.0), Transform::from_xyz(0.0, -0.28, 0.015)),
                (PartKind::WeaponGrip, Transform::from_xyz(-0.04, 0.08, 0.05)),
            ];
            let mut world = Mat4::IDENTITY;
            let mut elbow_z = 0.0;
            for (kind, rest) in joints {
                world *= pose.place(kind, rest).to_matrix();
                if matches!(kind, PartKind::Forearm(_)) {
                    elbow_z = world.transform_point3(Vec3::ZERO).z;
                }
            }
            (elbow_z, world.transform_vector3(Vec3::Z))
        };
        let (back_elbow, back_blade) = sample(&pose);
        pose.stride = std::f32::consts::PI;
        let (front_elbow, front_blade) = sample(&pose);
        assert!(back_elbow < -0.25, "elbow does not reach behind the torso");
        assert!(front_elbow > 0.2, "elbow does not reach in front of the torso");
        assert!(back_blade.y < -0.5 && back_blade.z > 0.2, "back blade {back_blade:?}");
        assert!(front_blade.y > 0.6 && front_blade.z > 0.2);

        let mut back_frames = 0;
        let mut front_frames = 0;
        let mut previous: Option<f32> = None;
        for frame in 0..60 {
            pose.stride = std::f32::consts::TAU * frame as f32 / 60.0;
            let (elbow, _) = sample(&pose);
            back_frames += usize::from(elbow < -0.08);
            front_frames += usize::from(elbow > 0.08);
            if let Some(last) = previous {
                assert!((elbow - last).abs() < 0.08, "arm jumped at frame {frame}");
            }
            previous = Some(elbow);
        }
        assert!(back_frames >= 24, "only {back_frames} frames place the elbow back");
        assert!(front_frames >= 24, "only {front_frames} frames place the elbow forward");
        assert!(back_frames > front_frames, "back {back_frames} frames, front {front_frames} frames");
    }

    #[test]
    fn moonlit_run_has_a_flight_phase_with_both_feet_clear() {
        let stance = stance_fraction(1.0, true);
        assert!(stance < 0.5);
        let phase = std::f32::consts::TAU * 0.45;
        for foot_phase in [phase, phase + std::f32::consts::PI] {
            let (_, lift) = foot_path(
                foot_phase.rem_euclid(std::f32::consts::TAU),
                0.44,
                stance,
                0.09 * 2.2,
            );
            assert!(lift > 0.04, "foot lift {lift}");
        }
    }

    #[test]
    fn planted_ankle_tracks_ground_through_the_full_walk_stance() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion {
                step_distance: 0.44,
                motion_style: voxel_assets::MotionStyle::Natural,
                plant_feet: true,
                ..default()
            },
            stride: 0.0,
            speed: 1.0,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let leg = Transform::from_xyz(0.14, -0.36, 0.0);
        let shin = Transform::from_xyz(0.0, -0.27, 0.0);
        let foot = Transform::from_xyz(0.0, -0.21, 0.0);
        let rest_body = Transform::from_xyz(0.0, 0.94, 0.0);
        let origin = body_rest_position(rest_body, leg, shin, foot);
        let mut stationary: Option<f32> = None;
        for index in 0..=20 {
            pose.stride = std::f32::consts::TAU * stance_fraction(0.0, true) * index as f32 / 20.0;
            let body = pose.place(PartKind::Body, rest_body);
            let (offset, lift) = foot_path(
                pose.stride,
                pose.motion.step_distance,
                stance_fraction(0.0, true),
                pose.motion.step_lift,
            );
            assert_eq!(lift, 0.0);
            let target = origin + Vec3::Z * offset;
            let local = body.rotation.inverse() * (target - body.translation) - leg.translation;
            let (thigh, knee) = solve_leg(local.y, local.z, 0.27, 0.21);
            assert!(knee < 0.8, "walk phase {} bends knee too far", pose.stride);
            let ankle = body.translation
                + body.rotation
                    * (leg.translation
                        + Quat::from_rotation_x(thigh)
                            * (shin.translation
                                + Quat::from_rotation_x(knee) * foot.translation));
            assert!((ankle.y - origin.y).abs() < 0.003);
            let travel = pose.stride * pose.motion.step_distance / std::f32::consts::PI;
            let ground_position = ankle.z + travel;
            if let Some(first) = stationary {
                assert!((ground_position - first).abs() < 0.003);
            } else {
                stationary = Some(ground_position);
            }
        }
    }

    #[test]
    fn moonlit_run_keeps_the_boot_above_the_floor() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion {
                step_distance: 0.44,
                motion_style: voxel_assets::MotionStyle::Natural,
                plant_feet: true,
                run_crouch: 0.08,
                ..default()
            },
            stride: 0.0,
            speed: 1.4,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let body_rest = Transform::from_xyz(0.0, 0.94, 0.0);
        let leg = Transform::from_xyz(0.14, -0.36, 0.0);
        let shin = Transform::from_xyz(0.0, -0.27, 0.0);
        let foot = Transform::from_xyz(0.0, -0.21, 0.0);
        let origin = body_rest_position(body_rest, leg, shin, foot);
        for index in 0..=60 {
            pose.stride = std::f32::consts::TAU * index as f32 / 60.0;
            let body = pose.place(PartKind::Body, body_rest);
            let (offset, lift) = foot_path(
                pose.stride,
                step_distance_for_speed(pose.motion.step_distance, pose.speed),
                stance_fraction(1.0, true),
                pose.motion.step_lift * 2.2,
            );
            let target = origin + Vec3::new(0.0, lift, offset);
            let local = body.rotation.inverse() * (target - body.translation) - leg.translation;
            let (thigh, knee) = solve_leg(local.y, local.z, 0.27, 0.21);
            let ankle = body.translation
                + body.rotation
                    * (leg.translation
                        + Quat::from_rotation_x(thigh)
                            * (shin.translation
                                + Quat::from_rotation_x(knee) * foot.translation));
            // The authored boot extends 0.0875 m below its ankle origin.
            assert!(ankle.y - 0.0875 >= -0.003, "run phase {} sank below floor", pose.stride);
        }
    }

    #[test]
    fn gait_phase_and_contacts_share_step_distance() {
        assert!((step_distance_for_speed(0.44, 1.4) - 0.528).abs() < 1e-6);
        let mut gait = GaitClock::default();
        assert_eq!(gait.advance(0.625, 1.25, true), 0);
        assert!((gait.phase - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(gait.advance(0.625, 1.25, true), 1);
        assert!((gait.phase - std::f32::consts::PI).abs() < 1e-5);
        assert_eq!(gait.advance(1.25, 1.25, false), 0);
        assert!(gait.phase < 1e-5);
    }

    #[test]
    fn motion_blends_quickly_and_turns_across_the_angle_wrap() {
        let mut motion = MotionState::default();
        motion.advance(Vec3::ZERO, std::f32::consts::PI - 0.04, 1.0 / 60.0);
        let (speed, turn) = motion.advance(
            Vec3::Z * crate::gameplay::WALK_SPEED,
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
    fn fast_turn_keeps_body_momentum_after_heading_stops() {
        let mut motion = MotionState::default();
        let frame = 1.0 / 60.0;
        motion.advance(Vec3::Z * 5.6, 0.0, frame);
        let (_, first) = motion.advance(Vec3::Z * 5.6, 0.9, frame);
        let (_, following) = motion.advance(Vec3::Z * 5.6, 0.9, frame);
        assert!(first > 0.4);
        assert!(following > 0.3);
        for _ in 0..60 {
            motion.advance(Vec3::Z * 5.6, 0.9, frame);
        }
        assert!(motion.turn.abs() < 0.05);
    }

    #[test]
    fn run_transfers_weight_through_torso_elbows_and_feet() {
        let rest_body = Transform::from_xyz(0.0, 0.94, 0.0);
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion::default(),
            stride: std::f32::consts::FRAC_PI_2,
            speed: 0.5,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let walking_y = pose.place(PartKind::Body, rest_body).translation.y;
        pose.speed = 1.4;
        let running_y = pose.place(PartKind::Body, rest_body).translation.y;
        assert!(running_y < walking_y - 0.08);
        let forearm_rest = Transform::from_xyz(0.0, -0.29, 0.015);
        let forward_elbow = pose.place(PartKind::Forearm(1.0), forearm_rest);
        let back_elbow = pose.place(PartKind::Forearm(-1.0), forearm_rest);
        assert!(forward_elbow.rotation.angle_between(back_elbow.rotation) > 0.3);
        let foot = pose.place(PartKind::Foot(1.0), Transform::default());
        assert!(foot.rotation.angle_between(Quat::IDENTITY) > 0.2);
        pose.stride = 3.0 * std::f32::consts::FRAC_PI_2;
        let lifted_knee = pose.place(PartKind::Shin(1.0), Transform::default());
        assert!(lifted_knee.rotation.to_euler(EulerRot::XYZ).0 > 0.4);
    }

    #[test]
    fn jump_extends_then_uses_a_shallow_airborne_knee_flex() {
        let mut pose = Pose {
            motion: voxel_assets::CharacterMotion::default(),
            stride: 0.0,
            speed: 0.0,
            turn: 0.0,
            breathe: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let body = Transform::from_xyz(0.0, 0.94, 0.0);
        let idle = pose.place(PartKind::Body, body);
        pose.jump = Some(JumpMotion::Preparing(1.0));
        assert!(pose.place(PartKind::Body, body).translation.y < idle.translation.y - 0.1);
        let crouch = pose.place(PartKind::Shin(1.0), Transform::default());
        pose.jump = Some(JumpMotion::Pushing(0.8));
        let pushing_foot = pose.place(PartKind::Foot(1.0), Transform::default());
        assert!(pushing_foot.rotation.to_euler(EulerRot::XYZ).0 > 0.3);
        pose.jump = Some(JumpMotion::Rising(0.0));
        let launch = pose.place(PartKind::Shin(1.0), Transform::default());
        pose.jump = Some(JumpMotion::Rising(1.0));
        let airborne = pose.place(PartKind::Shin(1.0), Transform::default());
        let leading = pose.place(PartKind::Leg(1.0), Transform::default());
        let trailing = pose.place(PartKind::Leg(-1.0), Transform::default());
        assert!(leading.rotation.angle_between(trailing.rotation) > 0.30);
        pose.jump = Some(JumpMotion::Falling(1.0));
        let reaching = pose.place(PartKind::Shin(1.0), Transform::default());
        assert!(launch.rotation.angle_between(Quat::IDENTITY) < 0.25);
        assert!(airborne.rotation.angle_between(Quat::IDENTITY) > 0.35);
        assert!(crouch.rotation.angle_between(airborne.rotation) > 0.10);
        assert!(airborne.rotation.angle_between(reaching.rotation) < 0.30);
        pose.jump = Some(JumpMotion::Landing(0.0));
        assert!(pose.place(PartKind::Body, body).translation.y < idle.translation.y - 0.1);
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
            jump: None,
            attack: None,
            dig: None,
        };
        let left = pose.place(PartKind::Arm(-1.0), rest);
        let right = pose.place(PartKind::Arm(1.0), rest);
        let elbow_rest = Transform::from_xyz(0.0, -0.29, 0.015);
        let idle_elbow = pose.place(PartKind::Forearm(1.0), elbow_rest);
        assert_ne!(left.rotation, right.rotation);
        assert!(left.translation.z > rest.translation.z);
        assert!(right.translation.z < rest.translation.z);
        let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
        attack.swift = true;
        attack.age = crate::combat::SWORD_WINDUP;
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
            jump: None,
            attack: None,
            dig: None,
        };
        let walking = pose.place(PartKind::Body, rest);
        let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
        attack.swift = true;
        pose.attack = Some(attack);
        let beginning = pose.place(PartKind::Body, rest);
        assert!(beginning.rotation.angle_between(walking.rotation) < 1e-5);
        attack.age = crate::combat::SWORD_WINDUP;
        pose.attack = Some(attack);
        let engaged = pose.place(PartKind::Body, rest);
        assert!(engaged.rotation.angle_between(walking.rotation) > 0.05);
        attack.age = crate::combat::SWORD_END;
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
            jump: None,
            attack: None,
            dig: None,
        };
        for age in [
            0.0,
            crate::combat::SWORD_WINDUP,
            0.18,
            crate::combat::SWORD_ACTIVE_END,
            crate::combat::SWORD_END,
        ] {
            let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
            attack.swift = true;
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

//! Climbing in the view (CL.4 of `kestrel/CLIMBING.md`, K3, K4, K9): what
//! the sim's climbing state (`sim::settlement::climb`) looks like.
//!
//! - The pose ([`ClimbPose`], posed by `character`): on a face the elder
//!   faces the rock with its hands up on it and its feet on it, solved onto
//!   the face by two-bone IK (`character::climb_limb_targets`). A reach
//!   cycle ([`ClimbClock`]) moves the hands and feet in diagonal pairs,
//!   hand over hand, advanced only by movement on the face, so a hanging
//!   elder holds still. The lunge stretches toward its way, the pull-over
//!   puts the hands on the top and a knee up, and a fall off a face flails.
//! - The drawn position on a face eases toward the sim's
//!   ([`ClimbFollow`]): an outside corner moves the body about 0.7 m in a
//!   tick and turns it a quarter, and the pull-over lifts, then moves over.
//! - The stamina wheel ([`Wheel`]): a ring by the elder's body, in screen
//!   space, as in Breath of the Wild. It drains, shows what a lunge just
//!   cost, flashes red near empty, stays red once exhausted, and hides once
//!   full on a floor for a moment. A fall that will hurt shows a red "!"
//!   in it while in the air.
//! - The silhouette ([`SilhouetteMaterial`]): the local elder's meshes are
//!   drawn a second time, flat, where rock is in front of them (the depth
//!   prepass more than [`SILHOUETTE_GAP`] meters nearer the camera than the
//!   body), in volume scenes.

use bevy::{
    asset::uuid::Uuid,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::ShaderRef,
};
use std::f32::consts::{PI, TAU};
use std::marker::PhantomData;

use sim::settlement::avatar::{AvatarState, JumpPhase};
use sim::settlement::climb::ClimbPhase;

use crate::character::Player;

/// One hand (or foot) move of the reach cycle covers this many rig meters
/// of climbing: the cycle is two moves, a hand each.
pub const REACH_STEP: f32 = 0.32;
/// The share of the cycle a hand holds the rock (the rest it reaches).
pub const GRIP: f32 = 0.6;
/// How fast the drawn climber eases toward the sim's position, per second.
pub const FOLLOW_RATE: f32 = 16.0;

/// What a climber is doing on (or off) a face, for posing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClimbStage {
    /// On the face, moved by the stick (or hanging still).
    Hold,
    /// A lunge, `progress` 0 to 1, toward `way` (rig x sideways, y up).
    Lunge { progress: f32, way: Vec2 },
    /// Pulling over the top, `progress` 0 to 1; `edge` is the top's edge
    /// in rig units over the root (y) and ahead of it (z).
    Mantle { progress: f32, edge: Vec2 },
    /// Falling after letting go (or the wheel running out), `time`
    /// seconds in; `hurts` if the landing will (K7).
    Falling { time: f32, hurts: bool },
}

/// A climber's pose, in the rig's units (meters over the actor scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClimbPose {
    pub stage: ClimbStage,
    /// The reach cycle's phase, radians ([`ClimbClock::phase`]).
    pub reach: f32,
    /// 0 hanging still to 1 climbing: how far the reaching hand leaves
    /// the rock.
    pub moving: f32,
    /// Which way it climbs on the face, a unit vector (x sideways, y up).
    pub travel: Vec2,
    /// The face's plane ahead of the root.
    pub face: f32,
}

/// The reach cycle: a phase that advances only with movement on the face.
#[derive(Clone, Copy, Debug)]
pub struct ClimbClock {
    pub phase: f32,
    pub moving: f32,
    pub travel: Vec2,
    /// Where the root was drawn last frame, and its rotation.
    last: Option<Vec3>,
}

impl Default for ClimbClock {
    fn default() -> Self {
        ClimbClock { phase: 0.0, moving: 0.0, travel: Vec2::Y, last: None }
    }
}

impl ClimbClock {
    /// Advances by the root's move since last frame, `root` drawn now. Only
    /// while `holding` (on a face, not lunging or pulling over) does it
    /// count; off a face it forgets where the root was.
    pub fn follow(&mut self, root: Transform, dt: f32, holding: bool) {
        let moved = match self.last {
            Some(last) if holding => {
                let local = root.rotation.inverse() * (root.translation - last) / root.scale.y.max(0.1);
                Vec2::new(local.x, local.y)
            }
            _ => Vec2::ZERO,
        };
        self.last = holding.then_some(root.translation);
        self.advance(moved, dt);
    }

    /// Advances by `moved` (rig x sideways, y up) in `dt` seconds.
    pub fn advance(&mut self, moved: Vec2, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        let distance = moved.length();
        // A corner's wrap is eased (`ClimbFollow`), so a frame's move is a
        // few centimeters; a bigger one is a snap, not climbing.
        let climbing = distance > 1e-4 && distance < 0.5;
        if climbing {
            self.phase = (self.phase + distance * PI / REACH_STEP).rem_euclid(TAU);
            let way = moved / distance;
            self.travel = self.travel.lerp(way, 1.0 - (-14.0 * dt).exp()).normalize_or(way);
        }
        let speed = if dt > 0.0 { distance / dt } else { 0.0 };
        let target = if climbing && speed > 0.1 { 1.0 } else { 0.0 };
        self.moving += (target - self.moving) * (1.0 - (-10.0 * dt).exp());
    }
}

/// The pose of a climber with `avatar`'s state, if it is climbing or
/// falling off a face: `root` is its drawn transform, `ticks` the drawn time,
/// `to_view` the sim-to-view mapping. Characters without a `climb` section
/// never climb.
pub fn climb_pose(
    avatar: &AvatarState,
    def: &content::character::CharacterDef,
    root: Transform,
    ticks: f32,
    clock: &ClimbClock,
    to_view: impl Fn(fx::FxVec2) -> Vec3,
) -> Option<ClimbPose> {
    let climb_def = def.climb.as_ref()?;
    let scale = root.scale.y.max(0.1);
    let face = (def.body.radius + sim::settlement::climb::GAP).to_f32() / scale;
    let pose = |stage| ClimbPose { stage, reach: clock.phase, moving: clock.moving, travel: clock.travel, face };
    let Some(climb) = avatar.climbing.climb else {
        // A fall off a face or a ledge: the jump's fall, for a climber with
        // no jump of its own (the elder). It hurts if its top is far enough
        // over the floor under it (`flight.support`).
        let JumpPhase::Airborne { started, .. } = avatar.jump else { return None };
        if def.jump().is_some() || avatar.flight.requested {
            return None;
        }
        let time = (ticks - started as f32).max(0.0) / content::TICKS_PER_SECOND as f32;
        let hurts = avatar.fall_top.is_some_and(|top| top - avatar.flight.support >= climb_def.safe_fall);
        return Some(pose(ClimbStage::Falling { time, hurts }));
    };
    let progress = |started: u64, until: u64| ((ticks - started as f32) / (until - started).max(1) as f32).clamp(0.0, 1.0);
    let stage = match climb.phase {
        ClimbPhase::Hold => ClimbStage::Hold,
        ClimbPhase::Lunge { started, until, up, side } => {
            let tangent = climb.face.tangent();
            let world = Vec3::new(tangent.x.to_f32(), 0.0, tangent.y.to_f32()) * side.to_f32() + Vec3::Y * up.to_f32();
            let local = root.rotation.inverse() * world;
            ClimbStage::Lunge { progress: progress(started, until), way: Vec2::new(local.x, local.y).normalize_or(Vec2::Y) }
        }
        ClimbPhase::Mantle { started, until, from, top, .. } => {
            let n = climb.face.normal();
            let into = -Vec3::new(n.x.to_f32(), 0.0, n.y.to_f32());
            let ahead = (root.translation - to_view(from)).with_y(0.0).dot(into);
            let over = (top.to_f32() - root.translation.y) / scale;
            ClimbStage::Mantle { progress: progress(started, until), edge: Vec2::new(over, face - ahead / scale) }
        }
    };
    Some(pose(stage))
}

/// Where the hands and feet go on the face, in rig units relative to the
/// root (the feet): `[left hand, right hand, left foot, right foot]`. The
/// hands hold over the shoulders, the feet under the hips; the reach cycle
/// moves them in diagonal pairs (the left hand with the right foot), each
/// holding the rock while the body climbs past it, then reaching on.
pub fn climb_targets(pose: &ClimbPose) -> [Vec3; 4] {
    let face = pose.face;
    let hand = |side: f32, offset: f32, lift: f32| Vec3::new(side * 0.30, 1.50, face - 0.05) + (pose.travel * offset).extend(-lift * 0.14);
    let foot = |side: f32, offset: f32, lift: f32| Vec3::new(side * 0.22, 0.36, face - 0.14) + (pose.travel * offset).extend(-lift * 0.10);
    let path = |phase: f32| -> (f32, f32) {
        let phase = phase.rem_euclid(TAU);
        let span = REACH_STEP * GRIP;
        if phase < TAU * GRIP {
            (span - phase * REACH_STEP / PI, 0.0)
        } else {
            let progress = (phase / TAU - GRIP) / (1.0 - GRIP);
            (-span + 2.0 * span * crate::combat::smooth(progress), (progress * PI).sin())
        }
    };
    let limb = |offset: f32| {
        let (along, lift) = path(pose.reach + offset);
        (along, lift * pose.moving)
    };
    let hold = {
        let (l, ll) = limb(0.0);
        let (r, rl) = limb(PI);
        [hand(-1.0, l, ll), hand(1.0, r, rl), foot(-1.0, r * 0.8, rl), foot(1.0, l * 0.8, ll)]
    };
    match pose.stage {
        ClimbStage::Hold => hold,
        ClimbStage::Lunge { progress, way } => {
            // Both hands stretch toward the way, the feet trail off the rock.
            let reach = crate::combat::smooth(progress / 0.35) * (1.0 - crate::combat::smooth((progress - 0.75) / 0.25));
            let stretch = |base: Vec3, by: Vec3| base.lerp(by, reach);
            let shoulders = Vec3::new(0.0, 1.30, 0.0);
            let up = (way * 0.62).extend(0.0);
            [
                stretch(hold[0], shoulders + up + Vec3::new(-0.20, 0.0, face - 0.12)),
                stretch(hold[1], shoulders + up + Vec3::new(0.20, 0.0, face - 0.12)),
                stretch(hold[2], Vec3::new(-0.18, 0.08, face - 0.30) - (way * 0.18).extend(0.0)),
                stretch(hold[3], Vec3::new(0.18, 0.02, face - 0.34) - (way * 0.18).extend(0.0)),
            ]
        }
        ClimbStage::Mantle { progress, edge } => {
            // The hands go onto the top, past the edge; a knee comes up onto
            // it as the body rises, then the other foot follows over.
            let on_top = |side: f32| Vec3::new(side * 0.26, edge.x + 0.04, edge.y + 0.16);
            let knee_up = crate::combat::smooth((progress - 0.25) / 0.4);
            let follow = crate::combat::smooth((progress - 0.65) / 0.3);
            let high = Vec3::new(0.20, edge.x + 0.06, edge.y + 0.10);
            let low = Vec3::new(-0.20, edge.x + 0.06, edge.y + 0.02);
            [
                on_top(-1.0),
                on_top(1.0),
                hold[2].lerp(low, follow),
                hold[3].lerp(high, knee_up),
            ]
        }
        ClimbStage::Falling { .. } => hold,
    }
}

/// The drawn position and heading of a climber: on a face they ease toward
/// the sim's (a wrapped corner, a pull-over); off one they snap back once
/// caught up.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClimbFollow {
    shown: Option<(Vec3, f32)>,
}

impl ClimbFollow {
    /// The drawn position and heading for the sim's `target` and
    /// `heading` this frame, `dt` seconds on. While `climbing` (on a face)
    /// they ease; once off, the ease finishes quickly and lets go. A jump
    /// of more than 2.5 m (a respawn) snaps.
    pub fn follow(&mut self, target: Vec3, heading: f32, dt: f32, climbing: bool) -> (Vec3, f32) {
        let Some((shown, facing)) = self.shown.filter(|(p, _)| p.distance(target) < 2.5) else {
            self.shown = climbing.then_some((target, heading));
            return (target, heading);
        };
        if !climbing && shown.distance(target) < 0.01 {
            self.shown = None;
            return (target, heading);
        }
        let rate = if climbing { FOLLOW_RATE } else { 30.0 };
        let weight = 1.0 - (-rate * dt.clamp(0.0, 0.1)).exp();
        let position = shown.lerp(target, weight);
        let facing = facing + crate::lockstep::angle_between(facing, heading) * weight;
        self.shown = Some((position, facing));
        (position, facing)
    }
}

/// The wheel's look this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelLook {
    /// 0 hidden to 1 shown.
    pub alpha: f32,
    /// What is left, 0 to 1.
    pub fill: f32,
    /// What was just spent (a lunge's fifth), still shown past `fill`.
    pub lost: f32,
    pub tone: WheelTone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelTone {
    /// Green.
    Normal,
    /// Near empty: flashing red, `flash` 0 to 1.
    Low { flash: f32 },
    /// Ran out: red until the wheel is full again (K6).
    Exhausted,
}

/// What the wheel shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelInput {
    /// 1 full to 0 empty (`Climbing::stamina`).
    pub stamina: f32,
    pub exhausted: bool,
    /// On a face.
    pub climbing: bool,
}

/// The wheel's own state across frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wheel {
    alpha: f32,
    /// The spent part still drawn, easing down to the fill.
    shown: f32,
    /// Seconds the spent part waits before it eases away.
    hold: f32,
    /// Seconds full and off a face.
    full_for: f32,
    clock: f32,
}

/// Under this the wheel flashes red.
pub const LOW: f32 = 0.3;
/// Seconds full and off a face before the wheel hides.
pub const HIDE_AFTER: f32 = 0.8;

impl Wheel {
    pub fn step(&mut self, input: WheelInput, dt: f32) -> WheelLook {
        let dt = dt.clamp(0.0, 0.1);
        self.clock += dt;
        let fill = input.stamina.clamp(0.0, 1.0);
        if fill >= self.shown {
            self.shown = fill;
            self.hold = 0.0;
        } else if self.hold < 0.35 {
            self.hold += dt;
        } else {
            self.shown = (self.shown - 0.6 * dt).max(fill);
        }
        let full = fill >= 0.999 && !input.exhausted;
        self.full_for = if full && !input.climbing { self.full_for + dt } else { 0.0 };
        let shown = !full || input.climbing || self.full_for < HIDE_AFTER;
        let target = if shown { 1.0 } else { 0.0 };
        let rate = if shown { 12.0 } else { 5.0 };
        self.alpha += (target - self.alpha) * (1.0 - (-rate * dt).exp());
        if self.alpha < 0.01 && !shown {
            self.alpha = 0.0;
        }
        let tone = if input.exhausted {
            WheelTone::Exhausted
        } else if fill < LOW {
            WheelTone::Low { flash: 0.5 + 0.5 * (self.clock * TAU * 3.0).cos() }
        } else {
            WheelTone::Normal
        };
        WheelLook { alpha: self.alpha, fill, lost: self.shown, tone }
    }
}

const SEGMENTS: usize = 32;
const WHEEL_RADIUS: f32 = 15.0;
const WHEEL_SIZE: f32 = 40.0;
const GREEN: [f32; 3] = [0.46, 0.92, 0.38];
const RED: [f32; 3] = [0.96, 0.22, 0.14];
const SPENT: [f32; 3] = [1.0, 0.86, 0.52];
const EMPTY: [f32; 4] = [0.03, 0.05, 0.06, 0.72];

/// The wheel's root node, by the local elder.
#[derive(Component)]
pub(crate) struct WheelNode {
    wheel: Wheel,
}
/// One segment of the ring, clockwise from the top.
#[derive(Component)]
pub(crate) struct WheelSegment(usize);
/// The "this fall will hurt" mark in the middle.
#[derive(Component)]
pub(crate) struct FallMark;

/// A segment's colour (sRGB with alpha) for a look.
pub fn segment_color(look: &WheelLook, index: usize) -> [f32; 4] {
    let start = index as f32 / SEGMENTS as f32;
    let end = (index + 1) as f32 / SEGMENTS as f32;
    let share = |upto: f32| ((upto - start) / (end - start)).clamp(0.0, 1.0);
    let filled = share(look.fill);
    let spent = share(look.lost) - filled;
    let tone = match look.tone {
        WheelTone::Normal => GREEN,
        WheelTone::Low { flash } => mix(GREEN, RED, 0.45 + 0.55 * flash),
        WheelTone::Exhausted => RED,
    };
    let mut color = mix3(EMPTY, tone, filled);
    if spent > 0.0 {
        color = mix3(color, SPENT, spent);
    }
    color[3] *= look.alpha;
    color
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn mix3(a: [f32; 4], b: [f32; 3], t: f32) -> [f32; 4] {
    let rgb = mix([a[0], a[1], a[2]], b, t);
    [rgb[0], rgb[1], rgb[2], a[3] + (1.0 - a[3]) * t]
}

/// The stamina wheel by the local climber's body (PostUpdate, after the
/// transforms): the drawn avatar's wheel, for a character that climbs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wheel(
    mut commands: Commands,
    time: Res<Time>,
    drawn: Res<crate::prediction::DrawnAvatar>,
    scene: Res<crate::scene_view::SceneView>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    player: Single<&GlobalTransform, With<Player>>,
    mut roots: Query<(&mut WheelNode, &mut Node, &mut Visibility), (Without<WheelSegment>, Without<FallMark>)>,
    mut segments: Query<(&WheelSegment, &mut BackgroundColor)>,
    mut marks: Query<(&mut Visibility, &mut TextColor), (With<FallMark>, Without<WheelNode>)>,
) {
    let Some(climb_def) = scene.player().climb.as_ref() else { return };
    let Ok((mut root, mut node, mut visibility)) = roots.single_mut() else {
        spawn_wheel(&mut commands);
        return;
    };
    let Some(avatar) = drawn.0.as_ref().map(|d| d.avatar()) else {
        *visibility = Visibility::Hidden;
        return;
    };
    let input = WheelInput {
        stamina: avatar.climbing.stamina().to_f32(),
        exhausted: avatar.climbing.exhausted,
        climbing: avatar.climbing.climb.is_some(),
    };
    let hurts = avatar.climbing.climb.is_none()
        && matches!(avatar.jump, JumpPhase::Airborne { .. })
        && avatar.fall_top.is_some_and(|top| top - avatar.flight.support >= climb_def.safe_fall);
    let mut look = root.wheel.step(input, time.delta_secs());
    if hurts {
        look.alpha = 1.0;
    }
    for (mut mark, mut color) in &mut marks {
        *mark = if hurts { Visibility::Inherited } else { Visibility::Hidden };
        let pulse = 0.6 + 0.4 * (time.elapsed_secs() * TAU * 4.0).cos();
        color.0 = Color::srgba(RED[0], RED[1], RED[2], pulse);
    }
    let scale = player.compute_transform().scale.y;
    let body = player.translation() + Vec3::Y * 1.1 * scale;
    // The body's right side on screen, whatever the zoom and the turn.
    let side = body + camera.1.right() * 0.55 * scale;
    let (Some(at), Some(edge)) = (camera.0.world_to_viewport(camera.1, body).ok().filter(|_| look.alpha > 0.0), camera.0.world_to_viewport(camera.1, side).ok()) else {
        *visibility = Visibility::Hidden;
        return;
    };
    // Beside the body on the right, clear of the health bar over the head.
    node.left = px(edge.x.max(at.x + 10.0) + 6.0);
    node.top = px(at.y - WHEEL_SIZE / 2.0 - 8.0);
    *visibility = Visibility::Inherited;
    for (segment, mut color) in &mut segments {
        let [r, g, b, a] = segment_color(&look, segment.0);
        color.0 = Color::srgba(r, g, b, a);
    }
}

fn spawn_wheel(commands: &mut Commands) {
    commands
        .spawn((
            WheelNode { wheel: Wheel::default() },
            service::WorldScoped::default(),
            Node { position_type: PositionType::Absolute, width: px(WHEEL_SIZE), height: px(WHEEL_SIZE), ..default() },
            Visibility::Hidden,
        ))
        .with_children(|parent| {
            for index in 0..SEGMENTS {
                let angle = (index as f32 + 0.5) / SEGMENTS as f32 * TAU;
                let (w, h) = (3.0, 7.0);
                let center = Vec2::new(WHEEL_SIZE / 2.0 + WHEEL_RADIUS * angle.sin(), WHEEL_SIZE / 2.0 - WHEEL_RADIUS * angle.cos());
                parent.spawn((
                    WheelSegment(index),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(center.x - w / 2.0),
                        top: px(center.y - h / 2.0),
                        width: px(w),
                        height: px(h),
                        border_radius: BorderRadius::all(px(1.0)),
                        ..default()
                    },
                    UiTransform { rotation: Rot2::radians(angle), ..default() },
                    BackgroundColor(Color::NONE),
                ));
            }
            parent.spawn((
                FallMark,
                Text::new("!"),
                TextFont { font_size: FontSize::Px(24.0), ..default() },
                TextColor(Color::srgb(RED[0], RED[1], RED[2])),
                TextLayout::justify(Justify::Center),
                Node { position_type: PositionType::Absolute, left: px(0), top: px(WHEEL_SIZE / 2.0 - 12.0), width: px(WHEEL_SIZE), ..default() },
                Visibility::Hidden,
            ));
        });
}

const SILHOUETTE_SHADER: Handle<Shader> = Handle::Uuid(Uuid::from_u128(0x3c2d8f61_7a0e_4b5c_9e1d_5f4a8b2c6d04), PhantomData);

/// Rock must be this many meters nearer the camera than the body for the
/// silhouette to show there: more than the body is deep seen from above
/// (the jetpack frame to the raised hands), so the elder's own parts in
/// front of one another don't; any rock face hiding a climber is a tile.
pub const SILHOUETTE_GAP: f32 = 1.3;
/// The silhouette's colour: the HUD's cyan, flat (linear, with alpha),
/// which reads against every shade of the gym's brown rock.
pub const SILHOUETTE_COLOR: [f32; 4] = [0.42, 0.92, 1.0, 0.85];

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct SilhouetteUniform {
    pub color: Vec4,
    /// x: [`SILHOUETTE_GAP`].
    pub settings: Vec4,
}

/// The elder drawn flat through rock in front of it (K9): blended, after
/// the opaque pass, only where the depth there is nearer than the body
/// (depth test reversed) by [`SILHOUETTE_GAP`].
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct SilhouetteMaterial {
    #[uniform(0)]
    pub uniform: SilhouetteUniform,
}

impl Default for SilhouetteMaterial {
    fn default() -> Self {
        SilhouetteMaterial { uniform: SilhouetteUniform { color: Vec4::from_array(SILHOUETTE_COLOR), settings: Vec4::new(SILHOUETTE_GAP, 0.0, 0.0, 0.0) } }
    }
}

impl Material for SilhouetteMaterial {
    fn fragment_shader() -> ShaderRef {
        SILHOUETTE_SHADER.clone().into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Reversed depth (near is 1): `Less` passes only behind what is
        // drawn there already.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Less);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

pub fn register_shader(app: &mut App) {
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(SILHOUETTE_SHADER.id(), Shader::from_wesl(include_str!("../shaders/silhouette.wesl"), "silhouette.wesl"))
        .expect("static silhouette shader handle");
}

/// A mesh drawn as part of the elder's silhouette.
#[derive(Component)]
pub struct Silhouette;
/// A mesh of the elder's that has its silhouette.
#[derive(Component)]
pub struct Silhouetted;

/// Gives each of the local player's meshes a silhouette twin (a child
/// with the same mesh and the silhouette material), once, in a volume
/// scene; elsewhere hides them. Meshes outside the player have none.
pub(crate) fn silhouettes(
    mut commands: Commands,
    service: Option<Res<crate::lockstep::SessionService>>,
    mut materials: ResMut<Assets<SilhouetteMaterial>>,
    mut handle: Local<Option<Handle<SilhouetteMaterial>>>,
    player: Option<Single<Entity, With<Player>>>,
    meshes: Query<(Entity, &Mesh3d), (With<MeshMaterial3d<StandardMaterial>>, Without<Silhouetted>, Without<Silhouette>)>,
    parents: Query<&ChildOf>,
    mut twins: Query<&mut Visibility, With<Silhouette>>,
) {
    let Some(player) = player.map(|p| *p) else { return };
    let volume = service.is_none_or(|s| s.world().is_none_or(|w| w.terrain().volume.is_some()));
    for mut visibility in &mut twins {
        let want = if volume { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want {
            *visibility = want;
        }
    }
    let material = handle.get_or_insert_with(|| materials.add(SilhouetteMaterial::default())).clone();
    for (entity, mesh) in &meshes {
        if !parents.iter_ancestors(entity).any(|a| a == player) {
            continue;
        }
        commands.entity(entity).insert(Silhouetted);
        let twin = commands
            .spawn((
                Silhouette,
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                if volume { Visibility::Inherited } else { Visibility::Hidden },
                bevy::light::NotShadowCaster,
            ))
            .id();
        commands.entity(entity).add_child(twin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(stage: ClimbStage, clock: &ClimbClock) -> ClimbPose {
        ClimbPose { stage, reach: clock.phase, moving: clock.moving, travel: clock.travel, face: 0.34 }
    }

    /// Climbing moves the reach cycle; hanging still doesn't, and the
    /// hands then stay on the rock.
    #[test]
    fn the_reach_cycle_advances_only_while_moving() {
        let mut clock = ClimbClock::default();
        let dt = 1.0 / 60.0;
        for _ in 0..30 {
            clock.advance(Vec2::new(0.0, 1.0 / 60.0), dt);
        }
        let phase = clock.phase;
        assert!((phase - 0.5 * PI / REACH_STEP).abs() < 1e-3, "half a meter up: {phase}");
        assert!(clock.moving > 0.9 && clock.travel.y > 0.99);
        for _ in 0..60 {
            clock.advance(Vec2::ZERO, dt);
        }
        assert_eq!(clock.phase, phase, "hanging still holds the cycle");
        assert!(clock.moving < 0.01);
        let still = climb_targets(&pose(ClimbStage::Hold, &clock));
        for hand in &still[..2] {
            assert!((hand.z - 0.29).abs() < 1e-3, "a still hand is on the rock: {hand}");
        }
        // Sideways moves it too, and the travel turns that way.
        for _ in 0..30 {
            clock.advance(Vec2::new(-1.5 / 60.0, 0.0), dt);
        }
        assert!(clock.phase != phase && clock.travel.x < -0.99);
        // A corner's snap (or a respawn) is not climbing.
        let before = clock.phase;
        clock.advance(Vec2::new(0.7, 0.0), dt);
        assert_eq!(clock.phase, before);
    }

    /// The hands hold the face over the shoulders and the feet under the
    /// hips, the reaching ones off it; while holding, a hand stays put on
    /// the rock as the body climbs past it.
    #[test]
    fn hands_and_feet_hold_the_face_in_diagonal_pairs() {
        let clock = ClimbClock { phase: 1.0, moving: 1.0, travel: Vec2::Y, last: None };
        let p = pose(ClimbStage::Hold, &clock);
        let [lh, rh, lf, rf] = climb_targets(&p);
        assert!(lh.x < 0.0 && rh.x > 0.0 && lf.x < 0.0 && rf.x > 0.0);
        assert!(lh.y > 1.25 && rh.y > 1.25, "hands over the shoulders: {lh} {rh}");
        assert!(lf.y < 0.6 && rf.y < 0.6, "feet under the hips: {lf} {rf}");
        for limb in [lh, rh, lf, rf] {
            assert!(limb.z <= p.face + 1e-4, "nothing in the rock: {limb}");
        }
        // The left hand grips (on the rock) while the right reaches (off it).
        assert!((lh.z - (p.face - 0.05)).abs() < 1e-4 && rh.z < lh.z - 0.05);
        // The left hand and right foot move together.
        let later = climb_targets(&ClimbPose { reach: 1.2, ..p });
        let up = REACH_STEP * 0.2 / PI;
        assert!(((lh.y - later[0].y) - up).abs() < 1e-4, "a gripping hand slides down as the body climbs");
        assert!((later[3].y - rf.y) * (later[0].y - lh.y) > 0.0);
    }

    /// The lunge stretches the hands its way; the pull-over puts them on
    /// the top.
    #[test]
    fn the_lunge_reaches_and_the_pull_over_holds_the_top() {
        let clock = ClimbClock::default();
        let rest = climb_targets(&pose(ClimbStage::Hold, &clock));
        let mid = climb_targets(&pose(ClimbStage::Lunge { progress: 0.5, way: Vec2::Y }, &clock));
        let highest = rest[0].y.max(rest[1].y);
        assert!(mid[0].y > highest + 0.1 && mid[1].y > highest + 0.1, "{mid:?}");
        let side = climb_targets(&pose(ClimbStage::Lunge { progress: 0.5, way: Vec2::X }, &clock));
        assert!(side[1].x > rest[1].x + 0.2);
        let edge = Vec2::new(0.6, 0.3);
        let over = climb_targets(&pose(ClimbStage::Mantle { progress: 0.5, edge }, &clock));
        for hand in &over[..2] {
            assert!(hand.y > edge.x && hand.z > edge.y, "a hand on the top: {hand}");
        }
    }

    /// The pose follows the sim's climbing state: held on a face, lunging,
    /// pulling over, falling off one (and whether that will hurt), and none
    /// standing.
    #[test]
    fn the_pose_follows_the_sims_climbing_state() {
        use fx::{Fx, FxVec2};
        use sim::settlement::climb::{Climb, Face};
        let def = crate::lockstep::builtin_character("elder");
        let root = Transform::from_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
        let clock = ClimbClock::default();
        let view = |p: FxVec2| Vec3::new(p.x.to_f32(), 0.0, p.y.to_f32());
        let mut avatar = AvatarState::default();
        assert_eq!(climb_pose(&avatar, def, root, 0.0, &clock, view), None, "standing");
        avatar.climbing.climb = Some(Climb { tile: (-14, -5), face: Face::West, phase: ClimbPhase::Hold });
        let held = climb_pose(&avatar, def, root, 0.0, &clock, view).unwrap();
        assert_eq!(held.stage, ClimbStage::Hold);
        assert!((held.face - 0.34).abs() < 1e-3, "the face 2 cm off the body: {}", held.face);
        // A lunge up, and one along the face.
        let tangent = Face::West.tangent();
        avatar.climbing.climb = Some(Climb { tile: (-14, -5), face: Face::West, phase: ClimbPhase::Lunge { started: 10, until: 30, up: Fx::from_ratio(1, 20), side: Fx::ZERO } });
        let ClimbStage::Lunge { progress, way } = climb_pose(&avatar, def, root, 20.0, &clock, view).unwrap().stage else { panic!() };
        assert!((progress - 0.5).abs() < 1e-4 && way.y > 0.99);
        avatar.climbing.climb = Some(Climb { tile: (-14, -5), face: Face::West, phase: ClimbPhase::Lunge { started: 10, until: 30, up: Fx::ZERO, side: Fx::from_ratio(1, 10) } });
        let ClimbStage::Lunge { way, .. } = climb_pose(&avatar, def, root, 20.0, &clock, view).unwrap().stage else { panic!() };
        let world = root.rotation * way.extend(0.0);
        assert!(world.distance(Vec3::new(tangent.x.to_f32(), 0.0, tangent.y.to_f32())) < 1e-3, "{world}");
        // Pulling over: the edge over the feet by what is left to rise.
        let from = FxVec2::new(Fx::from_ratio(-27, 2) - Fx::from_ratio(34, 100), Fx::from_int(-5));
        avatar.height = Fx::from_int(13);
        avatar.climbing.climb = Some(Climb { tile: (-14, -5), face: Face::West, phase: ClimbPhase::Mantle { started: 0, until: 30, from, low: Fx::from_ratio(25, 2), to: from, top: Fx::from_ratio(27, 2) } });
        let at = Transform { translation: Vec3::new(from.x.to_f32(), 13.0, -5.0), ..root };
        let ClimbStage::Mantle { progress, edge } = climb_pose(&avatar, def, at, 15.0, &clock, view).unwrap().stage else { panic!() };
        assert!((progress - 0.5).abs() < 1e-4 && (edge.x - 0.5).abs() < 1e-3 && (edge.y - 0.34).abs() < 1e-3, "{edge}");
        // Let go at 13 m over a floor at 6 m, a meter into the fall: it will
        // hurt; from 9 m it will not.
        avatar.climbing.climb = None;
        avatar.jump = JumpPhase::Airborne { started: 0, velocity: Fx::ZERO };
        avatar.fall_top = Some(Fx::from_int(13));
        avatar.height = Fx::from_int(12);
        avatar.flight.support = Fx::from_int(6);
        assert!(matches!(climb_pose(&avatar, def, root, 30.0, &clock, view).unwrap().stage, ClimbStage::Falling { hurts: true, time } if (time - 0.5).abs() < 1e-4));
        avatar.fall_top = Some(Fx::from_int(9));
        assert!(matches!(climb_pose(&avatar, def, root, 30.0, &clock, view).unwrap().stage, ClimbStage::Falling { hurts: false, .. }));
        // Characters that do not climb never do.
        assert_eq!(climb_pose(&avatar, crate::lockstep::builtin_character("exosuit"), root, 0.0, &clock, view), None);
    }

    /// Hidden full on the ground, shown climbing, red near empty, red once
    /// exhausted; a lunge's cost shows for a moment.
    #[test]
    fn the_wheel_hides_full_and_warns_near_empty() {
        let dt = 1.0 / 60.0;
        let mut wheel = Wheel::default();
        let ground = WheelInput { stamina: 1.0, exhausted: false, climbing: false };
        let mut look = wheel.step(ground, dt);
        for _ in 0..120 {
            look = wheel.step(ground, dt);
        }
        assert_eq!(look.alpha, 0.0, "hidden full on the ground");
        let climbing = WheelInput { climbing: true, ..ground };
        for _ in 0..30 {
            look = wheel.step(climbing, dt);
        }
        assert!(look.alpha > 0.95 && look.tone == WheelTone::Normal && look.fill == 1.0);
        // A lunge spends a fifth: shown as spent, then easing away.
        look = wheel.step(WheelInput { stamina: 0.8, ..climbing }, dt);
        assert!((look.fill - 0.8).abs() < 1e-6 && look.lost == 1.0);
        for _ in 0..90 {
            look = wheel.step(WheelInput { stamina: 0.8, ..climbing }, dt);
        }
        assert!((look.lost - 0.8).abs() < 1e-6, "the spent part eases away: {}", look.lost);
        let low = wheel.step(WheelInput { stamina: 0.2, ..climbing }, dt);
        assert!(matches!(low.tone, WheelTone::Low { .. }));
        let flashes: Vec<f32> = (0..40).map(|_| match wheel.step(WheelInput { stamina: 0.2, ..climbing }, dt).tone {
            WheelTone::Low { flash } => flash,
            tone => panic!("{tone:?}"),
        }).collect();
        assert!(flashes.iter().any(|f| *f > 0.9) && flashes.iter().any(|f| *f < 0.1), "it flashes");
        let red = segment_color(&WheelLook { alpha: 1.0, fill: 0.2, lost: 0.2, tone: WheelTone::Low { flash: 1.0 } }, 0);
        assert!(red[0] > 0.9 && red[1] < 0.3, "near empty is red: {red:?}");
        // Out: red, falling, and still red refilling on the ground.
        let out = WheelInput { stamina: 0.0, exhausted: true, climbing: false };
        assert_eq!(wheel.step(out, dt).tone, WheelTone::Exhausted);
        let refilling = wheel.step(WheelInput { stamina: 0.7, ..out }, dt);
        assert!(refilling.tone == WheelTone::Exhausted && refilling.alpha > 0.9);
        // Full again on the ground: back to green, and gone a moment later.
        for _ in 0..30 {
            look = wheel.step(ground, dt);
        }
        assert!(look.tone == WheelTone::Normal && look.alpha > 0.5, "still shown for a moment");
        for _ in 0..120 {
            look = wheel.step(ground, dt);
        }
        assert_eq!(look.alpha, 0.0);
    }

    /// On a face the drawn climber eases round a corner's jump instead of
    /// snapping; a respawn snaps; off the face it lets go.
    #[test]
    fn the_drawn_climber_eases_round_a_corner() {
        let mut follow = ClimbFollow::default();
        let dt = 1.0 / 60.0;
        assert_eq!(follow.follow(Vec3::ZERO, 0.0, dt, true), (Vec3::ZERO, 0.0));
        let corner = Vec3::new(0.7, 0.0, 0.5);
        let (p, h) = follow.follow(corner, std::f32::consts::FRAC_PI_2, dt, true);
        assert!(p.length() > 0.0 && p.distance(corner) > 0.5, "eases: {p}");
        assert!(h > 0.0 && h < 0.5);
        for _ in 0..30 {
            follow.follow(corner, std::f32::consts::FRAC_PI_2, dt, true);
        }
        let (p, _) = follow.follow(corner, std::f32::consts::FRAC_PI_2, dt, true);
        assert!(p.distance(corner) < 0.01);
        let far = Vec3::new(30.0, 0.0, 0.0);
        assert_eq!(follow.follow(far, 0.0, dt, true).0, far, "a respawn snaps");
        let off = far + Vec3::Y * 0.001;
        assert_eq!(follow.follow(off, 0.0, dt, false).0, off);
    }

    /// Only the player's meshes get a silhouette twin, once each.
    #[test]
    fn the_silhouette_draws_the_player_only() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>().init_resource::<Assets<SilhouetteMaterial>>().init_resource::<Assets<StandardMaterial>>();
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let world = app.world_mut();
        let part = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
        let joint = world.spawn(Transform::default()).add_child(part).id();
        world.spawn((Player, Transform::default())).add_child(joint);
        let other = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
        app.add_systems(Update, silhouettes);
        app.update();
        app.update();
        let world = app.world_mut();
        let twins: Vec<Entity> = world.query_filtered::<Entity, With<Silhouette>>().iter(world).collect();
        assert_eq!(twins.len(), 1, "one twin, for the player's one mesh");
        assert_eq!(world.get::<ChildOf>(twins[0]).unwrap().parent(), part);
        assert!(world.get::<MeshMaterial3d<SilhouetteMaterial>>(twins[0]).is_some());
        assert_eq!(world.get::<Mesh3d>(twins[0]).unwrap().0, mesh);
        assert!(world.get::<Silhouetted>(other).is_none());
        let material = world.get::<MeshMaterial3d<SilhouetteMaterial>>(twins[0]).unwrap().0.clone();
        let material = world.resource::<Assets<SilhouetteMaterial>>().get(&material).unwrap();
        assert_eq!(material.alpha_mode(), AlphaMode::Blend);
    }
}

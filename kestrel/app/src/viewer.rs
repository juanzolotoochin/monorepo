//! Character studio: repeatable poses from the production rig, viewed in place.
use crate::{
    assets::SceneLibrary,
    character::{JumpMotion, LocalBlade, Player},
    combat::Attack,
    gameplay::Controls,
    menu::LaunchMenu,
};
use bevy::{
    app::AppExit,
    camera::ScalingMode,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    ui::FocusPolicy,
};
use sim::settlement::avatar::SwordStroke;

// Rig roots use y=0 as their standing surface. Keep the inspection floor and
// root on the same plane so poses can be judged against actual foot contact.
const STUDIO_FLOOR_Y: f32 = 20.0;
const STUDIO_FLOOR_THICKNESS: f32 = 0.2;
const GRID_SPACING: f32 = 0.5;
const GRID_PERIOD: f32 = 4.0;
pub(crate) const STUDIO_FPS: f32 = 60.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clip {
    Idle,
    Walk,
    #[default]
    Run,
    Turn,
    Jump,
    RunJump,
    Forehand,
    Backhand,
    Overhead,
}
impl Clip {
    pub(crate) fn frame_count(self) -> u64 {
        match self {
            Self::Run => 30,
            Self::Walk => 33,
            Self::Turn => 200,
            Self::Jump => 80,
            Self::RunJump => 135,
            Self::Forehand | Self::Backhand | Self::Overhead => 66,
            Self::Idle => 90,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Walk => "WALK / PLANTED FEET",
            Self::Run => "RUN",
            Self::Turn => "RUN + DIRECTION CHANGE",
            Self::Jump => "JUMP",
            Self::RunJump => "RUN + JUMP",
            Self::Forehand => "SWORD / FOREHAND",
            Self::Backhand => "SWORD / BACKHAND",
            Self::Overhead => "SWORD / OVERHEAD",
        }
    }
    fn stroke(self) -> Option<SwordStroke> {
        match self {
            Self::Forehand => Some(SwordStroke::Forehand),
            Self::Backhand => Some(SwordStroke::Backhand),
            Self::Overhead => Some(SwordStroke::Overhead),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewAngle {
    #[default]
    ThreeQuarter,
    Front,
    Side,
}
#[derive(Resource, Default, Clone, Copy)]
pub struct Viewer {
    pub active: bool,
    pub clip: Clip,
    pub angle: ViewAngle,
    pub elapsed: f32,
    pub frame: u64,
    total_frame: u64,
    pub speed: f32,
    pub paused: bool,
    frame_accumulator: f32,
    motion_frame: u64,
    ground_offset: Vec2,
    distance: f32,
    gait_distance: f32,
}
impl Viewer {
    pub fn begin(&mut self) {
        self.active = true;
        self.clip = Clip::Run;
        self.angle = ViewAngle::ThreeQuarter;
        self.reset_clock();
        self.speed = 1.0;
        self.paused = false;
    }
    fn reset_clock(&mut self) {
        self.seek_frame(0);
    }
    fn seek_frame(&mut self, frame: u64) {
        self.frame = frame % self.clip.frame_count();
        self.total_frame = self.frame;
        self.frame_accumulator = 0.0;
        self.elapsed = self.frame as f32 / STUDIO_FPS;
        self.motion_frame = 0;
        self.ground_offset = Vec2::ZERO;
        self.distance = 0.0;
        self.gait_distance = 0.0;
    }
    fn advance_clock(&mut self, dt: f32) {
        self.frame_accumulator += dt.min(0.05).max(0.0) * self.speed * STUDIO_FPS;
        let frames = self.frame_accumulator.floor() as u64;
        self.frame_accumulator -= frames as f32;
        self.total_frame += frames;
        self.frame = self.total_frame % self.clip.frame_count();
        self.elapsed = self.frame as f32 / STUDIO_FPS;
    }
    fn step_frame(&mut self) {
        self.seek_frame(self.frame + 1);
    }
    fn step_back_frame(&mut self) {
        self.seek_frame(self.frame + self.clip.frame_count() - 1);
    }
    fn phase(&self) -> f32 {
        self.elapsed
    }
    fn turn_heading(&self) -> f32 {
        let t = self.phase();
        let blend = |start: f32, end: f32| crate::combat::smooth((t - start) / (end - start));
        if t < 0.65 {
            0.0
        } else if t < 1.10 {
            1.05 * blend(0.65, 1.10)
        } else if t < 1.55 {
            1.05
        } else if t < 2.05 {
            crate::combat::lerp(1.05, -0.75, blend(1.55, 2.05))
        } else if t < 2.50 {
            -0.75
        } else if t < 3.05 {
            -0.75 * (1.0 - blend(2.50, 3.05))
        } else {
            0.0
        }
    }
    pub(crate) fn turn_lean(&self) -> f32 {
        if self.clip != Clip::Turn {
            return 0.0;
        }
        let previous_frame = (self.frame + self.clip.frame_count() - 1) % self.clip.frame_count();
        let previous = Self { frame: previous_frame, elapsed: previous_frame as f32 / STUDIO_FPS, ..*self };
        ((self.turn_heading() - previous.turn_heading()) * 0.15 * STUDIO_FPS)
            .clamp(-0.9, 0.9)
    }
    pub fn jump(&self) -> (Option<JumpMotion>, f32) {
        if !matches!(self.clip, Clip::Jump | Clip::RunJump) {
            return (None, 0.0);
        }
        let t = self.phase() - if self.clip == Clip::RunJump { 0.55 } else { 0.0 };
        if t < 0.0 {
            return (None, 0.0);
        }
        if t < 0.07 {
            return (Some(JumpMotion::Preparing(crate::combat::smooth(t / 0.07))), 0.0);
        }
        if t < 0.14 {
            return (Some(JumpMotion::Pushing(crate::combat::smooth((t - 0.07) / 0.07))), 0.0);
        }
        if t < 0.72 {
            let p = ((t - 0.14) / 0.58).clamp(0.0, 1.0);
            let height = 0.56 * 4.0 * p * (1.0 - p);
            return (
                Some(if p < 0.5 {
                    JumpMotion::Rising(p * 2.0)
                } else {
                    JumpMotion::Falling((p - 0.5) * 2.0)
                }),
                height,
            );
        }
        if t < 0.92 {
            return (Some(JumpMotion::Landing((t - 0.72) / 0.20)), 0.0);
        }
        (None, 0.0)
    }

    fn advance_ground(&mut self, dt: f32, velocity: Vec3, step_distance: f32, stepping: bool) -> f32 {
        let displacement = velocity * dt.max(0.0);
        self.ground_offset += Vec2::new(displacement.x, displacement.z);
        self.distance += displacement.length();
        if stepping {
            self.gait_distance += displacement.length();
        }
        (self.gait_distance * std::f32::consts::PI / step_distance.clamp(0.2, 4.0))
            .rem_euclid(std::f32::consts::TAU)
    }
}

pub fn not_active(viewer: Res<Viewer>) -> bool {
    !viewer.active
}

pub fn sync_hud(viewer: Res<Viewer>, mut roots: Query<&mut Node, With<crate::scene::HudRoot>>) {
    for mut node in &mut roots {
        node.display = if viewer.active {
            Display::None
        } else {
            Display::Flex
        };
    }
}

#[derive(Component)]
pub(crate) struct Root;
#[derive(Component)]
pub(crate) struct Stage;
#[derive(Component)]
pub(crate) struct GroundMark {
    axis: MarkAxis,
    base: f32,
}
#[derive(Clone, Copy)]
enum MarkAxis { X, Z }

fn wrapped_mark(base: f32, offset: f32) -> f32 {
    (base - offset + GRID_PERIOD * 0.5).rem_euclid(GRID_PERIOD) - GRID_PERIOD * 0.5
}
#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Clip(Clip),
    Angle(ViewAngle),
    Speed,
    Pause,
    Step,
    StepBack,
    Reset,
    Back,
}
#[derive(Component)]
pub(crate) struct Status;
#[derive(Component)]
pub(crate) struct SwordMetrics;
#[derive(Component)]
pub(crate) struct SwordMetricsTitle;
#[derive(Component)]
pub(crate) struct SwordAngleText;
#[derive(Component)]
pub(crate) struct SwordPhaseText;
#[derive(Default)]
pub(crate) struct AngleLog {
    initialized: bool,
    enabled: bool,
    previous: Option<(Clip, u64)>,
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, title: &str) {
    parent
        .spawn((
            action,
            Interaction::None,
            FocusPolicy::Block,
            Node {
                width: percent(100.0),
                min_height: px(30.0),
                padding: UiRect::axes(px(10.0), px(5.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.16, 0.20, 0.92)),
            BorderColor::all(Color::srgb(0.18, 0.37, 0.42)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.95, 0.94)),
            ));
        });
}
fn label(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.49, 0.82, 0.85)),
    ));
}

pub fn setup(
    mut commands: Commands,
    viewer: Res<Viewer>,
    roots: Query<Entity, With<Root>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !viewer.active || !roots.is_empty() {
        return;
    }
    commands.spawn((
        Stage,
        Mesh3d(meshes.add(Cuboid::new(4.0, STUDIO_FLOOR_THICKNESS, 4.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.09, 0.17, 0.21),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, STUDIO_FLOOR_Y - STUDIO_FLOOR_THICKNESS * 0.5, 0.0),
    ));
    for index in -4..4 {
        let base = index as f32 * GRID_SPACING;
        let major = index % 2 == 0;
        let material = materials.add(StandardMaterial {
            base_color: if major { Color::srgb(0.32, 0.53, 0.57) } else { Color::srgb(0.19, 0.34, 0.38) },
            perceptual_roughness: 0.92,
            ..default()
        });
        let width = if major { 0.018 } else { 0.008 };
        for axis in [MarkAxis::X, MarkAxis::Z] {
            let size = match axis {
                MarkAxis::X => Vec3::new(width, 0.004, GRID_PERIOD),
                MarkAxis::Z => Vec3::new(GRID_PERIOD, 0.004, width),
            };
            commands.spawn((
                Stage,
                GroundMark { axis, base },
                Mesh3d(meshes.add(Cuboid::from_size(size))),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(
                    if matches!(axis, MarkAxis::X) { base } else { 0.0 },
                    STUDIO_FLOOR_Y + 0.003,
                    if matches!(axis, MarkAxis::Z) { base } else { 0.0 },
                ),
            ));
        }
    }
    for (position, intensity) in [
        (Vec3::new(2.5, STUDIO_FLOOR_Y + 3.5, 3.5), 1900.0),
        (Vec3::new(-3.0, STUDIO_FLOOR_Y + 2.6, -1.0), 1100.0),
    ] {
        commands.spawn((
            Stage,
            PointLight {
                intensity,
                range: 9.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(position),
        ));
    }
    commands
        .spawn((
            Root,
            GlobalZIndex(1001),
            Node {
                position_type: PositionType::Absolute,
                left: px(20.0),
                top: px(18.0),
                width: px(245.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                padding: UiRect::all(px(15.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.05, 0.94)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("CHARACTER STUDIO"),
                TextFont {
                    font_size: FontSize::Px(19.0),
                    ..default()
                },
                TextColor(Color::srgb(0.42, 0.92, 0.95)),
            ));
            label(panel, "LOOPING ANIMATIONS");
            label(panel, "MOVING FLOOR / 0.5 M GRID");
            for clip in [
                Clip::Idle,
                Clip::Walk,
                Clip::Run,
                Clip::Turn,
                Clip::Jump,
                Clip::RunJump,
                Clip::Forehand,
                Clip::Backhand,
                Clip::Overhead,
            ] {
                button(panel, Action::Clip(clip), clip.title());
            }
            label(panel, "CAMERA");
            button(
                panel,
                Action::Angle(ViewAngle::ThreeQuarter),
                "THREE QUARTER",
            );
            button(panel, Action::Angle(ViewAngle::Front), "FRONT");
            button(panel, Action::Angle(ViewAngle::Side), "SIDE");
            button(panel, Action::Speed, "CYCLE SPEED");
            button(panel, Action::Pause, "PAUSE / PLAY");
            button(panel, Action::Step, "STEP +1 [RIGHT]");
            button(panel, Action::StepBack, "STEP -1 [LEFT]");
            button(panel, Action::Reset, "RESET LOOP");
            button(panel, Action::Back, "BACK TO MENU");
            panel.spawn((
                Status,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.66, 0.87, 0.88)),
            ));
        });
    commands
        .spawn((
            Root,
            SwordMetrics,
            GlobalZIndex(1002),
            Node {
                display: Display::Flex,
                position_type: PositionType::Absolute,
                right: px(20.0),
                top: px(18.0),
                width: px(315.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                padding: UiRect::all(px(14.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.05, 0.94)),
        ))
        .with_children(|panel| {
            panel.spawn((
                SwordMetricsTitle,
                Text::new(""),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                TextColor(Color::srgb(0.42, 0.92, 0.95)),
            ));
            panel.spawn((
                SwordAngleText,
                Text::new(""),
                TextFont { font_size: FontSize::Px(19.0), ..default() },
                TextColor(Color::srgb(0.85, 0.95, 0.94)),
            ));
            panel.spawn((
                SwordPhaseText,
                Text::new(""),
                TextFont { font_size: FontSize::Px(13.0), ..default() },
                TextColor(Color::srgb(0.66, 0.87, 0.88)),
            ));
        });
}

pub fn input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut viewer: ResMut<Viewer>,
    mut menu: ResMut<LaunchMenu>,
    roots: Query<Entity, With<Root>>,
    stages: Query<Entity, With<Stage>>,
    mut actions: Query<(&Action, Ref<Interaction>, &mut BackgroundColor)>,
) {
    if !viewer.active {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        viewer.paused = true;
        viewer.step_frame();
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        viewer.paused = true;
        viewer.step_back_frame();
    }
    for (action, interaction, _) in &mut actions {
        if !interaction.is_changed() || *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            Action::Clip(clip) => {
                viewer.clip = clip;
                viewer.reset_clock();
            }
            Action::Angle(angle) => viewer.angle = angle,
            Action::Speed => {
                viewer.speed = if viewer.speed < 0.75 {
                    1.0
                } else if viewer.speed < 1.25 {
                    1.5
                } else {
                    0.5
                }
            }
            Action::Pause => viewer.paused = !viewer.paused,
            Action::Step => {
                viewer.paused = true;
                viewer.step_frame();
            }
            Action::StepBack => {
                viewer.paused = true;
                viewer.step_back_frame();
            }
            Action::Reset => {
                viewer.paused = true;
                viewer.reset_clock();
            }
            Action::Back => {
                viewer.active = false;
                menu.open = true;
                for root in &roots {
                    commands.entity(root).despawn();
                }
                for stage in &stages {
                    commands.entity(stage).despawn();
                }
                return;
            }
        }
    }
    for (action, interaction, mut background) in &mut actions {
        let selected = match *action {
            Action::Clip(clip) => viewer.clip == clip,
            Action::Angle(angle) => viewer.angle == angle,
            _ => false,
        };
        background.0 = if selected {
            Color::srgba(0.09, 0.38, 0.43, 0.98)
        } else if *interaction == Interaction::Hovered {
            Color::srgba(0.12, 0.25, 0.29, 0.98)
        } else {
            Color::srgba(0.07, 0.16, 0.20, 0.92)
        };
    }
}

/// Set up in-place motion before the shared animation and trail systems.
pub fn drive(
    time: Res<Time>,
    library: Res<SceneLibrary>,
    mut viewer: ResMut<Viewer>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    mut marks: Query<(&GroundMark, &mut Transform), Without<Player>>,
    mut status: Query<&mut Text, With<Status>>,
) {
    if !viewer.active {
        return;
    }
    if !viewer.paused {
        viewer.advance_clock(time.delta_secs());
    }
    let t = viewer.phase();
    let spawn = Vec3::new(0.0, STUDIO_FLOOR_Y, 0.0);
    let (jump, height) = viewer.jump();
    player.translation = spawn + Vec3::Y * height;
    player.rotation = Quat::from_rotation_y(if viewer.clip == Clip::Turn {
        viewer.turn_heading()
    } else {
        0.0
    });
    let motion = library.scenes[library.current].scene.style.as_ref()
        .map_or_else(voxel_assets::CharacterMotion::default, |style| style.character_motion)
        ;
    let run_step = crate::character::step_distance_for_speed(motion.step_distance, 1.5);
    let gait_step = if matches!(viewer.clip, Clip::Run | Clip::Turn | Clip::RunJump) {
        run_step
    } else {
        motion.step_distance
    };
    let studio_speed = match viewer.clip {
        Clip::Walk => motion.step_distance * 2.0 * STUDIO_FPS / Clip::Walk.frame_count() as f32,
        Clip::Run => {
            run_step * 2.0 * STUDIO_FPS / Clip::Run.frame_count() as f32
        }
        Clip::Turn | Clip::RunJump => run_step * 2.0 * STUDIO_FPS / 20.0,
        _ => 0.0,
    };
    controls.velocity = player.rotation * Vec3::Z * studio_speed;
    // Apply every animation tick, even when a slow render skips several. This
    // keeps jump contacts and turning travel identical at a given frame ID.
    for tick in viewer.motion_frame + 1..=viewer.total_frame {
        let frame = tick % viewer.clip.frame_count();
        let sample = Viewer { frame, elapsed: frame as f32 / STUDIO_FPS, ..*viewer };
        let sample_rotation = Quat::from_rotation_y(if viewer.clip == Clip::Turn {
            sample.turn_heading()
        } else {
            0.0
        });
        let sample_velocity = sample_rotation * Vec3::Z * studio_speed;
        let stepping = !matches!(sample.jump().0, Some(JumpMotion::Rising(_) | JumpMotion::Falling(_)));
        viewer.advance_ground(1.0 / STUDIO_FPS, sample_velocity, gait_step, stepping);
        if frame == 0 {
            viewer.gait_distance = 0.0;
        }
    }
    viewer.motion_frame = viewer.total_frame;
    controls.gait.phase = viewer.advance_ground(0.0, Vec3::ZERO, gait_step, false);
    for (mark, mut transform) in &mut marks {
        match mark.axis {
            MarkAxis::X => transform.translation.x = wrapped_mark(mark.base, viewer.ground_offset.x),
            MarkAxis::Z => transform.translation.z = wrapped_mark(mark.base, viewer.ground_offset.y),
        }
    }
    controls.attack = viewer.clip.stroke().and_then(|stroke| {
        (t < 0.44).then(|| {
            let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
            attack.stroke = stroke;
            attack.swift = true;
            attack.age = t;
            attack
        })
    });
    for mut text in &mut status {
        text.0 = format!(
            "{} / F{}/{} / {:.1}x{}",
            viewer.clip.title(),
            viewer.frame,
            viewer.clip.frame_count(),
            viewer.speed,
            if viewer.paused { " PAUSED" } else { "" }
        );
    }
}

pub fn camera(
    viewer: Res<Viewer>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if !viewer.active {
        return;
    }
    let center = Vec3::new(0.0, STUDIO_FLOOR_Y + 1.0, 0.0);
    let offset = match viewer.angle {
        ViewAngle::ThreeQuarter => Vec3::new(3.0, 2.2, 4.4),
        ViewAngle::Front => Vec3::new(0.0, 1.5, 5.4),
        ViewAngle::Side => Vec3::new(5.4, 1.5, 0.0),
    };
    for (mut transform, mut projection) in &mut cameras {
        *transform = Transform::from_translation(center + offset).looking_at(center, Vec3::Y);
        if let Projection::Orthographic(ref mut ortho) = *projection {
            ortho.scaling_mode = ScalingMode::FixedVertical {
                viewport_height: 3.7,
            };
        }
    }
}

fn side_blade_angle(direction: Vec3) -> Option<f32> {
    let profile = Vec2::new(direction.z, direction.y);
    (profile.length_squared() > 0.0025).then(|| direction.y.atan2(direction.z.abs()).to_degrees())
}

pub fn metrics(
    viewer: Res<Viewer>,
    controls: Res<Controls>,
    sword: Query<&GlobalTransform, With<LocalBlade>>,
    mut panels: Query<&mut Node, With<SwordMetrics>>,
    mut titles: Query<&mut Text, (With<SwordMetricsTitle>, Without<SwordAngleText>, Without<SwordPhaseText>)>,
    mut readouts: Query<&mut Text, With<SwordAngleText>>,
    mut phase_texts: Query<&mut Text, (With<SwordPhaseText>, Without<SwordAngleText>)>,
    mut log: Local<AngleLog>,
) {
    if !viewer.active {
        return;
    }
    let Some(mut panel) = panels.iter_mut().next() else { return };
    panel.display = Display::Flex;
    if let Some(mut title) = titles.iter_mut().next() {
        let clip = match viewer.clip {
            Clip::Run => "RUN",
            Clip::Turn => "RUN + TURN",
            Clip::RunJump => "RUN + JUMP",
            Clip::Walk => "WALK",
            Clip::Idle => "IDLE",
            Clip::Jump => "JUMP",
            Clip::Forehand => "FOREHAND",
            Clip::Backhand => "BACKHAND",
            Clip::Overhead => "OVERHEAD",
        };
        title.0 = if viewer.angle == ViewAngle::Side {
            format!("{clip} / SWORD ANGLE")
        } else {
            format!("{clip} / ANIMATION")
        };
    }
    let Some(mut readout) = readouts.iter_mut().next() else { return };
    if viewer.angle != ViewAngle::Side {
        readout.0 = format!("FRAME {}/{}", viewer.frame, viewer.clip.frame_count());
        if let Some(mut phase_text) = phase_texts.iter_mut().next() {
            phase_text.0 = format!("TIME {:.3}s / 60 FPS", viewer.elapsed);
        }
        return;
    }
    let Some(sword) = sword.iter().next() else {
        readout.0 = "SWORD UNAVAILABLE".into();
        return;
    };
    let direction = sword.affine().transform_vector3(Vec3::Z).normalize_or_zero();
    let angle_degrees = side_blade_angle(direction);
    let angle = angle_degrees.map_or_else(
        || "EDGE ON / NO SIDE ANGLE".to_string(),
        |angle| {
            let pitch = if angle > 1.0 { "UP" } else if angle < -1.0 { "DOWN" } else { "LEVEL" };
            let heading = if direction.z > 0.05 { "FORWARD" } else if direction.z < -0.05 { "BACK" } else { "VERTICAL" };
            format!("{angle:+.1} DEG {pitch} / {heading}")
        },
    );
    readout.0 = angle;
    if let Some(mut phase_text) = phase_texts.iter_mut().next() {
        phase_text.0 = format!(
            "FRAME {}/{}  /  GAIT {:.0}%",
            viewer.frame,
            viewer.clip.frame_count(),
            controls.gait.phase / std::f32::consts::TAU * 100.0,
        );
    }
    if !log.initialized {
        log.initialized = true;
        log.enabled = std::env::args().any(|arg| arg == "--studio-angle-log");
    }
    if log.enabled && log.previous != Some((viewer.clip, viewer.total_frame)) {
        let phase = controls.gait.phase;
        if let Some(degrees) = angle_degrees {
            println!("ANGLE_FRAME,{:?},{},{},{:.6},{:.3},{:.3}", viewer.clip, viewer.frame, viewer.total_frame / viewer.clip.frame_count(), viewer.elapsed, phase / std::f32::consts::TAU * 100.0, degrees);
        }
        log.previous = Some((viewer.clip, viewer.total_frame));
    }
}

#[derive(Default)]
pub(crate) struct Capture {
    initialized: bool,
    path: Option<String>,
    gait_phase: Option<f32>,
    pause_at: Option<f32>,
    elapsed: f32,
    saved: bool,
}

/// `--capture-character-studio <path>` records the repeatable front-end view.
pub fn capture(
    mut commands: Commands,
    time: Res<Time>,
    mut viewer: ResMut<Viewer>,
    controls: Res<Controls>,
    mut state: Local<Capture>,
    mut exit: MessageWriter<AppExit>,
) {
    if !viewer.active {
        return;
    }
    if !state.initialized {
        state.initialized = true;
        let args: Vec<_> = std::env::args().collect();
        state.path = args
            .windows(2)
            .find(|pair| pair[0] == "--capture-character-studio")
            .map(|pair| pair[1].clone());
        state.gait_phase = args
            .windows(2)
            .find(|pair| pair[0] == "--studio-capture-gait")
            .and_then(|pair| pair[1].parse::<f32>().ok());
        state.pause_at = args
            .windows(2)
            .find(|pair| pair[0] == "--studio-pause-at")
            .and_then(|pair| pair[1].parse::<f32>().ok());
    }
    let Some(path) = state.path.clone() else {
        return;
    };
    if state.pause_at.is_some_and(|at| viewer.elapsed >= at) {
        viewer.paused = true;
    }
    state.elapsed += time.delta_secs().min(0.1);
    let phase = viewer.phase();
    let pushing = std::env::args().any(|arg| arg == "--studio-capture-push");
    let key_pose = match viewer.clip {
        Clip::Jump if pushing => (0.10..0.13).contains(&phase),
        Clip::RunJump if pushing => (0.65..0.68).contains(&phase),
        Clip::Jump => (0.40..0.48).contains(&phase),
        Clip::RunJump => (0.97..1.03).contains(&phase),
        Clip::Forehand | Clip::Backhand | Clip::Overhead => (0.17..0.21).contains(&phase),
        _ => true,
    };
    let gait_pose = state.gait_phase.is_none_or(|target| {
        let phase = controls.gait.phase / std::f32::consts::TAU;
        (phase - target).abs() < 0.02
    });
    if state.elapsed > 2.0 && key_pose && gait_pose && !state.saved {
        state.saved = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if state.elapsed > 3.5 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn side_blade_angle_uses_the_ground_as_zero() {
        assert_eq!(side_blade_angle(Vec3::Z), Some(0.0));
        assert_eq!(side_blade_angle(Vec3::Y), Some(90.0));
        assert_eq!(side_blade_angle(Vec3::NEG_Y), Some(-90.0));
        assert!((side_blade_angle(Vec3::new(1.0, 1.0, -1.0)).unwrap() - 45.0).abs() < 1e-5);
        assert_eq!(side_blade_angle(Vec3::X), None);
    }

    #[test]
    fn studio_clock_uses_fixed_frames_independent_of_render_cadence() {
        let mut twice = Viewer::default();
        twice.speed = 1.0;
        twice.advance_clock(1.0 / 120.0);
        assert_eq!(twice.frame, 0);
        twice.advance_clock(1.0 / 120.0);
        let mut once = Viewer::default();
        once.speed = 1.0;
        once.advance_clock(1.0 / 60.0);
        assert_eq!((twice.frame, twice.elapsed), (once.frame, once.elapsed));
        twice.step_frame();
        assert_eq!(twice.frame, 2);
        assert!((twice.elapsed - 2.0 / STUDIO_FPS).abs() < 1e-6);
        for _ in 0..28 {
            twice.step_frame();
        }
        assert_eq!(twice.frame, 0);
        assert_eq!(twice.elapsed, 0.0);
        assert_eq!(twice.total_frame, 0);
        twice.step_back_frame();
        assert_eq!(twice.frame, 29);
        twice.step_frame();
        assert_eq!(twice.frame, 0);
    }

    #[test]
    fn marked_floor_and_gait_use_the_same_travel_distance() {
        let mut viewer = Viewer::default();
        let phase = viewer.advance_ground(0.1, Vec3::Z * 5.6, 1.2, true);
        assert!((viewer.ground_offset.y - 0.56).abs() < 0.001);
        assert!((viewer.distance - 0.56).abs() < 0.001);
        assert!((phase - 0.56 * std::f32::consts::PI / 1.2).abs() < 0.001);
        assert!((wrapped_mark(0.5, viewer.ground_offset.y) + 0.06).abs() < 0.001);
        let paused_phase = viewer.advance_ground(0.0, Vec3::Z * 5.6, 1.2, true);
        assert_eq!(phase, paused_phase);
        assert!((viewer.distance - 0.56).abs() < 0.001);
    }

    #[test]
    fn run_jump_keeps_its_takeoff_gait_while_the_ground_moves_below() {
        let mut viewer = Viewer::default();
        let takeoff = viewer.advance_ground(0.1, Vec3::Z * 2.6, 0.44, true);
        let in_air = viewer.advance_ground(0.2, Vec3::Z * 2.6, 0.44, false);
        assert_eq!(in_air, takeoff);
        assert!((viewer.ground_offset.y - 0.78).abs() < 0.001);
        assert!((viewer.gait_distance - 0.26).abs() < 0.001);
    }

    #[test]
    fn jump_loop_has_takeoff_apex_and_landing() {
        let mut viewer = Viewer::default();
        viewer.clip = Clip::Jump;
        viewer.elapsed = 0.06;
        assert!(matches!(viewer.jump().0, Some(JumpMotion::Preparing(_))));
        viewer.elapsed = 0.11;
        assert!(matches!(viewer.jump().0, Some(JumpMotion::Pushing(_))));
        viewer.elapsed = 0.43;
        assert!(viewer.jump().1 > 0.5);
        viewer.elapsed = 0.82;
        assert!(matches!(viewer.jump().0, Some(JumpMotion::Landing(_))));
    }

    #[test]
    fn running_jump_has_stride_before_takeoff_and_after_landing() {
        let mut viewer = Viewer::default();
        viewer.clip = Clip::RunJump;
        viewer.elapsed = 0.4;
        assert!(viewer.jump().0.is_none());
        viewer.elapsed = 0.65;
        assert!(matches!(viewer.jump().0, Some(JumpMotion::Pushing(_))));
        viewer.elapsed = 0.98;
        assert!(viewer.jump().1 > 0.5);
        viewer.elapsed = 1.85;
        assert!(viewer.jump().0.is_none());
    }

    #[test]
    fn running_turn_loop_has_straights_and_two_direction_changes() {
        let mut viewer = Viewer::default();
        viewer.clip = Clip::Turn;
        viewer.elapsed = 0.3;
        assert_eq!(viewer.turn_heading(), 0.0);
        viewer.elapsed = 1.25;
        assert!(viewer.turn_heading() > 1.0);
        viewer.elapsed = 2.25;
        assert!(viewer.turn_heading() < -0.7);
        viewer.elapsed = 3.25;
        assert_eq!(viewer.turn_heading(), 0.0);
    }
}

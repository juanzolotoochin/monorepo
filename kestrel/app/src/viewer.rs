//! Character studio: repeatable poses from the production rig, viewed in place.
use crate::{
    assets::SceneLibrary,
    character_catalog::{CharacterEntry, CharacterSource},
    character::{JumpMotion, LocalBlade, Player},
    combat::Attack,
    gameplay::Controls,
};
use bevy::{
    app::AppExit,
    camera::ScalingMode,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
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
    const ALL: [Self; 9] = [
        Self::Idle, Self::Walk, Self::Run, Self::Turn, Self::Jump,
        Self::RunJump, Self::Forehand, Self::Backhand, Self::Overhead,
    ];
    pub(crate) fn supports(self, character: &CharacterEntry) -> bool {
        match self {
            Self::Walk | Self::Run | Self::Turn => character.can_locomote,
            Self::Jump | Self::RunJump => character.can_jump,
            Self::Forehand | Self::Backhand | Self::Overhead =>
                character.weapon == Some(content::scenario::Weapon::Sword),
            _ => true,
        }
    }
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "idle" => Self::Idle,
            "walk" => Self::Walk,
            "run" => Self::Run,
            "turn" => Self::Turn,
            "jump" => Self::Jump,
            "run-jump" => Self::RunJump,
            "forehand" => Self::Forehand,
            "backhand" => Self::Backhand,
            "overhead" => Self::Overhead,
            _ => return None,
        })
    }
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
            Self::Walk => "WALK",
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
    pub character: usize,
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

/// Studio selects only the pack containing the chosen rig. Gameplay scene
/// setup and simulation remain independent of this asset selection.
pub fn select_character_pack(viewer: Res<Viewer>, mut library: ResMut<SceneLibrary>) {
    if !viewer.active { return; }
    if let Some(entry) = library.characters.get(viewer.character) {
        library.current = entry.scene_index;
        library.requested = None;
    }
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
    Character(usize),
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
pub(crate) struct CharacterName;
#[derive(Component)]
pub(crate) struct CharacterPicker;
#[derive(Resource)]
struct StudioRig {
    character: usize,
    scale: f32,
    rig: crate::rig::Rig,
}
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

mod ui;
pub use ui::{input, setup};

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
    let motion = crate::character::motion_profile(&library.scenes[library.current]);
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

/// Preview NPCs with the same rig and pose function used by settlement actors.
pub fn pose_selected_rig(world: &mut World) {
    let viewer = *world.resource::<Viewer>();
    if !viewer.active && world.get_resource::<StudioRig>().is_none() { return; }
    let (scene_index, source) = {
        let library = world.resource::<SceneLibrary>();
        let Some(entry) = library.characters.get(viewer.character) else { return };
        (entry.scene_index, entry.source.clone())
    };
    let npc = viewer.active && scene_index == world.resource::<SceneLibrary>().current
        && matches!(source, CharacterSource::Skin(_));
    for mut visibility in world.query_filtered::<&mut Visibility, With<Player>>().iter_mut(world) {
        let desired = if npc { Visibility::Hidden } else { Visibility::Inherited };
        if *visibility != desired { *visibility = desired; }
    }
    let old = world.get_resource::<StudioRig>().map(|state| state.character);
    if (!npc || old != Some(viewer.character)) && old.is_some() {
        let state = world.remove_resource::<StudioRig>().unwrap();
        world.entity_mut(state.rig.root).despawn();
    }
    if !npc { return; }
    if world.get_resource::<StudioRig>().is_none() {
        let (profile, skin) = {
            let pack = &world.resource::<SceneLibrary>().scenes[scene_index];
            let skin = match &source {
                CharacterSource::Skin(id) => pack.skins.iter().find(|skin| &skin.id == id).cloned(),
                CharacterSource::BuiltIn | CharacterSource::Actor => None,
            };
            (pack.scene.style.clone(), skin)
        };
        let (Some(profile), Some(skin)) = (profile, skin) else { return };
        let template = crate::rig::RigTemplate::upload(world, &skin, &profile);
        let rig = template.spawn(world);
        world.insert_resource(StudioRig { character: viewer.character, scale: template.scale, rig });
    }
    let moving = matches!(viewer.clip, Clip::Walk | Clip::Run | Clip::Turn);
    let cycle = viewer.frame as f32 / viewer.clip.frame_count() as f32;
    let heading = if viewer.clip == Clip::Turn { viewer.turn_heading() } else { 0.0 };
    world.resource_scope(|world, mut state: Mut<StudioRig>| {
        let scale = state.scale;
        let travel = if moving { cycle * state.rig.cycle_distance(scale) } else { 0.0 };
        state.rig.pose(world, Vec3::new(0.0, STUDIO_FLOOR_Y, 0.0), heading,
            0.0, scale, travel, 0.0, 0.0, 0.0, None, Some(moving));
    });
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
    library: Res<SceneLibrary>,
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
    let has_sword = library.characters.get(viewer.character).is_some_and(|character|
        character.weapon == Some(content::scenario::Weapon::Sword));
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
        title.0 = if viewer.angle == ViewAngle::Side && has_sword {
            format!("{clip} / SWORD ANGLE")
        } else {
            format!("{clip} / ANIMATION")
        };
    }
    let Some(mut readout) = readouts.iter_mut().next() else { return };
    if viewer.angle != ViewAngle::Side || !has_sword {
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
    fn studio_discovers_clips_from_each_character_capability() {
        let library = SceneLibrary::new();
        for character in library.characters.iter() {
            assert!(Clip::Idle.supports(character));
            assert_eq!(Clip::Walk.supports(character), character.can_locomote);
            assert_eq!(Clip::Run.supports(character), character.can_locomote);
            assert_eq!(Clip::Turn.supports(character), character.can_locomote);
            assert_eq!(Clip::Jump.supports(character), character.can_jump);
            assert_eq!(Clip::RunJump.supports(character), character.can_jump);
            let sword = character.weapon == Some(content::scenario::Weapon::Sword);
            assert_eq!(Clip::Forehand.supports(character), sword);
            assert_eq!(Clip::Backhand.supports(character), sword);
            assert_eq!(Clip::Overhead.supports(character), sword);
        }
    }

    #[test]
    fn studio_character_action_selects_a_specific_character() {
        let mut library = SceneLibrary::new();
        let ox = library.characters.index_for_id("skin:ox").unwrap();
        let ghoul = library.characters.index_for_id("skin:ghoul").unwrap();
        library.current = library.characters[ox].scene_index;
        let next = library.characters[ghoul].scene_index;
        assert_eq!(library.current, next);
        let menu = crate::menu::LaunchMenu::new(false, &library);
        let mut viewer = Viewer::default();
        viewer.begin();
        viewer.character = ox;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(library)
            .insert_resource(menu)
            .insert_resource(viewer)
            .add_systems(Update, (input, select_character_pack).chain());
        app.world_mut().spawn((Action::Character(ghoul), Interaction::Pressed,
            BackgroundColor::default(), Node::default()));
        app.update();
        assert_eq!(app.world().resource::<SceneLibrary>().current, next);
        assert_eq!(app.world().resource::<SceneLibrary>().requested, None);
        assert_eq!(app.world().resource::<Viewer>().character, ghoul);
    }
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

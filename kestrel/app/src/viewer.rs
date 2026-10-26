//! Character studio: repeatable poses from the production rig, viewed in place.
//!
//! Studio is App flow, not a session (`flow::Front::Studio`): there is no
//! simulation and no world. It dresses the player's body in the selected
//! character's scene pack on a stage of its own, and everything it spawns
//! goes when it closes.
use crate::{
    assets::SceneLibrary,
    character_catalog::{CharacterEntry, CharacterSource},
    character::{JumpMotion, LocalBlade, Player},
    combat::Attack,
    flow::Front,
    gameplay::Controls,
};
use bevy::{
    app::AppExit,
    camera::ScalingMode,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    state::state_scoped::DespawnOnExit,
};
use service::{AppLayer, AppServices, Phase, Service};
use sim::settlement::avatar::SwordStroke;
use std::sync::Arc;
use voxel_assets::{Geometry, ScenePack};

// Rig roots use y=0 as their standing surface. Keep the inspection floor and
// root on the same plane so poses can be judged against actual foot contact.
const STUDIO_FLOOR_Y: f32 = 20.0;
const STUDIO_FLOOR_THICKNESS: f32 = 0.2;
const GRID_SPACING: f32 = 0.5;
const GRID_PERIOD: f32 = 4.0;
const STEP_HEIGHT: f32 = 0.24;
pub(crate) const STUDIO_FPS: f32 = 60.0;

fn step_span(step_distance: f32) -> (f32, f32) {
    (step_distance * 1.40, step_distance * 3.20)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clip {
    Idle,
    Walk,
    Step,
    #[default]
    Run,
    Turn,
    Jump,
    RunJump,
    Attack,
    Forehand,
    Backhand,
    Overhead,
}
impl Clip {
    const ALL: [Self; 11] = [
        Self::Idle, Self::Walk, Self::Step, Self::Run, Self::Turn, Self::Jump,
        Self::RunJump, Self::Attack, Self::Forehand, Self::Backhand, Self::Overhead,
    ];
    pub(crate) fn supports(self, character: &CharacterEntry) -> bool {
        match self {
            Self::Walk | Self::Run | Self::Turn => character.can_locomote,
            Self::Step => character.can_locomote
                && !matches!(character.source, CharacterSource::Skin(_)),
            Self::Jump | Self::RunJump => character.can_jump,
            Self::Attack => character.weapon.is_some_and(|weapon| weapon != content::scenario::Weapon::Sword),
            Self::Forehand | Self::Backhand | Self::Overhead =>
                character.weapon == Some(content::scenario::Weapon::Sword),
            _ => true,
        }
    }
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "idle" => Self::Idle,
            "walk" => Self::Walk,
            "step" => Self::Step,
            "run" => Self::Run,
            "turn" => Self::Turn,
            "jump" => Self::Jump,
            "run-jump" => Self::RunJump,
            "attack" => Self::Attack,
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
            Self::Step => 120,
            Self::Turn => 200,
            Self::Jump => 80,
            Self::RunJump => 135,
            Self::Attack | Self::Forehand | Self::Backhand | Self::Overhead => 66,
            Self::Idle => 90,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Walk => "WALK",
            Self::Step => "WALK / STEP UP + DOWN",
            Self::Run => "RUN",
            Self::Turn => "RUN + DIRECTION CHANGE",
            Self::Jump => "JUMP",
            Self::RunJump => "RUN + JUMP",
            Self::Attack => "WEAPON / ATTACK",
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
    fn preview_attack(self, character: &CharacterEntry, age: f32) -> Option<Attack> {
        if !self.supports(character) {
            return None;
        }
        let weapon = character.weapon?;
        let mut attack = Attack::new(Vec3::Z, weapon);
        if let Some(stroke) = self.stroke() {
            attack.stroke = stroke;
            attack.swift = true;
        } else if self != Self::Attack {
            return None;
        }
        if age >= attack.timing().2 {
            return None;
        }
        attack.age = age;
        Some(attack)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewAngle {
    #[default]
    ThreeQuarter,
    Front,
    Side,
}
/// Character Studio's playback (App layer). `active` while Studio is open.
#[derive(Resource, Default, Clone, Copy)]
pub struct Viewer {
    pub active: bool,
    /// `--character-studio`: open Studio as soon as the front end is up.
    open_on_start: bool,
    pub character: usize,
    /// A scene prefab being inspected in place of the character.
    pub prefab: Option<(usize, usize)>,
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
    step_distance: f32,
}
impl Viewer {
    pub fn begin(&mut self) {
        self.active = true;
        self.prefab = None;
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

impl Service for Viewer {
    type Layer = AppLayer;
    type Deps = ();

    fn init(world: &mut World) -> Self {
        Viewer::from_args(world.resource::<SceneLibrary>(), &std::env::args().collect::<Vec<_>>())
    }

    fn systems(app: &mut App) {
        let open = in_state(Front::Studio);
        app.add_systems(OnEnter(Front::Studio), enter)
            .add_systems(OnExit(Front::Studio), leave)
            .add_service_systems::<Self, _>(
                Phase::Input,
                (
                    open_on_start.run_if(in_state(Front::Menu)),
                    (input.run_if(crate::control::interactive), ui::scroll_picker, refresh_stage).chain().run_if(open.clone()),
                ),
            )
            .add_service_systems::<Self, _>(
                Phase::Present,
                (
                    (drive, crate::character::animate_character, crate::style::select_weapon, pose_selected_rig).chain(),
                    camera,
                    capture.run_if(crate::control::interactive),
                )
                    .chain()
                    .run_if(open.clone()),
            )
            .add_systems(
                PostUpdate,
                (metrics, crate::effects::blade_trail)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .run_if(crate::control::rendered)
                    .run_if(open),
            );
    }
}

impl Viewer {
    /// Studio as the command line asks for it: `--character-studio` opens
    /// it, `--studio-character`, `--studio-clip` and `--studio-angle` pick
    /// what it shows. `scenes.initial`'s character is the default.
    pub fn from_args(scenes: &SceneLibrary, args: &[String]) -> Self {
        let mut viewer = Viewer::default();
        if !args.iter().any(|arg| arg == "--character-studio") {
            return viewer;
        }
        let value = |flag: &str| args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].as_str());
        let initial = value("--scene").and_then(|id| scenes.index(id).ok()).unwrap_or(0);
        viewer.begin();
        viewer.open_on_start = true;
        viewer.character = value("--studio-character")
            .map(|id| scenes.characters.index_for_id(id).unwrap_or_else(|error| panic!("{error}")))
            .or_else(|| scenes.character(initial).and_then(|character| scenes.characters.index_for_id(&character.id).ok()))
            .or_else(|| (!scenes.characters.is_empty()).then_some(0))
            .expect("Character Studio needs at least one character");
        if let Some(clip) = value("--studio-clip") {
            viewer.clip = Clip::from_id(clip).unwrap_or(Clip::Run);
        }
        if !viewer.clip.supports(&scenes.characters[viewer.character]) {
            viewer.clip = if Clip::Run.supports(&scenes.characters[viewer.character]) { Clip::Run } else { Clip::Idle };
        }
        if let Some(angle) = value("--studio-angle") {
            viewer.angle = match angle {
                "front" => ViewAngle::Front,
                "side" => ViewAngle::Side,
                _ => ViewAngle::ThreeQuarter,
            };
        }
        viewer
    }

    /// Readies Studio for a scene's character, the menu's choice: its actor,
    /// or any character it brings. Returns the character, if it has one.
    pub fn open_for_scene(&mut self, scenes: &SceneLibrary, scene: usize) -> Option<usize> {
        let index = scenes
            .character(scene)
            .map(|character| character.scene_index)
            .or_else(|| scenes.characters.first().map(|character| character.scene_index))?;
        let character = scenes
            .characters
            .iter()
            .position(|entry| entry.scene_index == index && matches!(entry.source, CharacterSource::Actor))
            .or_else(|| scenes.characters.iter().position(|entry| entry.scene_index == index))?;
        self.begin();
        self.character = character;
        if !Clip::Run.supports(&scenes.characters[character]) {
            self.clip = Clip::Idle;
        }
        Some(character)
    }
}

/// The scene pack Studio dresses its character from, while open.
#[derive(Resource)]
pub struct StudioPack {
    /// The pack's index in the `SceneLibrary`.
    pub index: usize,
    pub pack: Arc<ScenePack>,
}

/// The Studio's copy of the player's body.
#[derive(Component)]
struct StudioBody;

/// `--character-studio` opens Studio once the menu is up.
fn open_on_start(mut viewer: ResMut<Viewer>, mut front: ResMut<NextState<Front>>) {
    if std::mem::take(&mut viewer.open_on_start) {
        front.set(Front::Studio);
    }
}

/// Opens Studio: the stage, its panels and the character. The HUD hides.
fn enter(world: &mut World) {
    world.resource_mut::<Viewer>().active = true;
    *world.resource_mut::<Controls>() = Controls::default();
    *world.resource_mut::<crate::effects::Feedback>() = default();
    for mut node in world.query_filtered::<&mut Node, With<crate::scene::HudRoot>>().iter_mut(world) {
        node.display = Display::None;
    }
    world.run_system_cached(setup).expect("Character Studio's stage");
    build_stage(world);
}

/// Closes Studio. Its entities go with `Front::Studio`; the rig preview and
/// the pack go here, and the HUD comes back for the next world.
fn leave(world: &mut World) {
    world.resource_mut::<Viewer>().active = false;
    world.remove_resource::<StudioPack>();
    clear_prefab(world);
    let bodies: Vec<_> = world.query_filtered::<Entity, With<StudioBody>>().iter(world).collect();
    for entity in bodies {
        world.despawn(entity);
    }
    if let Some(state) = world.remove_resource::<StudioRig>() {
        world.entity_mut(state.rig.root).despawn();
    }
    for mut node in world.query_filtered::<&mut Node, With<crate::scene::HudRoot>>().iter_mut(world) {
        node.display = Display::Flex;
    }
}

/// Dresses a fresh copy of the player's body in the selected character's
/// pack, the way a world would, but scoped to Studio.
fn build_stage(world: &mut World) {
    let viewer = *world.resource::<Viewer>();
    let Some(index) = world.resource::<SceneLibrary>().characters.get(viewer.character).map(|entry| entry.scene_index) else {
        return;
    };
    let old: Vec<_> = world.query_filtered::<Entity, With<StudioBody>>().iter(world).collect();
    for entity in old {
        world.despawn(entity);
    }
    let pack = world.resource::<SceneLibrary>().scenes[index].clone();
    let art = world.resource::<crate::scene::SceneArt>().clone();
    {
        let mut commands = world.commands();
        crate::scene::spawn_player(&mut commands, &art, &pack.scene);
    }
    world.flush();
    crate::style::apply(world, &pack, true);
    // The body is built for a world; no world is running, so it moves to
    // Studio's scope.
    let bodies: Vec<_> = world.query_filtered::<Entity, (With<Player>, With<service::WorldScoped>)>().iter(world).collect();
    for entity in bodies {
        world
            .entity_mut(entity)
            .remove::<service::WorldScoped>()
            .insert((StudioBody, DespawnOnExit(Front::Studio)));
    }
    world.insert_resource(StudioPack { index, pack });
}

/// Rebuilds the character when the selection moves to another pack.
fn refresh_stage(world: &mut World) {
    let viewer = *world.resource::<Viewer>();
    if let Some((scene, prefab)) = viewer.prefab {
        let current = world.get_resource::<StudioPrefab>().map(|state| (state.scene, state.prefab));
        if current != Some((scene, prefab)) {
            show_prefab(world, scene, prefab);
        }
        return;
    }
    clear_prefab(world);
    let wanted = world.resource::<SceneLibrary>().characters.get(viewer.character).map(|entry| entry.scene_index);
    if wanted.is_some() && wanted != world.get_resource::<StudioPack>().map(|studio| studio.index) {
        build_stage(world);
    }
}

#[derive(Component)]
pub(crate) struct Root;
#[derive(Component)]
pub(crate) struct Stage;
#[derive(Component)]
pub(crate) struct StepPlatform;
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

fn step_surface(distance: f32, step_distance: f32) -> f32 {
    let (start, end) = step_span(step_distance);
    if (start..end).contains(&distance) { STEP_HEIGHT } else { 0.0 }
}

fn studio_step_root_height(distance: f32, step_distance: f32) -> f32 {
    // The first foot lands on top after one stride. Let it take the body's
    // weight before rising; lower after the third contact reaches the floor.
    STEP_HEIGHT * crate::combat::smooth((distance / step_distance - 1.0) / 0.4)
        * (1.0 - crate::combat::smooth((distance / step_distance - 2.7) / 0.5))
}

pub(crate) fn studio_surface_height(viewer: &Viewer, local_z: f32) -> f32 {
    STUDIO_FLOOR_Y + if viewer.clip == Clip::Step {
        step_surface(viewer.distance + local_z, viewer.step_distance)
    } else {
        0.0
    }
}
#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Character(usize),
    Prefab(usize, usize),
    Clip(Clip),
    Angle(ViewAngle),
    Speed,
    Pause,
    Step,
    StepBack,
    Reset,
    ScrollUp,
    ScrollDown,
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
struct PrefabPreview;
#[derive(Resource)]
struct StudioPrefab {
    scene: usize,
    prefab: usize,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
}

fn clear_prefab(world: &mut World) {
    if let Some(preview) = world.remove_resource::<StudioPrefab>() {
        for entity in world.query_filtered::<Entity, With<PrefabPreview>>().iter(world).collect::<Vec<_>>() {
            world.despawn(entity);
        }
        for handle in preview.meshes {
            world.resource_mut::<Assets<Mesh>>().remove(&handle);
        }
        for handle in preview.materials {
            world.resource_mut::<Assets<StandardMaterial>>().remove(&handle);
        }
    }
}

fn show_prefab(world: &mut World, scene: usize, index: usize) {
    clear_prefab(world);
    let Some(prefab) = world.resource::<SceneLibrary>().scenes.get(scene)
        .and_then(|pack| pack.prefabs.get(index)).cloned() else { return };
    let model = match &prefab.geometry {
        Geometry::Baked(model) => model.clone(),
        Geometry::Runtime(recipe) => voxel_assets::generate_boulder(recipe)
            .expect("validated prefab recipe"),
    };
    let materials: Vec<_> = prefab.palette.materials.iter()
        .map(|material| world.resource_mut::<Assets<StandardMaterial>>().add(crate::assets::standard(material)))
        .collect();
    let mut meshes = Vec::new();
    for data in &model.meshes {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(crate::assets::mesh(data));
        world.spawn((
            PrefabPreview,
            DespawnOnExit(Front::Studio),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials[data.material].clone()),
            Transform::from_xyz(0.0, STUDIO_FLOOR_Y, 0.0),
        ));
        meshes.push(mesh);
    }
    world.insert_resource(StudioPrefab { scene, prefab: index, meshes, materials });
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
    studio: Option<Res<StudioPack>>,
    options: Res<crate::control::Options>,
    hub: Res<crate::control::Hub>,
    mut viewer: ResMut<Viewer>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    mut marks: Query<(&GroundMark, &mut Transform), (Without<Player>, Without<StepPlatform>)>,
    mut step_platform: Query<(&mut Transform, &mut Visibility), (With<StepPlatform>, Without<Player>, Without<GroundMark>)>,
    mut status: Query<&mut Text, With<Status>>,
) {
    if !viewer.active {
        return;
    }
    if options.studio_frames.is_some() {
        if hub.in_warmup() {
            viewer.seek_frame(0);
            viewer.paused = true;
        } else if hub.first_capture_tick() {
            viewer.seek_frame(0);
            viewer.paused = true;
        } else {
            viewer.paused = false;
        }
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
    let Some(studio) = studio else { return };
    let motion = crate::character::motion_profile(&studio.pack);
    let run_step = crate::character::step_distance_for_speed(motion.step_distance, 1.5);
    let gait_step = if matches!(viewer.clip, Clip::Run | Clip::Turn | Clip::RunJump) {
        run_step
    } else {
        motion.step_distance
    };
    viewer.step_distance = gait_step;
    let studio_speed = match viewer.clip {
        Clip::Walk => motion.step_distance * 2.0 * STUDIO_FPS / Clip::Walk.frame_count() as f32,
        Clip::Step => gait_step * 4.0 * STUDIO_FPS / Clip::Step.frame_count() as f32,
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
            if viewer.clip == Clip::Step {
                viewer.distance = 0.0;
                viewer.ground_offset = Vec2::ZERO;
            }
        }
    }
    viewer.motion_frame = viewer.total_frame;
    if viewer.clip == Clip::Step {
        player.translation.y += studio_step_root_height(viewer.distance, gait_step);
    }
    controls.gait.phase = viewer.advance_ground(0.0, Vec3::ZERO, gait_step, false);
    for (mark, mut transform) in &mut marks {
        match mark.axis {
            MarkAxis::X => transform.translation.x = wrapped_mark(mark.base, viewer.ground_offset.x),
            MarkAxis::Z => transform.translation.z = wrapped_mark(mark.base, viewer.ground_offset.y),
        }
    }
    for (mut transform, mut visibility) in &mut step_platform {
        *visibility = if viewer.clip == Clip::Step { Visibility::Inherited } else { Visibility::Hidden };
        let (start, end) = step_span(gait_step);
        transform.translation.z = (start + end) * 0.5 - viewer.distance;
        transform.scale.z = end - start;
    }
    if let Some(character) = library.characters.get(viewer.character) {
        controls.weapon = character.weapon.unwrap_or(content::scenario::Weapon::Sword);
        controls.attack = viewer.clip.preview_attack(character, t);
    }
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
    let npc = viewer.active && viewer.prefab.is_none() && Some(scene_index) == world.get_resource::<StudioPack>().map(|studio| studio.index)
        && matches!(source, CharacterSource::Skin(_));
    for mut visibility in world.query_filtered::<&mut Visibility, With<Player>>().iter_mut(world) {
        let desired = if npc || viewer.prefab.is_some() { Visibility::Hidden } else { Visibility::Inherited };
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
    let moving = matches!(viewer.clip, Clip::Walk | Clip::Step | Clip::Run | Clip::Turn);
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
            Clip::Step => "STEP UP + DOWN",
            Clip::Idle => "IDLE",
            Clip::Jump => "JUMP",
            Clip::Attack => "ATTACK",
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
    frame: Option<u64>,
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
        state.frame = args
            .windows(2)
            .find(|pair| pair[0] == "--studio-capture-frame")
            .and_then(|pair| pair[1].parse::<u64>().ok());
        if let Some(frame) = state.frame {
            viewer.seek_frame(frame);
            viewer.paused = true;
        }
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
        Clip::Attack | Clip::Forehand | Clip::Backhand | Clip::Overhead => (0.17..0.21).contains(&phase),
        _ => true,
    };
    let gait_pose = state.gait_phase.is_none_or(|target| {
        let phase = controls.gait.phase / std::f32::consts::TAU;
        (phase - target).abs() < 0.02
    });
    if state.elapsed > 2.0 && key_pose && gait_pose
        && state.pause_at.is_none_or(|at| viewer.elapsed >= at) && !state.saved {
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
    fn exosuit_step_keeps_stance_soles_planted_and_clears_the_block() {
        use crate::character::{grounded_foot_targets, PartKind, Pose};
        use std::collections::HashMap;

        let library = SceneLibrary::new();
        let exosuit = library.characters.index_for_id("builtin:exosuit").unwrap();
        let motion = crate::character::motion_profile(&library.scenes[library.characters[exosuit].scene_index]);
        assert!(motion.plant_feet);
        let mut world = World::new();
        let root_id = world.spawn_empty().id();
        let body_id = world.spawn_empty().id();
        let body_rest = Transform::from_xyz(0.0, 0.94, 0.0);
        let mut legs = Vec::new();
        for side in [-1.0_f32, 1.0] {
            legs.push((side, world.spawn_empty().id(), world.spawn_empty().id(), world.spawn_empty().id()));
        }
        let mut previous: [Option<f32>; 2] = [None, None];
        let mut previous_center: [Option<Vec3>; 2] = [None, None];
        for frame in 0..Clip::Step.frame_count() {
            let distance = 4.0 * motion.step_distance * frame as f32 / Clip::Step.frame_count() as f32;
            let root = Transform::from_xyz(0.0, STUDIO_FLOOR_Y +
                studio_step_root_height(distance, motion.step_distance), distance);
            let mut pose = Pose {
                stride: (distance * std::f32::consts::PI / motion.step_distance)
                    .rem_euclid(std::f32::consts::TAU),
                speed: (4.0 * motion.step_distance / 2.0) / motion.walk_speed,
                turn: 0.0, aim_yaw: 0.0, breathe: 0.0, step_pitch: 0.0, airborne: false,
                jump: None, attack: None, dig: None, motion,
            };
            let terrain = crate::character::foot_terrain(root, &pose,
                |point| Some(STUDIO_FLOOR_Y + step_surface(point.z, motion.step_distance)));
            pose.step_pitch = crate::character::step_body_pitch(&pose, terrain.ground);
            let body = pose.place(PartKind::Body, body_rest);
            let mut joints = HashMap::from([(body_id, (PartKind::Body, body_rest, body, Some(root_id)))]);
            for &(side, leg_id, shin_id, foot_id) in &legs {
                let leg_rest = Transform::from_xyz(side * 0.20, -0.36, 0.0);
                let shin_rest = Transform::from_xyz(0.0, -0.27, 0.0);
                let foot_rest = Transform::from_xyz(0.0, -0.21, 0.0);
                joints.insert(leg_id, (PartKind::Leg(side), leg_rest,
                    pose.place(PartKind::Leg(side), leg_rest), Some(body_id)));
                joints.insert(shin_id, (PartKind::Shin(side), shin_rest,
                    pose.place(PartKind::Shin(side), shin_rest), Some(leg_id)));
                joints.insert(foot_id, (PartKind::Foot(side), foot_rest,
                    pose.place(PartKind::Foot(side), foot_rest), Some(shin_id)));
            }
            for (entity, target) in grounded_foot_targets(&joints, root_id, &pose, terrain) {
                joints.get_mut(&entity).unwrap().2 = target;
            }
            for (index, &(side, leg_id, shin_id, foot_id)) in legs.iter().enumerate() {
                let sole = root.to_matrix() * body.to_matrix()
                    * joints[&leg_id].2.to_matrix() * joints[&shin_id].2.to_matrix()
                    * joints[&foot_id].2.to_matrix();
                let center = sole.transform_point3(Vec3::new(0.0, -0.09, 0.12));
                if let Some(last) = previous_center[index] {
                    assert!(center.distance(last) < 0.12,
                        "frame {frame} side {side} foot jumped {}m", center.distance(last));
                }
                previous_center[index] = Some(center);
                let phase = (pose.stride + if side > 0.0 { 0.0 } else { std::f32::consts::PI })
                    .rem_euclid(std::f32::consts::TAU);
                let stance = phase < std::f32::consts::TAU * 0.55;
                let surface = STUDIO_FLOOR_Y + step_surface(center.z, motion.step_distance);
                for edge_z in [-0.115, 0.355] {
                    let edge = sole.transform_point3(Vec3::new(0.0, -0.09, edge_z));
                    let edge_surface = STUDIO_FLOOR_Y + step_surface(edge.z, motion.step_distance);
                    assert!(edge.y >= edge_surface - 0.025,
                        "frame {frame} side {side} boot edge {edge_z} z={} y={} entered surface={edge_surface}", edge.z, edge.y);
                }
                if stance {
                    assert!((center.y - surface).abs() < 0.055,
                        "frame {frame} side {side} planted sole y={} surface={surface}", center.y);
                    if let Some(last) = previous[index] {
                        assert!((center.z - last).abs() < 0.01,
                            "frame {frame} side {side} stance sole slid from {last} to {}", center.z);
                    }
                    previous[index] = Some(center.z);
                } else {
                    assert!(center.y >= surface - 0.025,
                        "frame {frame} side {side} swing sole y={} entered surface={surface}", center.y);
                    previous[index] = None;
                }
            }
        }
    }

    #[test]
    fn step_preview_places_leading_and_trailing_feet_on_different_levels() {
        let mut viewer = Viewer::default();
        viewer.clip = Clip::Step;
        viewer.distance = 0.3;
        viewer.step_distance = 0.50;
        let pose = crate::character::Pose {
            motion: voxel_assets::CharacterMotion { step_distance: 0.50, ..default() },
            stride: 0.0,
            speed: 0.6,
            turn: 0.0,
            aim_yaw: 0.0,
            breathe: 0.0,
            step_pitch: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let root = Transform::from_xyz(0.0, STUDIO_FLOOR_Y + 0.12, 0.0);
        let offsets = crate::character::foot_terrain(root, &pose,
            |point| Some(studio_surface_height(&viewer, point.z))).ground;
        assert!(offsets[0] < -0.05, "trailing foot should stay on the lower surface");
        assert!(offsets[1] > 0.05, "leading foot should rise onto the step");
        assert_eq!(step_surface(0.0, viewer.step_distance), 0.0);
        assert_eq!(step_surface(1.2, viewer.step_distance), STEP_HEIGHT);
        assert_eq!(step_surface(4.0, viewer.step_distance), 0.0);
    }

    #[test]
    fn swinging_foot_changes_height_continuously_over_both_step_edges() {
        let mut motion = voxel_assets::CharacterMotion::default();
        motion.step_distance = 0.50;
        let (start, end) = step_span(motion.step_distance);
        let mut pose = crate::character::Pose {
            motion,
            stride: 0.0,
            speed: 0.6,
            turn: 0.0,
            aim_yaw: 0.0,
            breathe: 0.0,
            step_pitch: 0.0,
            airborne: false,
            jump: None,
            attack: None,
            dig: None,
        };
        let sample = |distance: f32, pose: &crate::character::Pose| {
            let root = Transform::from_xyz(0.0, STUDIO_FLOOR_Y, distance);
            crate::character::foot_terrain(root, pose,
                |point| Some(STUDIO_FLOOR_Y + step_surface(point.z, motion.step_distance))).ground[0]
        };
        let stance_end = std::f32::consts::TAU * 0.55;
        let swing_length = std::f32::consts::TAU - stance_end;
        let mut uphill = Vec::new();
        let mut downhill = Vec::new();
        for fraction in [0.0, 0.25, 0.5, 0.75, 0.999] {
            let left_phase = stance_end + swing_length * fraction;
            pose.stride = left_phase - std::f32::consts::PI;
            let first_swing_distance = pose.stride * motion.step_distance / std::f32::consts::PI;
            uphill.push(sample(first_swing_distance, &pose));
            downhill.push(sample(2.0 * motion.step_distance + first_swing_distance, &pose));
        }
        assert!(start > motion.step_distance * 0.55 + 0.36);
        assert!(end > motion.step_distance * 2.55);
        assert!(end < motion.step_distance * 3.55);
        assert!(uphill.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(downhill.windows(2).all(|pair| pair[0] >= pair[1]));
        assert!(uphill[1] > 0.0 && uphill[3] < STEP_HEIGHT);
        assert!(downhill[1] < STEP_HEIGHT && downhill[3] > 0.0);
    }

    #[test]
    fn studio_discovers_clips_from_each_character_capability() {
        let library = SceneLibrary::new();
        let static_skin = &library.characters[library.characters.index_for_id("skin:item_lumber").unwrap()];
        assert!(!static_skin.can_locomote);
        assert!(Clip::Idle.supports(static_skin));
        assert!(!Clip::Run.supports(static_skin));
        let vehicle = &library.characters[library.characters.index_for_id("skin:cart").unwrap()];
        assert!(vehicle.can_locomote);
        assert!(Clip::Run.supports(vehicle));
        for character in library.characters.iter() {
            assert!(Clip::Idle.supports(character));
            assert_eq!(Clip::Walk.supports(character), character.can_locomote);
            assert_eq!(Clip::Step.supports(character), character.can_locomote
                && !matches!(character.source, CharacterSource::Skin(_)));
            assert_eq!(Clip::Run.supports(character), character.can_locomote);
            assert_eq!(Clip::Turn.supports(character), character.can_locomote);
            assert_eq!(Clip::Jump.supports(character), character.can_jump);
            assert_eq!(Clip::RunJump.supports(character), character.can_jump);
            assert_eq!(Clip::Attack.supports(character), character.weapon.is_some_and(|weapon| weapon != content::scenario::Weapon::Sword));
            let sword = character.weapon == Some(content::scenario::Weapon::Sword);
            assert_eq!(Clip::Forehand.supports(character), sword);
            assert_eq!(Clip::Backhand.supports(character), sword);
            assert_eq!(Clip::Overhead.supports(character), sword);
        }
    }

    #[test]
    fn actor_with_partial_visual_style_uses_player_rig_and_weapon_action() {
        let library = SceneLibrary::new();
        let character = &library.characters[library.characters.index_for_id("laser-pit").unwrap()];
        assert_eq!(character.source, CharacterSource::Actor);
        assert!(Clip::Walk.supports(character));
        assert!(Clip::Run.supports(character));
        assert!(Clip::Step.supports(character));
        assert!(!Clip::Jump.supports(character), "the laser pit's exosuit flies");
        assert!(Clip::Attack.supports(character));
        assert!(!Clip::Forehand.supports(character));
        let attack = Clip::Attack.preview_attack(character, 0.06).unwrap();
        assert_eq!(attack.weapon, content::scenario::Weapon::Laser);
        assert_eq!(attack.motion(), crate::combat::ActionMotion::Cast);
    }

    #[test]
    fn studio_character_action_selects_a_specific_character() {
        // The ox and the ghoul share a scene pack; picking one picks that rig.
        let library = SceneLibrary::new();
        let ox = library.characters.index_for_id("skin:ox").unwrap();
        let ghoul = library.characters.index_for_id("skin:ghoul").unwrap();
        assert_eq!(library.characters[ox].scene_index, library.characters[ghoul].scene_index);
        let mut viewer = Viewer::default();
        viewer.begin();
        viewer.character = ox;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<debug_ui::GameInput>()
            .init_resource::<NextState<Front>>()
            .insert_resource(library)
            .insert_resource(viewer)
            .add_systems(Update, input);
        app.world_mut().spawn((Action::Character(ghoul), Interaction::Pressed,
            BackgroundColor::default(), Node::default()));
        app.update();
        assert_eq!(app.world().resource::<Viewer>().character, ghoul);
        assert!(matches!(app.world().resource::<NextState<Front>>(), NextState::Unchanged), "Studio stays open");
    }

    #[test]
    fn studio_picker_buttons_reach_entries_below_the_viewport() {
        let mut viewer = Viewer::default();
        viewer.begin();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<debug_ui::GameInput>()
            .init_resource::<NextState<Front>>()
            .insert_resource(SceneLibrary::new())
            .insert_resource(viewer)
            .add_systems(Update, input);
        let picker = app.world_mut().spawn((
            CharacterPicker,
            ComputedNode {
                size: Vec2::new(350.0, 600.0),
                content_size: Vec2::new(350.0, 2000.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            ScrollPosition::default(),
        )).id();
        app.world_mut().spawn((Action::ScrollDown, Interaction::Pressed,
            BackgroundColor::default(), Node::default()));
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(picker).unwrap().y, 480.0);
    }

    #[test]
    fn studio_builds_its_character_outside_a_session_and_leaves_nothing_behind() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let library = SceneLibrary::new();
        let ox = library.characters.index_for_id("skin:ox").unwrap();
        let glade = library.index("moonlit-glade").unwrap();
        let mut app = crate::test_front_app(options, library, hub, None);
        for _ in 0..3 {
            app.update();
        }
        let players = |app: &mut App| app.world_mut().query_filtered::<(), With<Player>>().iter(app.world()).count();
        let hud_shown = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&Node, With<crate::scene::HudRoot>>()
                .iter(app.world())
                .all(|node| node.display != Display::None)
        };
        assert_eq!(players(&mut app), 0, "no body without a world");

        let opened = app.world_mut().resource_mut::<Viewer>().open_for_scene(&SceneLibrary::new(), glade);
        assert!(opened.is_some());
        app.world_mut().resource_mut::<NextState<Front>>().set(Front::Studio);
        app.update();
        assert!(app.world().resource::<Viewer>().active);
        assert_eq!(players(&mut app), 1);
        assert_eq!(app.world().resource::<StudioPack>().index, glade);
        assert!(!hud_shown(&mut app));
        assert_eq!(*app.world().resource::<State<service::SessionState>>().get(), service::SessionState::Off, "Studio is no session");

        // Picking a character from another pack rebuilds the body.
        app.world_mut().resource_mut::<Viewer>().character = ox;
        app.update();
        let ox_pack = SceneLibrary::new().characters[ox].scene_index;
        assert_eq!(app.world().resource::<StudioPack>().index, ox_pack);
        assert_eq!(players(&mut app), 1);
        assert!(app.world().get_resource::<StudioRig>().is_some(), "the ox poses through its rig");

        let (prefab_scene, prefab_index) = app.world().resource::<SceneLibrary>().scenes.iter()
            .enumerate()
            .find_map(|(scene, pack)| pack.prefabs.iter().position(|prefab|
                matches!(prefab.geometry, Geometry::Runtime(_))).map(|index| (scene, index)))
            .expect("a runtime boulder is available for inspection");
        app.world_mut().resource_mut::<Viewer>().prefab = Some((prefab_scene, prefab_index));
        app.update();
        assert!(app.world().get_resource::<StudioPrefab>().is_some());
        assert!(app.world_mut().query_filtered::<(), With<PrefabPreview>>().iter(app.world()).count() > 0);
        assert!(app.world_mut().query_filtered::<&Visibility, With<Player>>().iter(app.world())
            .all(|visibility| *visibility == Visibility::Hidden));
        app.world_mut().resource_mut::<Viewer>().prefab = None;
        app.update();
        assert!(app.world().get_resource::<StudioPrefab>().is_none());
        assert_eq!(app.world_mut().query_filtered::<(), With<PrefabPreview>>().iter(app.world()).count(), 0);

        app.world_mut().resource_mut::<NextState<Front>>().set(Front::Menu);
        app.update();
        assert!(!app.world().resource::<Viewer>().active);
        assert_eq!(players(&mut app), 0);
        assert!(app.world().get_resource::<StudioPack>().is_none());
        assert!(app.world().get_resource::<StudioRig>().is_none());
        assert_eq!(app.world_mut().query_filtered::<(), Or<(With<Root>, With<Stage>)>>().iter(app.world()).count(), 0);
        assert!(hud_shown(&mut app));
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

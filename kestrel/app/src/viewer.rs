//! Character studio: repeatable poses from the production rig, viewed in place.
use crate::{
    character::{JumpMotion, Player},
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clip {
    Idle,
    #[default]
    Run,
    Turn,
    Jump,
    Forehand,
    Backhand,
    Overhead,
}
impl Clip {
    fn title(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Run => "RUN",
            Self::Turn => "TURN",
            Self::Jump => "JUMP",
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
#[derive(Resource, Default)]
pub struct Viewer {
    pub active: bool,
    pub clip: Clip,
    pub angle: ViewAngle,
    pub elapsed: f32,
    pub speed: f32,
    pub paused: bool,
}
impl Viewer {
    pub fn begin(&mut self) {
        self.active = true;
        self.clip = Clip::Run;
        self.angle = ViewAngle::ThreeQuarter;
        self.elapsed = 0.0;
        self.speed = 1.0;
        self.paused = false;
    }
    fn phase(&self) -> f32 {
        let period = if self.clip.stroke().is_some() {
            1.10
        } else {
            1.5
        };
        self.elapsed.rem_euclid(period)
    }
    pub fn jump(&self) -> (Option<JumpMotion>, f32) {
        if self.clip != Clip::Jump {
            return (None, 0.0);
        }
        let t = self.phase();
        if t < 0.12 {
            return (Some(JumpMotion::Preparing(t / 0.12)), 0.0);
        }
        if t < 0.84 {
            let p = ((t - 0.12) / 0.72).clamp(0.0, 1.0);
            let height = 1.3 * 4.0 * p * (1.0 - p);
            return (
                Some(if p < 0.5 {
                    JumpMotion::Rising(p * 2.0)
                } else {
                    JumpMotion::Falling((p - 0.5) * 2.0)
                }),
                height,
            );
        }
        if t < 1.06 {
            return (Some(JumpMotion::Landing((t - 0.84) / 0.22)), 0.0);
        }
        (None, 0.0)
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
#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Clip(Clip),
    Angle(ViewAngle),
    Speed,
    Pause,
    Step,
    Reset,
    Back,
}
#[derive(Component)]
pub(crate) struct Status;

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
        Mesh3d(meshes.add(Cuboid::new(4.0, 0.2, 4.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.09, 0.17, 0.21),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, 19.75, 0.0),
    ));
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
            for clip in [
                Clip::Idle,
                Clip::Run,
                Clip::Turn,
                Clip::Jump,
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
            button(panel, Action::Step, "STEP ONE FRAME");
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
}

pub fn input(
    mut commands: Commands,
    mut viewer: ResMut<Viewer>,
    mut menu: ResMut<LaunchMenu>,
    roots: Query<Entity, With<Root>>,
    stages: Query<Entity, With<Stage>>,
    mut actions: Query<(&Action, Ref<Interaction>, &mut BackgroundColor)>,
    mut status: Query<&mut Text, With<Status>>,
) {
    if !viewer.active {
        return;
    }
    for (action, interaction, _) in &mut actions {
        if !interaction.is_changed() || *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            Action::Clip(clip) => {
                viewer.clip = clip;
                viewer.elapsed = 0.0;
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
                viewer.elapsed += 1.0 / 60.0;
            }
            Action::Reset => {
                viewer.paused = true;
                viewer.elapsed = 0.0;
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
    for mut text in &mut status {
        text.0 = format!(
            "{}   /   {:.1}x   /   {:.2}s{}",
            viewer.clip.title(),
            viewer.speed,
            viewer.phase(),
            if viewer.paused { "   PAUSED" } else { "" }
        );
    }
}

/// Set up in-place motion before the shared animation and trail systems.
pub fn drive(
    time: Res<Time>,
    mut viewer: ResMut<Viewer>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    if !viewer.active {
        return;
    }
    if !viewer.paused {
        viewer.elapsed += time.delta_secs().min(0.05) * viewer.speed;
    }
    let t = viewer.phase();
    let spawn = Vec3::new(0.0, 20.0, 0.0);
    let (_, height) = viewer.jump();
    player.translation = spawn + Vec3::Y * height;
    player.rotation = Quat::from_rotation_y(if viewer.clip == Clip::Turn {
        (viewer.elapsed * 2.4).sin() * 0.9
    } else {
        0.0
    });
    controls.velocity = if matches!(viewer.clip, Clip::Run | Clip::Turn) {
        Vec3::Z * 5.6
    } else {
        Vec3::ZERO
    };
    controls.gait.phase = viewer.elapsed * if viewer.clip == Clip::Run { 12.0 } else { 9.0 };
    controls.attack = viewer.clip.stroke().and_then(|stroke| {
        (t < 0.44).then(|| {
            let mut attack = Attack::new(Vec3::Z, content::scenario::Weapon::Sword);
            attack.stroke = stroke;
            attack.swift = true;
            attack.age = t;
            attack
        })
    });
}

pub fn camera(
    viewer: Res<Viewer>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if !viewer.active {
        return;
    }
    let center = Vec3::new(0.0, 21.0, 0.0);
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

#[derive(Default)]
pub(crate) struct Capture {
    initialized: bool,
    path: Option<String>,
    elapsed: f32,
    saved: bool,
}

/// `--capture-character-studio <path>` records the repeatable front-end view.
pub fn capture(
    mut commands: Commands,
    time: Res<Time>,
    viewer: Res<Viewer>,
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
    }
    let Some(path) = state.path.clone() else {
        return;
    };
    state.elapsed += time.delta_secs().min(0.1);
    let phase = viewer.phase();
    let key_pose = match viewer.clip {
        Clip::Jump => (0.43..0.53).contains(&phase),
        Clip::Forehand | Clip::Backhand | Clip::Overhead => (0.17..0.21).contains(&phase),
        _ => true,
    };
    if state.elapsed > 2.0 && key_pose && !state.saved {
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
    fn jump_loop_has_takeoff_apex_and_landing() {
        let mut viewer = Viewer::default();
        viewer.clip = Clip::Jump;
        viewer.elapsed = 0.06;
        assert!(matches!(viewer.jump().0, Some(JumpMotion::Preparing(_))));
        viewer.elapsed = 0.48;
        assert!(viewer.jump().1 > 1.2);
        viewer.elapsed = 0.92;
        assert!(matches!(viewer.jump().0, Some(JumpMotion::Landing(_))));
    }
}

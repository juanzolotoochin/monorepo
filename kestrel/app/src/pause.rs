//! The pause overlay (Session layer): Escape, or Menu/Start on the seated
//! controller, opens it over a session; so do losing window focus and the
//! seated controller disconnecting. Lockstep can't stop the clock for one
//! player, so the world keeps running; the overlay only takes the player's
//! input. Opening it suspends direct control (`input::Suspend::Pause`):
//! the avatar is released and nothing held resumes when it closes. Players'
//! runs only (`Options::menus`).
// `Interaction`'s replacement (`Hovered`, `Pressed`) needs bevy_picking,
// which the build leaves out.
#![allow(deprecated)]

use bevy::prelude::*;
use debug_ui::GameInput;
use service::{AppServices, Phase, Service, SessionLayer, SessionScoped};

use crate::flow::Flow;
use crate::input::{InputService, Nav, Suspend};
use crate::menu::{button, font, highlight, BACKDROP, MUTED, TEXT};

#[derive(Resource, Default)]
pub struct Pause {
    pub open: bool,
    /// The controller's focus among the buttons.
    pub focus: usize,
}

impl Service for Pause {
    type Layer = SessionLayer;
    type Deps = (Flow,);

    fn init(_world: &mut World) -> Self {
        Pause::default()
    }

    /// Registered before the other Session services, so that while open it
    /// takes the input before anything else in the session reads it.
    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Input, (toggle, navigate, press, show, swallow).chain());
    }
}

/// What an overlay button does.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Resume,
    Leave,
    Quit,
}

const CHOICES: [Choice; 3] = [Choice::Resume, Choice::Leave, Choice::Quit];

#[derive(Component)]
#[require(SessionScoped)]
struct Overlay;

fn toggle(game: Res<GameInput>, mut input: ResMut<InputService>, mut pause: ResMut<Pause>) {
    let escape = game.keys().just_pressed(KeyCode::Escape) && !input.escape_taken();
    let menu = input.take_menu();
    let was_open = pause.open;
    if escape || menu {
        pause.open = !pause.open;
    }
    if input.frame().pause_requested {
        pause.open = true;
    }
    if pause.open && !was_open {
        pause.focus = 0;
        input.suspend(Suspend::Pause);
    }
}

/// The controller moves the focus, confirms, or goes back (resumes).
fn navigate(mut input: ResMut<InputService>, mut pause: ResMut<Pause>, mut flow: ResMut<Flow>, mut exit: MessageWriter<AppExit>) {
    if !pause.open {
        return;
    }
    match input.take_nav() {
        Some(Nav::Up) => pause.focus = (pause.focus + CHOICES.len() - 1) % CHOICES.len(),
        Some(Nav::Down) => pause.focus = (pause.focus + 1) % CHOICES.len(),
        _ => {}
    }
    if input.take_back() {
        pause.open = false;
        return;
    }
    if input.take_confirm() {
        choose(CHOICES[pause.focus], &mut pause, &mut flow, &mut exit);
    }
}

fn choose(choice: Choice, pause: &mut Pause, flow: &mut Flow, exit: &mut MessageWriter<AppExit>) {
    pause.open = false;
    match choice {
        Choice::Resume => {}
        Choice::Leave => flow.leave(None),
        Choice::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

fn press(
    buttons: Query<(&Interaction, &Choice), Changed<Interaction>>,
    mut pause: ResMut<Pause>,
    mut flow: ResMut<Flow>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, choice) in &buttons {
        if *interaction == Interaction::Pressed {
            choose(*choice, &mut pause, &mut flow, &mut exit);
        }
    }
}

/// Builds the overlay while open and removes it once closed.
fn show(
    mut commands: Commands,
    pause: Res<Pause>,
    overlays: Query<Entity, With<Overlay>>,
    mut buttons: Query<(&Interaction, &Choice, &mut BackgroundColor, &mut BorderColor)>,
) {
    if !pause.open {
        for overlay in &overlays {
            commands.entity(overlay).despawn();
        }
        return;
    }
    for (interaction, choice, mut background, mut border) in &mut buttons {
        highlight(*interaction, CHOICES[pause.focus] == *choice, &mut background, &mut border);
    }
    if !overlays.is_empty() {
        return;
    }
    let choice = |choice: Choice, text: &str| (button(choice, 220.0, JustifyContent::Center), children![(Text::new(text), font(16.0), TextColor(TEXT))]);
    commands.spawn((
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        GlobalZIndex(60),
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(12),
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(BACKDROP),
            children![
                (Text::new("PAUSED  /  THE WORLD KEEPS RUNNING"), font(11.0), TextColor(MUTED), Node { margin: UiRect::bottom(px(8)), ..default() }),
                choice(Choice::Resume, "RESUME"),
                choice(Choice::Leave, "LEAVE TO MENU"),
                choice(Choice::Quit, "QUIT GAME"),
                (Text::new("Controller: D-pad to choose / A confirm / B resume"), font(11.0), TextColor(MUTED), Node { margin: UiRect::top(px(8)), ..default() }),
            ],
        )],
    ));
}

/// While open, nothing else in the session sees the keyboard, the mouse
/// buttons, the wheel, or controller gameplay. Held keys count as released:
/// gameplay sees each release once, and they stay off until pressed again
/// after the overlay closes. Bevy's own input, which the overlay's buttons
/// and the debug UI read, is left alone.
fn swallow(pause: Res<Pause>, mut game: ResMut<GameInput>, mut input: ResMut<InputService>) {
    if pause.open {
        game.block();
        input.block();
    }
}

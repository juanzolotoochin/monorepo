//! The pause overlay (Session layer): Escape opens it over a session.
//! Lockstep can't stop the clock for one player, so the world keeps
//! running; the overlay only takes the player's input. Players' runs only
//! (`Options::menus`).
// `Interaction`'s replacement (`Hovered`, `Pressed`) needs bevy_picking,
// which the build leaves out.
#![allow(deprecated)]

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use service::{AppServices, Phase, Service, SessionLayer, SessionScoped};

use crate::flow::Flow;
use crate::menu::{button, font, highlight, BACKDROP, MUTED, TEXT};

#[derive(Resource, Default)]
pub struct Pause {
    pub open: bool,
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
        app.add_service_systems::<Self, _>(Phase::Input, (toggle, press, show, swallow).chain());
    }
}

/// What an overlay button does.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    Resume,
    Leave,
    Quit,
}

#[derive(Component)]
#[require(SessionScoped)]
struct Overlay;

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut pause: ResMut<Pause>) {
    if keys.just_pressed(KeyCode::Escape) {
        pause.open = !pause.open;
    }
}

fn press(
    buttons: Query<(&Interaction, &Choice), Changed<Interaction>>,
    mut pause: ResMut<Pause>,
    mut flow: ResMut<Flow>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, choice) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        pause.open = false;
        match choice {
            Choice::Resume => {}
            Choice::Leave => flow.leave(None),
            Choice::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Builds the overlay while open and removes it once closed.
fn show(
    mut commands: Commands,
    pause: Res<Pause>,
    overlays: Query<Entity, With<Overlay>>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor, &mut BorderColor), With<Choice>>,
) {
    if !pause.open {
        for overlay in &overlays {
            commands.entity(overlay).despawn();
        }
        return;
    }
    for (interaction, mut background, mut border) in &mut buttons {
        highlight(*interaction, false, &mut background, &mut border);
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
            ],
        )],
    ));
}

/// While open, nothing else in the session sees the keyboard, the mouse
/// buttons or the wheel. Held keys count as released.
fn swallow(
    pause: Res<Pause>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut scroll: ResMut<AccumulatedMouseScroll>,
) {
    if pause.open {
        keys.reset_all();
        mouse.reset_all();
        scroll.delta = Vec2::ZERO;
    }
}

//! The interactive egui backend (bevy_egui), driven by hand so the pass runs
//! in `Update` before gameplay reads input, not in `PostUpdate`.
//!
//! - PreUpdate, after bevy_egui has translated the devices into the
//!   context's `EguiInput`: the context is made manual, `/` and the closing
//!   Escape are handled, and while hidden the translated input is reduced to
//!   releases so nothing replays when the overlay shows again.
//! - Update, [`DebugUiSystems::Pass`]: exactly one `run_ui` per frame, with
//!   the menu bar and providers only while visible (an empty pass while
//!   hidden, so the previous frame's windows don't stay painted). The pass
//!   then measures what it captured for routing.
//! - Update, [`DebugUiSystems::Route`]: Bevy UI nodes under the debug UI
//!   don't see the pointer.
//! - PostUpdate: bevy_egui tessellates and renders our output; a check
//!   confirms nobody ran a second pass.

use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use bevy_egui::{
    egui, EguiContext, EguiContextSettings, EguiFullOutput, EguiGlobalSettings, EguiInput, EguiPlugin,
    EguiPostUpdateSet, EguiPreUpdateSet, PrimaryEguiContext,
};

use crate::input::{Capture, CaptureSnapshot, GameInput};
use crate::{DebugUi, DebugUiSystems, TextFocus};

pub(crate) fn install(app: &mut App) {
    app.add_plugins(EguiPlugin::default())
        // The shared-input absorber clears Bevy's input resources and
        // message buffers; routing (`GameInput`) does that job instead.
        .insert_resource(EguiGlobalSettings { enable_absorb_bevy_input_system: false, ..default() })
        .add_systems(
            PreUpdate,
            (manual_context, prepare).chain().after(EguiPreUpdateSet::ProcessInput).before(EguiPreUpdateSet::BeginPass),
        )
        .add_systems(Update, pass.in_set(DebugUiSystems::Pass))
        .add_systems(Update, guard_bevy_ui.in_set(DebugUiSystems::Route).after(crate::input::route))
        .add_systems(PostUpdate, check_single_pass.after(EguiPostUpdateSet::EndPass).before(EguiPostUpdateSet::ProcessOutput));
}

/// Hands the pass to us, and while hidden keeps bevy_egui from changing the
/// cursor or IME. The first hidden frame still lets it apply the empty
/// pass's default cursor and IME off.
fn manual_context(state: Res<DebugUi>, mut settings: Query<&mut EguiContextSettings, With<PrimaryEguiContext>>) {
    let updates = state.visible || state.hidden_passes == 0;
    for mut settings in &mut settings {
        if !settings.run_manually {
            settings.run_manually = true;
        }
        if settings.enable_cursor_icon_updates != updates {
            settings.enable_cursor_icon_updates = updates;
        }
        if settings.enable_ime != updates {
            settings.enable_ime = updates;
        }
    }
}

fn is_slash(message: &KeyboardInput) -> bool {
    matches!(&message.logical_key, Key::Character(c) if c.as_str() == "/")
}

fn is_slash_event(event: &egui::Event) -> bool {
    match event {
        egui::Event::Text(text) => text == "/",
        egui::Event::Key { key, .. } => *key == egui::Key::Slash,
        _ => false,
    }
}

/// `/` shows or hides the overlay unless a text field has the keyboard;
/// Escape while editing debug text closes it. Either key belongs to the
/// debug UI for its whole press. While hidden the context only learns about
/// releases (so it never thinks a button is stuck down).
#[allow(clippy::too_many_arguments)]
fn prepare(
    mut keyboard: MessageReader<KeyboardInput>,
    mut focus: MessageReader<KeyboardFocusLost>,
    mut state: ResMut<DebugUi>,
    text_focus: Res<TextFocus>,
    mut input: ResMut<GameInput>,
    mut contexts: Query<(&mut EguiContext, &mut EguiInput), With<PrimaryEguiContext>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    let Ok((mut context, mut raw)) = contexts.single_mut() else {
        keyboard.clear();
        focus.clear();
        return;
    };
    let ctx = context.get_mut().clone();
    let was_visible = state.visible;
    // From the last pass: a debug text field has the keyboard.
    let editing = was_visible && ctx.text_edit_focused();
    let mut toggled = false;
    for message in keyboard.read() {
        if message.state != ButtonState::Pressed || message.repeat {
            continue;
        }
        if is_slash(message) && !editing && !text_focus.external {
            state.visible = !state.visible;
            toggled = true;
            input.claim(message.key_code);
        } else if message.key_code == KeyCode::Escape && editing && state.visible {
            state.visible = false;
            input.claim(KeyCode::Escape);
        }
    }
    if toggled {
        raw.events.retain(|event| !is_slash_event(event));
    }
    let focus_lost = focus.read().count() > 0;
    let hiding = was_visible && !state.visible;
    if focus_lost || hiding {
        ctx.memory_mut(|memory| memory.stop_text_input());
        raw.events.push(egui::Event::PointerGone);
    }
    if !state.visible {
        raw.events.retain(|event| {
            matches!(
                event,
                egui::Event::PointerButton { pressed: false, .. }
                    | egui::Event::Key { pressed: false, .. }
                    | egui::Event::PointerGone
                    | egui::Event::ModifiersChanged(_)
            )
        });
        // Window focus must not give the hidden UI focus back.
        raw.focused = false;
        raw.hovered_files.clear();
        raw.dropped_files.clear();
    }
    if hiding {
        if let Ok(window) = windows.single() {
            commands.entity(window).try_insert(CursorIcon::System(SystemCursorIcon::Default));
        }
    }
}

/// The frame's one UI pass, then what it captured.
fn pass(world: &mut World) {
    let mut contexts = world.query_filtered::<(Entity, &mut EguiContext, &mut EguiInput), With<PrimaryEguiContext>>();
    let Some((entity, ctx, input)) = contexts.iter_mut(world).next().map(|(entity, mut ctx, mut input)| (entity, ctx.get_mut().clone(), input.take())) else {
        world.resource_mut::<CaptureSnapshot>().0 = Capture::default();
        return;
    };
    let visible = world.resource::<DebugUi>().visible;
    let output = ctx.run_ui(input, |ui| {
        if visible {
            crate::menu::draw(world, ui);
        }
    });
    if let Some(mut full) = world.get_mut::<EguiFullOutput>(entity) {
        full.0 = Some(output);
    }
    let capture = if visible {
        Capture {
            keyboard: ctx.egui_wants_keyboard_input(),
            pointer: ctx.is_pointer_over_egui() || ctx.egui_is_using_pointer() || ctx.any_popup_open(),
            pointer_interrupt: ctx.egui_is_using_pointer(),
        }
    } else {
        Capture::default()
    };
    world.resource_mut::<CaptureSnapshot>().0 = capture;
    let mut state = world.resource_mut::<DebugUi>();
    state.passes += 1;
    state.hidden_passes = if visible { 0 } else { state.hidden_passes.saturating_add(1) };
    state.last_pass = Some(ctx.cumulative_pass_nr());
}

/// Clicks and hovers over the debug UI don't reach Bevy UI buttons under it.
/// Bevy recomputes `Interaction` every PreUpdate, so this lasts one frame.
#[allow(deprecated)]
fn guard_bevy_ui(capture: Res<CaptureSnapshot>, input: Res<GameInput>, mut nodes: Query<&mut Interaction>) {
    if capture.0.pointer || input.ui_owns_pointer() {
        for mut interaction in &mut nodes {
            interaction.set_if_neq(Interaction::None);
        }
    }
}

/// bevy_egui runs its own passes in PostUpdate for contexts that aren't
/// manual; ours must be the only one.
fn check_single_pass(mut state: ResMut<DebugUi>, mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>) {
    let Ok(mut ctx) = contexts.single_mut() else { return };
    let now = ctx.get_mut().cumulative_pass_nr();
    if state.last_pass.is_some_and(|ours| ours != now) {
        state.extra_passes += 1;
        error!("the debug UI context ran a pass besides the manual one");
    }
}

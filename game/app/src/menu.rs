//! The main menu (App layer): shown while no session runs, it starts one
//! through `Flow`. Players' runs only (`Options::menus`).
// `Interaction`'s replacement (`Hovered`, `Pressed`) needs bevy_picking,
// which the build leaves out.
#![allow(deprecated)]

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::state::state_scoped::DespawnOnExit;
use service::{AppLayer, AppServices, Phase, Service, SessionState};

use crate::assets::SceneLibrary;
use crate::flow::{Flow, Launch};

#[derive(Resource)]
pub struct MainMenu {
    name: String,
    scene: usize,
    port: String,
    address: String,
    /// The field that has the keyboard.
    focus: Option<Field>,
    /// Why the last choice didn't start a session.
    error: Option<String>,
}

impl Service for MainMenu {
    type Layer = AppLayer;
    type Deps = (Flow,);

    fn init(world: &mut World) -> Self {
        let defaults = world.resource::<Flow>().defaults.clone();
        MainMenu {
            name: defaults.name,
            scene: defaults.scene,
            port: defaults.port.to_string(),
            address: defaults.address,
            focus: None,
            error: None,
        }
    }

    fn systems(app: &mut App) {
        let off = in_state(SessionState::Off);
        app.add_service_systems::<Self, _>(Phase::Input, (spawn, press, type_text).chain().run_if(off.clone()))
            .add_service_systems::<Self, _>(Phase::Present, show.run_if(off));
    }
}

/// What a menu button does.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Play,
    Host,
    Join,
    Quit,
    PreviousScene,
    NextScene,
    Edit(Field),
}

/// A text field.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Name,
    Port,
    Address,
}

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct FieldText(Field);
#[derive(Component)]
struct SceneName;
#[derive(Component)]
struct Message;

const BACKDROP: Color = Color::srgb(0.018, 0.025, 0.045);
const TEXT: Color = Color::srgb(0.83, 0.90, 0.93);
const MUTED: Color = Color::srgb(0.42, 0.54, 0.62);
const CYAN: Color = Color::srgb(0.27, 0.88, 0.95);
const LINE: Color = Color::srgb(0.20, 0.27, 0.33);
const HOVER: Color = Color::srgb(0.06, 0.13, 0.18);
const WARNING: Color = Color::srgb(1.0, 0.55, 0.3);

impl MainMenu {
    /// The launch an action asks for, from the fields.
    pub fn launch(&self, action: Action, defaults: &crate::flow::Defaults) -> Result<Launch, String> {
        let network = match action {
            Action::Play => None,
            Action::Host => Some(crate::flow::host(&self.port)?),
            Action::Join => Some(crate::flow::join(&self.address)?),
            _ => return Err(format!("{action:?} starts no session")),
        };
        Ok(Launch { network, scene: self.scene, name: self.name.trim().to_string(), record: defaults.record.clone(), replay: None })
    }

    fn value_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Name => &mut self.name,
            Field::Port => &mut self.port,
            Field::Address => &mut self.address,
        }
    }

    fn value(&self, field: Field) -> &str {
        match field {
            Field::Name => &self.name,
            Field::Port => &self.port,
            Field::Address => &self.address,
        }
    }

    /// Carries out an action: starting a session goes through `Flow`.
    fn act(&mut self, action: Action, flow: &mut Flow, scenes: usize, exit: &mut MessageWriter<AppExit>) {
        self.focus = match action {
            Action::Edit(field) => Some(field),
            _ => None,
        };
        match action {
            Action::Play | Action::Host | Action::Join => match self.launch(action, &flow.defaults) {
                Ok(launch) => {
                    self.error = None;
                    flow.start(launch);
                }
                Err(e) => self.error = Some(e),
            },
            Action::Quit => {
                exit.write(AppExit::Success);
            }
            Action::PreviousScene => self.scene = (self.scene + scenes - 1) % scenes,
            Action::NextScene => self.scene = (self.scene + 1) % scenes,
            Action::Edit(_) => {}
        }
    }
}

/// Builds the menu when no session runs; it goes when one starts.
fn spawn(mut commands: Commands, roots: Query<(), With<MenuRoot>>) {
    if !roots.is_empty() {
        return;
    }
    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };
    let button = |action: Action, width: f32, justify: JustifyContent| {
        (
            // What the old `Button` marker required, as in `piles::view`.
            Interaction::None,
            bevy::ui::FocusPolicy::Block,
            action,
            Node {
                width: px(width),
                padding: UiRect::axes(px(12), px(7)),
                border: UiRect::all(px(1)),
                justify_content: justify,
                ..default()
            },
            BorderColor::all(LINE),
            BackgroundColor(BACKDROP),
        )
    };
    let label = |text: &str, size: f32, color: Color| (Text::new(text), font(size), TextColor(color));
    let row = || Node { flex_direction: FlexDirection::Row, column_gap: px(10), align_items: AlignItems::Center, ..default() };
    let field = |field: Field, width: f32| {
        (
            button(Action::Edit(field), width, JustifyContent::Start),
            children![(FieldText(field), Text::new(""), font(14.0), TextColor(TEXT))],
        )
    };
    commands.spawn((
        MenuRoot,
        DespawnOnExit(SessionState::Off),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::left(px(120)),
            row_gap: px(12),
            ..default()
        },
        BackgroundColor(BACKDROP),
        GlobalZIndex(50),
        children![
            label("K E S T R E L", 40.0, TEXT),
            (Node { width: px(42), height: px(2), margin: UiRect::bottom(px(18)), ..default() }, BackgroundColor(CYAN)),
            label("NAME", 11.0, MUTED),
            field(Field::Name, 320.0),
            (row(), children![
                (button(Action::Play, 110.0, JustifyContent::Center), children![label("PLAY", 16.0, TEXT)]),
                (button(Action::PreviousScene, 36.0, JustifyContent::Center), children![label("<", 16.0, TEXT)]),
                (SceneName, Node { width: px(260), justify_content: JustifyContent::Center, ..default() }, Text::new(""), font(14.0), TextColor(TEXT)),
                (button(Action::NextScene, 36.0, JustifyContent::Center), children![label(">", 16.0, TEXT)]),
            ]),
            (row(), children![
                (button(Action::Host, 110.0, JustifyContent::Center), children![label("HOST", 16.0, TEXT)]),
                label("PORT", 11.0, MUTED),
                field(Field::Port, 100.0),
            ]),
            (row(), children![
                (button(Action::Join, 110.0, JustifyContent::Center), children![label("JOIN", 16.0, TEXT)]),
                field(Field::Address, 240.0),
            ]),
            (button(Action::Quit, 110.0, JustifyContent::Center), children![label("QUIT", 16.0, TEXT)]),
            (Message, Text::new(""), font(14.0), TextColor(WARNING), Node { max_width: px(640), margin: UiRect::top(px(14)), ..default() }),
        ],
    ));
}

/// Mouse clicks on the menu's buttons and fields.
fn press(
    buttons: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut menu: ResMut<MainMenu>,
    mut flow: ResMut<Flow>,
    library: Res<SceneLibrary>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action) in &buttons {
        if *interaction == Interaction::Pressed {
            menu.act(*action, &mut flow, library.scenes.len(), &mut exit);
        }
    }
}

/// Typing into the focused field. Enter on the port hosts, on the address
/// joins, on the name plays; Escape and Tab leave the field.
fn type_text(
    mut keys: MessageReader<KeyboardInput>,
    mut menu: ResMut<MainMenu>,
    mut flow: ResMut<Flow>,
    library: Res<SceneLibrary>,
    mut exit: MessageWriter<AppExit>,
) {
    for key in keys.read() {
        let Some(field) = menu.focus else { break };
        if key.state != ButtonState::Pressed {
            continue;
        }
        match &key.logical_key {
            Key::Enter => {
                let action = match field {
                    Field::Name => Action::Play,
                    Field::Port => Action::Host,
                    Field::Address => Action::Join,
                };
                menu.act(action, &mut flow, library.scenes.len(), &mut exit);
            }
            Key::Escape | Key::Tab => menu.focus = None,
            Key::Backspace => {
                menu.value_mut(field).pop();
            }
            _ => {
                let Some(text) = &key.text else { continue };
                let value = menu.value_mut(field);
                for c in text.chars().filter(|c| !c.is_control()) {
                    let fits = match field {
                        Field::Name => value.chars().count() < 24,
                        Field::Port => c.is_ascii_digit() && value.len() < 5,
                        Field::Address => !c.is_whitespace() && value.len() < 64,
                    };
                    if fits {
                        value.push(c);
                    }
                }
            }
        }
    }
    flow.typing = menu.focus.is_some();
}

/// Shows the fields, the scene, the message and which button is under the
/// pointer or has the keyboard.
fn show(
    menu: Res<MainMenu>,
    flow: Res<Flow>,
    library: Res<SceneLibrary>,
    mut texts: Query<(&mut Text, Option<&FieldText>, Has<SceneName>, Has<Message>)>,
    mut buttons: Query<(&Interaction, &Action, &mut BackgroundColor, &mut BorderColor)>,
) {
    for (mut text, field, scene, message) in &mut texts {
        let shown = if let Some(FieldText(field)) = field {
            let caret = if menu.focus == Some(*field) { "_" } else { "" };
            format!("{}{caret}", menu.value(*field))
        } else if scene {
            library.scenes[menu.scene].scene.name.clone()
        } else if message {
            menu.error.clone().or_else(|| flow.ended.clone()).unwrap_or_default()
        } else {
            continue;
        };
        if text.0 != shown {
            text.0 = shown;
        }
    }
    for (interaction, action, mut background, mut border) in &mut buttons {
        let focused = matches!(action, Action::Edit(field) if menu.focus == Some(*field));
        let hovered = *interaction != Interaction::None;
        background.set_if_neq(BackgroundColor(if hovered { HOVER } else { BACKDROP }));
        border.set_if_neq(BorderColor::all(if focused || hovered { CYAN } else { LINE }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::Defaults;
    use crate::lockstep::Network;

    fn menu() -> (MainMenu, Defaults) {
        let defaults = Defaults { name: "ana".into(), scene: 3, record: Some("rec".into()), port: 7000, address: "127.0.0.1:7100".into() };
        let menu = MainMenu { name: " ana ".into(), scene: 3, port: "7000".into(), address: defaults.address.clone(), focus: None, error: None };
        (menu, defaults)
    }

    #[test]
    fn actions_build_launches_from_the_fields() {
        let (menu, defaults) = menu();
        let play = menu.launch(Action::Play, &defaults).unwrap();
        assert!(play.network.is_none());
        assert_eq!((play.scene, play.name.as_str(), play.record.as_deref()), (3, "ana", Some("rec")));
        assert!(matches!(menu.launch(Action::Host, &defaults).unwrap().network, Some(Network::Host(a)) if a.port() == 7000));
        assert!(matches!(menu.launch(Action::Join, &defaults).unwrap().network, Some(Network::Join(a)) if a.port() == 7100));
    }

    #[test]
    fn bad_fields_are_errors() {
        let (mut menu, defaults) = menu();
        menu.port = String::new();
        menu.address = "no port here".into();
        assert!(menu.launch(Action::Host, &defaults).is_err());
        assert!(menu.launch(Action::Join, &defaults).is_err());
        assert!(menu.launch(Action::Quit, &defaults).is_err());
    }
}

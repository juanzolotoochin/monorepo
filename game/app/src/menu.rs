//! The main menu (App layer): shown while no session runs, it starts one
//! through `Flow`. Players' runs only (`Options::menus`).
// `Interaction`'s replacement (`Hovered`, `Pressed`) needs bevy_picking,
// which the build leaves out.
#![allow(deprecated)]

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::clipboard::{Clipboard, ClipboardRead};
use bevy::prelude::*;
use bevy::state::state_scoped::DespawnOnExit;
use service::{AppLayer, AppServices, Phase, Service, SessionState};

use crate::assets::SceneLibrary;
use crate::flow::{Flow, Launch};

/// Disposable, per-user menu preferences. Scene IDs survive scene reordering.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Settings {
    name: String,
    scene: String,
    port: String,
    address: String,
}

impl Settings {
    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join("kestrel-menu-settings.json")
    }

    fn read(path: &std::path::Path) -> Option<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                warn!("Cannot read menu settings: {e}");
                return None;
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(settings) => Some(settings),
            Err(e) => {
                warn!("Ignoring invalid menu settings: {e}");
                None
            }
        }
    }

    fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
    }

    fn apply(self, menu: &mut MainMenu, library: &SceneLibrary) {
        menu.name = filtered(Field::Name, &self.name);
        menu.port = filtered(Field::Port, &self.port);
        menu.address = filtered(Field::Address, &self.address);
        if let Ok(scene) = library.index(&self.scene) {
            menu.scene = scene;
        }
    }
}

/// Use the same limits for typing, paste, and restored settings.
fn append(value: &mut String, field: Field, text: &str) {
    for c in text.chars().filter(|c| !c.is_control()) {
        let fits = match field {
            Field::Name => value.chars().count() < 24,
            Field::Port => c.is_ascii_digit() && value.len() < 5,
            Field::Address => !c.is_whitespace() && value.len() + c.len_utf8() <= 64,
        };
        if fits {
            value.push(c);
        }
    }
}

fn filtered(field: Field, text: &str) -> String {
    let mut value = String::new();
    append(&mut value, field, text);
    value
}

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
        let mut menu = MainMenu {
            name: defaults.name,
            scene: defaults.scene,
            port: defaults.port.to_string(),
            address: defaults.address,
            focus: None,
            error: None,
        };
        if let Some(settings) = Settings::read(&Settings::path()) {
            settings.apply(&mut menu, world.resource::<SceneLibrary>());
        }
        menu
    }

    fn systems(app: &mut App) {
        let off = in_state(SessionState::Off);
        app.add_service_systems::<Self, _>(Phase::Input, (spawn, press, type_text, persist).chain().run_if(off.clone()))
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

pub(crate) const BACKDROP: Color = Color::srgb(0.018, 0.025, 0.045);
pub(crate) const TEXT: Color = Color::srgb(0.83, 0.90, 0.93);
pub(crate) const MUTED: Color = Color::srgb(0.42, 0.54, 0.62);
const CYAN: Color = Color::srgb(0.27, 0.88, 0.95);
const LINE: Color = Color::srgb(0.20, 0.27, 0.33);
const HOVER: Color = Color::srgb(0.06, 0.13, 0.18);
const WARNING: Color = Color::srgb(1.0, 0.55, 0.3);

pub(crate) fn font(size: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(size), ..default() }
}

/// A clickable box in the menus' style; `kind` says what it does.
pub(crate) fn button(kind: impl Component, width: f32, justify: JustifyContent) -> impl Bundle {
    (
        // What the old `Button` marker required, as in `piles::view`.
        Interaction::None,
        bevy::ui::FocusPolicy::Block,
        kind,
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
}

/// Lights a button up under the pointer, or while it has the keyboard.
pub(crate) fn highlight(interaction: Interaction, focused: bool, background: &mut Mut<BackgroundColor>, border: &mut Mut<BorderColor>) {
    let hovered = interaction != Interaction::None;
    background.set_if_neq(BackgroundColor(if hovered { HOVER } else { BACKDROP }));
    border.set_if_neq(BorderColor::all(if focused || hovered { CYAN } else { LINE }));
}

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
            label("Click a field: Ctrl+C copies / Ctrl+V replaces (Cmd on macOS)", 11.0, MUTED),
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
    held: Res<ButtonInput<KeyCode>>,
    mut clipboard: ResMut<Clipboard>,
    mut menu: ResMut<MainMenu>,
    mut flow: ResMut<Flow>,
    library: Res<SceneLibrary>,
    mut exit: MessageWriter<AppExit>,
) {
    let shortcut = if cfg!(target_os = "macos") {
        held.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight])
    } else {
        held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
    };
    for key in keys.read() {
        let Some(field) = menu.focus else { break };
        if key.state != ButtonState::Pressed {
            continue;
        }
        if shortcut {
            let result = match &key.logical_key {
                Key::Character(c) if c.eq_ignore_ascii_case("c") => clipboard.set_text(menu.value(field).to_string()),
                Key::Character(c) if c.eq_ignore_ascii_case("v") => match clipboard.fetch_text() {
                    ClipboardRead::Ready(Ok(text)) => {
                        *menu.value_mut(field) = filtered(field, text.trim());
                        Ok(())
                    }
                    ClipboardRead::Ready(Err(e)) => Err(e),
                    _ => continue,
                },
                _ => continue,
            };
            menu.error = result.err().map(|e| format!("Clipboard: {e}"));
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
                append(menu.value_mut(field), field, text);
            }
        }
    }
    flow.typing = menu.focus.is_some();
}

/// Save after edits, including the frame that launches or quits. Focus and
/// connection errors aren't preferences and don't cause disk writes.
fn persist(menu: Res<MainMenu>, library: Res<SceneLibrary>, mut last: Local<Option<Settings>>) {
    let settings = Settings {
        name: menu.name.clone(),
        scene: library.scenes[menu.scene].scene.id.clone(),
        port: menu.port.clone(),
        address: menu.address.clone(),
    };
    if last.as_ref() == Some(&settings) {
        return;
    }
    if let Err(e) = settings.write(&Settings::path()) {
        warn!("Cannot save menu settings: {e}");
    }
    *last = Some(settings);
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
        highlight(*interaction, focused, &mut background, &mut border);
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

    #[test]
    fn pasted_text_replaces_defaults_and_respects_field_limits() {
        let (mut menu, _) = menu();
        *menu.value_mut(Field::Address) = filtered(Field::Address, "  my-desktop:7777\r\n".trim());
        assert_eq!(menu.address, "my-desktop:7777");
        assert_eq!(filtered(Field::Port, "7a12345\n"), "71234");
        assert_eq!(filtered(Field::Name, &"é".repeat(30)).chars().count(), 24);
        assert_eq!(filtered(Field::Address, &"é".repeat(40)).len(), 64);
        assert_eq!(filtered(Field::Address, "a b\tc\nd:7000"), "abcd:7000");
    }

    #[test]
    fn settings_round_trip_and_restore_the_last_address() {
        let path = std::env::temp_dir().join(format!(
            "kestrel-menu-test-{}-{}.json", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        ));
        assert!(Settings::read(&path).is_none());
        let settings = Settings {
            name: "Ada".into(), scene: "field".into(), port: "7777".into(), address: "my-desktop:7777".into(),
        };
        settings.write(&path).unwrap();
        let restored = Settings::read(&path).unwrap();
        assert_eq!(restored, settings);
        let library = SceneLibrary::new();
        let (mut menu, defaults) = menu();
        restored.apply(&mut menu, &library);
        assert_eq!(menu.address, "my-desktop:7777");
        assert_eq!(menu.name, "Ada");
        assert_eq!(menu.scene, library.index("field").unwrap());
        assert!(matches!(menu.launch(Action::Host, &defaults).unwrap().network, Some(Network::Host(a)) if a.port() == 7777));
        std::fs::write(&path, b"{unfinished").unwrap();
        assert!(Settings::read(&path).is_none());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn removed_scene_keeps_the_default_and_restored_fields_are_bounded() {
        let (mut menu, _) = menu();
        Settings {
            name: "n".repeat(100), scene: "removed-scene".into(), port: "7777777".into(), address: "x".repeat(100),
        }.apply(&mut menu, &SceneLibrary::new());
        assert_eq!(menu.scene, 3);
        assert_eq!(menu.name.len(), 24);
        assert_eq!(menu.port, "77777");
        assert_eq!(menu.address.len(), 64);
    }
}

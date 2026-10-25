//! The launch menu (App layer): shown while no session runs and Character
//! Studio is closed. It picks a scene and how to play, then starts a session
//! through `Flow`, or opens Character Studio. A session that ends returns
//! here with its reason. Players' runs only (`Options::menus`).
// `Interaction`'s replacement (`Hovered`, `Pressed`) needs bevy_picking,
// which the build leaves out.
#![allow(deprecated)]

use bevy::{
    app::AppExit,
    input::{keyboard::KeyboardInput, ButtonState},
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    state::state_scoped::DespawnOnExit,
    ui::FocusPolicy,
};
use service::{AppLayer, AppServices, Phase, Service};

use crate::assets::SceneLibrary;
use crate::flow::{Flow, Front, Launch};
use crate::viewer::Viewer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Solo,
    Host,
    Join,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Port,
    Address,
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Scene(usize),
    Mode(Mode),
    Field(Field),
    Start,
    Viewer,
    Quit,
}
#[derive(Component)]
pub(crate) struct Root;
#[derive(Component)]
pub(crate) struct FieldText(Field);
#[derive(Component)]
pub(crate) struct StatusText;

#[derive(Resource)]
pub struct MainMenu {
    selected_scene: usize,
    mode: Mode,
    focus: Option<Field>,
    name: String,
    port: String,
    address: String,
    error: String,
}

impl Service for MainMenu {
    type Layer = AppLayer;
    type Deps = (Flow, Viewer);

    fn init(world: &mut World) -> Self {
        let defaults = world.resource::<Flow>().defaults.clone();
        MainMenu {
            selected_scene: defaults.scene,
            mode: Mode::Solo,
            focus: None,
            name: defaults.name,
            port: defaults.port.to_string(),
            address: defaults.address,
            error: String::new(),
        }
    }

    fn systems(app: &mut App) {
        let open = in_state(Front::Menu);
        app.add_service_systems::<Self, _>(
            Phase::Input,
            (setup, update.run_if(crate::control::interactive)).chain().run_if(open.clone()),
        )
        .add_service_systems::<Self, _>(Phase::Present, capture.run_if(crate::control::interactive).run_if(open));
    }
}

impl MainMenu {
    /// The session the chosen mode asks for, from the fields.
    fn launch(&self, defaults: &crate::flow::Defaults) -> Result<Launch, String> {
        let network = match self.mode {
            Mode::Solo => None,
            Mode::Host => {
                let port: u16 = self.port.trim().parse().map_err(|_| "Enter a valid host port (1–65535).")?;
                if port == 0 {
                    return Err("Enter a valid host port (1–65535).".into());
                }
                Some(crate::lockstep::Network::Host(([0, 0, 0, 0], port).into()))
            }
            Mode::Join => Some(
                crate::flow::join(&self.address).map_err(|_| "Enter a host and port, for example 192.168.1.5:7777.")?,
            ),
        };
        Ok(Launch {
            network,
            scene: self.selected_scene,
            name: self.name.trim().to_string(),
            record: defaults.record.clone(),
            replay: None,
        })
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
}

/// The menus' shared look, also used by the pause overlay.
pub(crate) const BACKDROP: Color = Color::srgba(0.015, 0.035, 0.05, 0.97);
pub(crate) const TEXT: Color = Color::srgb(0.81, 0.93, 0.91);
pub(crate) const MUTED: Color = Color::srgb(0.55, 0.71, 0.75);
const ACCENT: Color = Color::srgb(0.92, 0.66, 0.35);
const TITLE: Color = Color::srgb(0.43, 0.91, 0.95);
const IDLE: Color = Color::srgba(0.08, 0.16, 0.19, 0.96);
const HOVER: Color = Color::srgba(0.12, 0.25, 0.29, 0.98);
const SELECTED: Color = Color::srgba(0.10, 0.39, 0.43, 0.98);
const LINE: Color = Color::srgb(0.17, 0.32, 0.37);
const FOCUS_LINE: Color = Color::srgb(0.43, 0.91, 0.95);
const WARNING: Color = Color::srgb(1.0, 0.45, 0.32);

pub(crate) fn font(size: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(size), ..default() }
}

/// A clickable box in the menus' style; `kind` says what it does.
pub(crate) fn button(kind: impl Component, width: f32, justify: JustifyContent) -> impl Bundle {
    (
        Interaction::None,
        FocusPolicy::Block,
        kind,
        Node {
            width: px(width),
            min_height: px(34.0),
            padding: UiRect::axes(px(12.0), px(7.0)),
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.0)),
            justify_content: justify,
            ..default()
        },
        BorderColor::all(LINE),
        BackgroundColor(IDLE),
    )
}

/// Lights a button up under the pointer, or while it has the keyboard.
pub(crate) fn highlight(interaction: Interaction, focused: bool, background: &mut Mut<BackgroundColor>, border: &mut Mut<BorderColor>) {
    let hovered = interaction != Interaction::None;
    background.set_if_neq(BackgroundColor(if hovered { HOVER } else { IDLE }));
    border.set_if_neq(BorderColor::all(if focused || hovered { FOCUS_LINE } else { LINE }));
}

fn label(parent: &mut ChildSpawnerCommands, text: &str, size: f32, color: Color) {
    parent.spawn((Text::new(text), font(size), TextColor(color)));
}

fn choice(parent: &mut ChildSpawnerCommands, action: Action, text: &str) {
    parent
        .spawn(button(action, 0.0, JustifyContent::Start))
        .insert(Node {
            width: percent(100.0),
            min_height: px(34.0),
            padding: UiRect::axes(px(12.0), px(7.0)),
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .with_children(|p| label(p, text, 16.0, TEXT));
}

fn field(parent: &mut ChildSpawnerCommands, field: Field, title: &str) {
    label(parent, title, 13.0, MUTED);
    parent
        .spawn((
            Action::Field(field),
            Interaction::None,
            FocusPolicy::Block,
            Node {
                width: percent(100.0),
                min_height: px(40.0),
                padding: UiRect::all(px(9.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BorderColor::all(LINE),
            BackgroundColor(Color::srgba(0.04, 0.10, 0.13, 1.0)),
        ))
        .with_children(|p| {
            p.spawn((FieldText(field), Text::new(""), font(17.0), TextColor(Color::WHITE)));
        });
}

/// Builds the menu when it opens; it goes when a session starts or Studio
/// opens.
pub fn setup(mut commands: Commands, scenes: Res<SceneLibrary>, roots: Query<(), With<Root>>) {
    if !roots.is_empty() {
        return;
    }
    commands
        .spawn((
            Root,
            DespawnOnExit(Front::Menu),
            GlobalZIndex(1000),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(BACKDROP),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: px(1080.0),
                max_width: percent(94.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(18.0),
                ..default()
            })
            .with_children(|panel| {
                label(panel, "K E S T R E L   / /   L A U N C H", 30.0, TITLE);
                label(panel, "Choose a world and how to play.", 17.0, MUTED);
                panel
                    .spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(28.0), ..default() })
                    .with_children(|columns| {
                        columns
                            .spawn(Node {
                                width: percent(58.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(5.0),
                                ..default()
                            })
                            .with_children(|list| {
                                label(list, "SCENES", 18.0, ACCENT);
                                for (index, pack) in scenes.scenes.iter().enumerate() {
                                    choice(list, Action::Scene(index), &pack.scene.name);
                                }
                            });
                        columns
                            .spawn(Node {
                                width: percent(42.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(11.0),
                                ..default()
                            })
                            .with_children(|choices| {
                                label(choices, "SESSION", 18.0, ACCENT);
                                field(choices, Field::Name, "NAME");
                                choice(choices, Action::Mode(Mode::Solo), "PLAY SOLO");
                                choice(choices, Action::Mode(Mode::Host), "HOST MULTIPLAYER");
                                choice(choices, Action::Mode(Mode::Join), "JOIN MULTIPLAYER");
                                field(choices, Field::Port, "HOST PORT");
                                field(choices, Field::Address, "JOIN ADDRESS");
                                label(choices, "Joining? The host picks the scene.", 13.0, MUTED);
                                choice(choices, Action::Start, "START GAME  >");
                                choice(choices, Action::Viewer, "CHARACTER STUDIO  >");
                                choice(choices, Action::Quit, "QUIT");
                                choices.spawn((StatusText, Text::new(""), font(14.0), TextColor(WARNING)));
                            });
                    });
                label(panel, "Click a field to edit / Enter to start / Esc to quit", 13.0, Color::srgb(0.42, 0.60, 0.65));
            });
        });
}

/// Clicks and typing: picks the scene and mode, edits the fields, starts a
/// session through `Flow`, opens Character Studio or quits.
pub fn update(
    mut menu: ResMut<MainMenu>,
    mut flow: ResMut<Flow>,
    mut viewer: ResMut<Viewer>,
    scenes: Res<SceneLibrary>,
    mut front: ResMut<NextState<Front>>,
    mut keys: MessageReader<KeyboardInput>,
    mut actions: Query<(&Action, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
    mut fields: Query<(&FieldText, &mut Text)>,
    mut status: Query<&mut Text, (With<StatusText>, Without<FieldText>)>,
    mut exit: MessageWriter<AppExit>,
) {
    let mut start = false;
    for (action, interaction, _, _) in &mut actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            Action::Scene(index) => {
                menu.selected_scene = index;
                menu.error.clear();
            }
            Action::Mode(mode) => {
                menu.mode = mode;
                menu.focus = None;
                menu.error.clear();
            }
            Action::Field(field) => {
                menu.focus = Some(field);
                match field {
                    Field::Port => menu.mode = Mode::Host,
                    Field::Address => menu.mode = Mode::Join,
                    Field::Name => {}
                }
                menu.error.clear();
            }
            Action::Start => start = true,
            Action::Viewer => {
                let Some(character) = viewer.open_for_scene(&scenes, menu.selected_scene) else {
                    menu.error = "No playable characters are available.".into();
                    continue;
                };
                viewer.character = character;
                menu.focus = None;
                front.set(Front::Studio);
                return;
            }
            Action::Quit => {
                exit.write(AppExit::Success);
                return;
            }
        }
    }
    for key in keys.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        match key.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter => start = true,
            KeyCode::Escape if menu.focus.is_some() => menu.focus = None,
            KeyCode::Escape => {
                exit.write(AppExit::Success);
                return;
            }
            KeyCode::Backspace => {
                if let Some(field) = menu.focus {
                    menu.value_mut(field).pop();
                    menu.error.clear();
                }
            }
            KeyCode::Tab => {
                menu.focus = Some(match menu.focus {
                    Some(Field::Name) => Field::Port,
                    Some(Field::Port) => Field::Address,
                    Some(Field::Address) | None => Field::Name,
                })
            }
            _ => {
                if let (Some(field), Some(text)) = (menu.focus, key.text.as_ref()) {
                    let value = menu.value_mut(field);
                    for c in text.chars().filter(|c| !c.is_control()) {
                        let fits = match field {
                            Field::Name => value.chars().count() < 24,
                            Field::Port => c.is_ascii_digit() && value.len() < 5,
                            Field::Address => (c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-')) && value.len() < 80,
                        };
                        if fits {
                            value.push(c);
                        }
                    }
                    menu.error.clear();
                }
            }
        }
    }
    // A field that has the keyboard keeps the app's keys (M mutes) quiet.
    flow.typing = menu.focus.is_some();
    if start {
        match menu.launch(&flow.defaults) {
            Ok(launch) => {
                menu.error.clear();
                menu.focus = None;
                flow.typing = false;
                flow.start(launch);
                return;
            }
            Err(error) => menu.error = error,
        }
    }
    for (action, interaction, mut background, mut border) in &mut actions {
        let selected = match *action {
            Action::Scene(index) => menu.selected_scene == index,
            Action::Mode(mode) => menu.mode == mode,
            _ => false,
        };
        let focused = matches!(*action, Action::Field(field) if menu.focus == Some(field));
        highlight(*interaction, focused, &mut background, &mut border);
        if selected {
            background.set_if_neq(BackgroundColor(SELECTED));
        }
    }
    for (field, mut text) in &mut fields {
        let caret = if menu.focus == Some(field.0) { "_" } else { "" };
        let shown = format!("{}{caret}", menu.value(field.0));
        if text.0 != shown {
            text.0 = shown;
        }
    }
    // The menu's own error, or why the last session ended.
    let message = if menu.error.is_empty() { flow.ended.clone().unwrap_or_default() } else { menu.error.clone() };
    for mut text in &mut status {
        if text.0 != message {
            text.0 = message.clone();
        }
    }
}

#[derive(Default)]
pub(crate) struct MenuCapture {
    initialized: bool,
    path: Option<String>,
    elapsed: f32,
    saved: bool,
}

/// `--capture-menu <path>` saves a repeatable screenshot for visual review.
pub fn capture(mut commands: Commands, time: Res<Time>, mut capture: Local<MenuCapture>, mut exit: MessageWriter<AppExit>) {
    if !capture.initialized {
        capture.initialized = true;
        let args: Vec<_> = std::env::args().collect();
        capture.path = args.windows(2).find(|pair| pair[0] == "--capture-menu").map(|pair| pair[1].clone());
    }
    let Some(path) = capture.path.clone() else {
        return;
    };
    capture.elapsed += time.delta_secs().min(0.1);
    if capture.elapsed > 2.0 && !capture.saved {
        capture.saved = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if capture.elapsed > 3.0 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::Defaults;
    use crate::lockstep::Network;

    fn menu() -> (MainMenu, Defaults) {
        let defaults = Defaults {
            name: "ana".into(),
            scene: 3,
            record: Some("rec".into()),
            port: crate::flow::DEFAULT_PORT,
            address: "127.0.0.1:4200".into(),
        };
        let menu = MainMenu {
            selected_scene: 3,
            mode: Mode::Solo,
            focus: None,
            name: " ana ".into(),
            port: "4200".into(),
            address: defaults.address.clone(),
            error: String::new(),
        };
        (menu, defaults)
    }

    #[test]
    fn launch_choices_build_the_session() {
        let (mut menu, defaults) = menu();
        let solo = menu.launch(&defaults).unwrap();
        assert!(solo.network.is_none());
        assert_eq!((solo.scene, solo.name.as_str(), solo.record.as_deref()), (3, "ana", Some("rec")));

        menu.mode = Mode::Host;
        assert!(matches!(menu.launch(&defaults).unwrap().network, Some(Network::Host(addr)) if addr.port() == 4200));

        menu.mode = Mode::Join;
        assert!(matches!(menu.launch(&defaults).unwrap().network, Some(Network::Join(addr)) if addr.port() == 4200));

        menu.address = "not-an-address".into();
        assert!(menu.launch(&defaults).is_err());
        menu.mode = Mode::Host;
        menu.port = "0".into();
        assert!(menu.launch(&defaults).is_err());
    }

    #[test]
    fn the_menu_offers_port_7777() {
        assert_eq!(crate::flow::DEFAULT_PORT, 7777);
    }

    /// The menu's input on its own, over the menu's services, with `action`
    /// pressed in `scene`.
    fn press(scene: usize, action: Action) -> App {
        let scenes = SceneLibrary::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<KeyboardInput>()
            .add_message::<AppExit>()
            .init_resource::<NextState<Front>>()
            .init_resource::<Viewer>()
            .insert_resource(crate::flow::Start { launch: None, defaults: menu().1 })
            .insert_resource(scenes)
            .add_systems(Update, update);
        let flow = <Flow as Service>::init(app.world_mut());
        app.insert_resource(flow);
        let mut menu = <MainMenu as Service>::init(app.world_mut());
        menu.selected_scene = scene;
        app.insert_resource(menu);
        app.world_mut().spawn((action, Interaction::Pressed, BackgroundColor::default(), BorderColor::default()));
        app.update();
        app
    }

    #[test]
    fn start_asks_flow_for_a_session_in_the_selected_scene() {
        let field = SceneLibrary::new().index("field").unwrap();
        let app = press(field, Action::Start);
        let launch = app.world().resource::<Flow>().starting().expect("a session is starting");
        assert_eq!(launch.scene, field);
        assert!(launch.network.is_none());
        assert_eq!(launch.name, "ana");
    }

    #[test]
    fn studio_opens_the_selected_scenes_character() {
        let scenes = SceneLibrary::new();
        let glade = scenes.index("moonlit-glade").unwrap();
        let app = press(glade, Action::Viewer);
        assert!(matches!(app.world().resource::<NextState<Front>>(), NextState::Pending(Front::Studio)));
        let viewer = app.world().resource::<Viewer>();
        assert_eq!(scenes.characters[viewer.character].scene_index, glade);
        assert!(app.world().resource::<Flow>().starting().is_none());
    }
}

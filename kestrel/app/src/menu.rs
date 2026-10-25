//! Launch choices are applied before the first simulation world is built.
use crate::{
    assets::SceneLibrary,
    lockstep::{Game, Network},
    viewer::Viewer,
};
use bevy::{
    app::AppExit,
    input::{keyboard::KeyboardInput, ButtonState},
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    ui::FocusPolicy,
};
use std::net::ToSocketAddrs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Solo,
    Host,
    Join,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
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
pub struct LaunchMenu {
    pub open: bool,
    selected_scene: usize,
    mode: Mode,
    focus: Option<Field>,
    port: String,
    address: String,
    error: String,
}

impl LaunchMenu {
    pub fn new(open: bool, scenes: &SceneLibrary) -> Self {
        Self {
            open,
            selected_scene: scenes.initial,
            mode: Mode::Solo,
            focus: None,
            port: "4100".into(),
            address: "127.0.0.1:4100".into(),
            error: String::new(),
        }
    }

    fn network(&self) -> Result<Option<Network>, String> {
        match self.mode {
            Mode::Solo => Ok(None),
            Mode::Host => {
                let port: u16 = self
                    .port
                    .parse()
                    .map_err(|_| "Enter a valid host port (1–65535).")?;
                if port == 0 {
                    return Err("Enter a valid host port (1–65535).".into());
                }
                Ok(Some(Network::Host(([0, 0, 0, 0], port).into())))
            }
            Mode::Join => {
                let address = self.address.trim();
                let host = address
                    .to_socket_addrs()
                    .map_err(|_| "Enter a host and port, for example 192.168.1.5:4100.")?
                    .find(|addr| addr.is_ipv4())
                    .ok_or("The host address has no IPv4 endpoint.")?;
                Ok(Some(Network::Join(host)))
            }
        }
    }

    fn launch(&self, scenes: &mut SceneLibrary, game: &mut Game) -> Result<(), String> {
        let network = self.network()?;
        scenes.initial = self.selected_scene;
        scenes.locked = matches!(network, Some(Network::Join(_)));
        game.network = network;
        Ok(())
    }
}

pub fn playing(menu: Res<LaunchMenu>) -> bool {
    !menu.open
}

fn label(parent: &mut ChildSpawnerCommands, text: &str, size: f32, color: Color) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    ));
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, text: &str) {
    parent
        .spawn((
            action,
            Interaction::None,
            FocusPolicy::Block,
            Node {
                width: percent(100.0),
                min_height: px(34.0),
                padding: UiRect::axes(px(12.0), px(7.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.16, 0.19, 0.96)),
            BorderColor::all(Color::srgb(0.17, 0.32, 0.37)),
        ))
        .with_children(|p| label(p, text, 16.0, Color::srgb(0.81, 0.93, 0.91)));
}

pub fn setup(
    mut commands: Commands,
    menu: Res<LaunchMenu>,
    scenes: Res<SceneLibrary>,
    roots: Query<Entity, With<Root>>,
) {
    if !menu.open || !roots.is_empty() {
        return;
    }
    commands
        .spawn((
            Root,
            GlobalZIndex(1000),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.05, 0.97)),
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
                label(
                    panel,
                    "K E S T R E L   / /   L A U N C H",
                    30.0,
                    Color::srgb(0.43, 0.91, 0.95),
                );
                label(
                    panel,
                    "Choose a world and how to play.",
                    17.0,
                    Color::srgb(0.55, 0.71, 0.75),
                );
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(28.0),
                        ..default()
                    })
                    .with_children(|columns| {
                        columns
                            .spawn(Node {
                                width: percent(58.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(5.0),
                                ..default()
                            })
                            .with_children(|list| {
                                label(list, "SCENES", 18.0, Color::srgb(0.92, 0.66, 0.35));
                                for (index, pack) in scenes.scenes.iter().enumerate() {
                                    button(list, Action::Scene(index), &pack.scene.name);
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
                                label(choices, "SESSION", 18.0, Color::srgb(0.92, 0.66, 0.35));
                                button(choices, Action::Mode(Mode::Solo), "PLAY SOLO");
                                button(choices, Action::Mode(Mode::Host), "HOST MULTIPLAYER");
                                button(choices, Action::Mode(Mode::Join), "JOIN MULTIPLAYER");
                                label(choices, "HOST PORT", 13.0, Color::srgb(0.55, 0.71, 0.75));
                                choices
                                    .spawn((
                                        Action::Field(Field::Port),
                                        Interaction::None,
                                        FocusPolicy::Block,
                                        Node {
                                            width: percent(100.0),
                                            min_height: px(40.0),
                                            padding: UiRect::all(px(9.0)),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgba(0.04, 0.10, 0.13, 1.0)),
                                    ))
                                    .with_children(|field| {
                                        field.spawn((
                                            FieldText(Field::Port),
                                            Text::new("4100"),
                                            TextFont {
                                                font_size: FontSize::Px(17.0),
                                                ..default()
                                            },
                                            TextColor(Color::WHITE),
                                        ));
                                    });
                                label(choices, "JOIN ADDRESS", 13.0, Color::srgb(0.55, 0.71, 0.75));
                                choices
                                    .spawn((
                                        Action::Field(Field::Address),
                                        Interaction::None,
                                        FocusPolicy::Block,
                                        Node {
                                            width: percent(100.0),
                                            min_height: px(40.0),
                                            padding: UiRect::all(px(9.0)),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgba(0.04, 0.10, 0.13, 1.0)),
                                    ))
                                    .with_children(|field| {
                                        field.spawn((
                                            FieldText(Field::Address),
                                            Text::new("127.0.0.1:4100"),
                                            TextFont {
                                                font_size: FontSize::Px(17.0),
                                                ..default()
                                            },
                                            TextColor(Color::WHITE),
                                        ));
                                    });
                                label(
                                    choices,
                                    "Joining? Select the same scene as the host.",
                                    13.0,
                                    Color::srgb(0.55, 0.71, 0.75),
                                );
                                button(choices, Action::Start, "START GAME  >");
                                button(choices, Action::Viewer, "CHARACTER STUDIO  >");
                                button(choices, Action::Quit, "QUIT");
                                choices.spawn((
                                    StatusText,
                                    Text::new(""),
                                    TextFont {
                                        font_size: FontSize::Px(14.0),
                                        ..default()
                                    },
                                    TextColor(Color::srgb(1.0, 0.45, 0.32)),
                                ));
                            });
                    });
                label(
                    panel,
                    "Click a field to edit / Enter to start / Esc to quit",
                    13.0,
                    Color::srgb(0.42, 0.60, 0.65),
                );
            });
        });
}

pub fn update(
    mut commands: Commands,
    mut menu: ResMut<LaunchMenu>,
    mut scenes: ResMut<SceneLibrary>,
    mut game: ResMut<Game>,
    mut viewer: ResMut<Viewer>,
    mut keys: MessageReader<KeyboardInput>,
    mut backgrounds: Query<(&Action, &Interaction, &mut BackgroundColor)>,
    mut fields: Query<(&FieldText, &mut Text)>,
    mut status: Query<&mut Text, (With<StatusText>, Without<FieldText>)>,
    root: Query<Entity, With<Root>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !menu.open {
        return;
    }
    let mut start = false;
    for (action, interaction, _) in &mut backgrounds {
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
                menu.mode = if field == Field::Port {
                    Mode::Host
                } else {
                    Mode::Join
                };
                menu.error.clear();
            }
            Action::Start => start = true,
            Action::Viewer => {
                let index = scenes.character(menu.selected_scene)
                    .map(|character| character.scene_index)
                    .or_else(|| scenes.characters.first().map(|character| character.scene_index));
                let Some(index) = index else {
                    menu.error = "No playable characters are available.".into();
                    continue;
                };
                scenes.initial = index;
                scenes.requested = Some(index);
                scenes.locked = false;
                game.network = None;
                viewer.begin();
                viewer.character = scenes.characters.iter()
                    .position(|entry| entry.scene_index == index && matches!(entry.source, crate::character_catalog::CharacterSource::Actor))
                    .or_else(|| scenes.characters.iter().position(|entry| entry.scene_index == index))
                    .unwrap();
                if !crate::viewer::Clip::Run.supports(&scenes.characters[viewer.character]) {
                    viewer.clip = crate::viewer::Clip::Idle;
                }
                menu.open = false;
                if let Ok(root) = root.single() {
                    commands.entity(root).despawn();
                }
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
            KeyCode::Backspace => match menu.focus {
                Some(Field::Port) => {
                    menu.port.pop();
                    menu.error.clear();
                }
                Some(Field::Address) => {
                    menu.address.pop();
                    menu.error.clear();
                }
                None => {}
            },
            KeyCode::Tab => {
                menu.focus = Some(if menu.focus == Some(Field::Port) {
                    Field::Address
                } else {
                    Field::Port
                })
            }
            _ => {
                if let (Some(field), Some(text)) = (menu.focus, key.text.as_ref()) {
                    let target = if field == Field::Port {
                        &mut menu.port
                    } else {
                        &mut menu.address
                    };
                    let valid = |c: char| {
                        if field == Field::Port {
                            c.is_ascii_digit()
                        } else {
                            c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-')
                        }
                    };
                    for c in text.chars().filter(|c| valid(*c)) {
                        if target.len() < 80 {
                            target.push(c);
                        }
                    }
                    menu.error.clear();
                }
            }
        }
    }
    if start {
        match menu.launch(&mut scenes, &mut game) {
            Ok(()) => {
                menu.open = false;
                if let Ok(root) = root.single() {
                    commands.entity(root).despawn();
                }
                return;
            }
            Err(error) => menu.error = error,
        }
    }
    for (action, interaction, mut background) in &mut backgrounds {
        let selected = match *action {
            Action::Scene(index) => menu.selected_scene == index,
            Action::Mode(mode) => menu.mode == mode,
            Action::Field(field) => menu.focus == Some(field),
            Action::Viewer => false,
            _ => false,
        };
        background.0 = if selected {
            Color::srgba(0.10, 0.39, 0.43, 0.98)
        } else if *interaction == Interaction::Hovered {
            Color::srgba(0.12, 0.25, 0.29, 0.98)
        } else {
            Color::srgba(0.08, 0.16, 0.19, 0.96)
        };
    }
    for (field, mut text) in &mut fields {
        text.0 = match field.0 {
            Field::Port => menu.port.clone(),
            Field::Address => menu.address.clone(),
        };
    }
    for mut text in &mut status {
        text.0 = menu.error.clone();
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
pub fn capture(
    mut commands: Commands,
    time: Res<Time>,
    menu: Res<LaunchMenu>,
    mut capture: Local<MenuCapture>,
    mut exit: MessageWriter<AppExit>,
) {
    if !menu.open {
        return;
    }
    if !capture.initialized {
        capture.initialized = true;
        let args: Vec<_> = std::env::args().collect();
        capture.path = args
            .windows(2)
            .find(|pair| pair[0] == "--capture-menu")
            .map(|pair| pair[1].clone());
    }
    let Some(path) = capture.path.clone() else {
        return;
    };
    capture.elapsed += time.delta_secs().min(0.1);
    if capture.elapsed > 2.0 && !capture.saved {
        capture.saved = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if capture.elapsed > 3.0 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_choices_apply_before_the_scene_is_loaded() {
        let mut scenes = SceneLibrary::new();
        let mut game = Game::default();
        let mut menu = LaunchMenu::new(true, &scenes);
        menu.selected_scene = scenes.index("field").unwrap();
        menu.mode = Mode::Host;
        menu.port = "4200".into();
        menu.launch(&mut scenes, &mut game).unwrap();
        assert_eq!(scenes.initial, scenes.index("field").unwrap());
        assert!(!scenes.locked, "a host may switch scenes");
        assert!(matches!(game.network, Some(Network::Host(addr)) if addr.port() == 4200));

        menu.mode = Mode::Join;
        menu.address = "127.0.0.1:4200".into();
        menu.launch(&mut scenes, &mut game).unwrap();
        assert!(matches!(game.network, Some(Network::Join(addr)) if addr.port() == 4200));

        menu.address = "not-an-address".into();
        assert!(menu.launch(&mut scenes, &mut game).is_err());
    }

    #[test]
    fn start_action_opens_the_selected_scene() {
        let scenes = SceneLibrary::new();
        let selected = scenes.initial;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<KeyboardInput>()
            .add_message::<AppExit>()
            .insert_resource(LaunchMenu::new(true, &scenes))
            .insert_resource(scenes)
            .insert_resource(Game::default())
            .init_resource::<Viewer>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Action::Start,
            Interaction::Pressed,
            BackgroundColor::default(),
        ));
        app.update();
        assert!(!app.world().resource::<LaunchMenu>().open);
        assert_eq!(app.world().resource::<SceneLibrary>().initial, selected);
        assert!(app.world().resource::<Game>().network.is_none());
    }

    #[test]
    fn studio_action_opens_the_first_available_character() {
        let scenes = SceneLibrary::new();
        let character_scene = scenes.characters.first().unwrap().scene_index;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<KeyboardInput>()
            .add_message::<AppExit>()
            .insert_resource(LaunchMenu::new(true, &scenes))
            .insert_resource(scenes)
            .insert_resource(Game::default())
            .init_resource::<Viewer>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Action::Viewer,
            Interaction::Pressed,
            BackgroundColor::default(),
        ));
        app.update();
        assert!(app.world().resource::<Viewer>().active);
        assert!(!app.world().resource::<LaunchMenu>().open);
        assert_eq!(
            app.world().resource::<SceneLibrary>().requested,
            Some(character_scene)
        );
    }
}

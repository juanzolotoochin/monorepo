//! "The tree has fallen" (Session layer, `SACRED_TREE_BALLISTA.md`, B7):
//! once the Sacred Tree's health reaches 0 (`Settlement::tree_fallen`), the
//! run is over. An overlay says so and takes the player's input; R, Enter
//! or the controller's confirm restarts the scene from the start
//! (`Flow::restart`), or, in a networked game, leaves to the menu. Other
//! scenes never fall.

use bevy::prelude::*;
use debug_ui::GameInput;
use service::{AppServices, Phase, Service, SessionLayer, SessionScoped};

use crate::flow::Flow;
use crate::input::InputService;
use crate::lockstep::SessionService;
use crate::menu::{font, BACKDROP, MUTED, TEXT};

#[derive(Resource, Default)]
pub struct Fallen {
    /// The tree has fallen in this session.
    pub shown: bool,
}

impl Service for Fallen {
    type Layer = SessionLayer;
    type Deps = (Flow, SessionService);

    fn init(_world: &mut World) -> Self {
        Fallen::default()
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Input, (watch, restart, show).chain());
    }
}

#[derive(Component)]
#[require(SessionScoped)]
struct Overlay;

/// Notices the tree falling in the confirmed world.
fn watch(session: Res<SessionService>, mut fallen: ResMut<Fallen>) {
    if !fallen.shown && session.settlement().is_some_and(|s| s.tree_fallen()) {
        fallen.shown = true;
    }
}

/// While shown, nothing else in the session sees the player's input; R,
/// Enter or confirm restarts (or leaves, when networked).
fn restart(fallen: Res<Fallen>, session: Res<SessionService>, mut game: ResMut<GameInput>, mut input: ResMut<InputService>, mut flow: ResMut<Flow>) {
    if !fallen.shown {
        return;
    }
    let keys = game.keys();
    let pressed = keys.just_pressed(KeyCode::KeyR) || keys.just_pressed(KeyCode::Enter) || input.take_confirm();
    game.block();
    input.block();
    if !pressed {
        return;
    }
    if session.networked() || !flow.restart() {
        flow.leave(None);
    }
}

fn show(mut commands: Commands, fallen: Res<Fallen>, session: Res<SessionService>, overlays: Query<Entity, With<Overlay>>) {
    if !fallen.shown || !overlays.is_empty() {
        return;
    }
    let hint = if session.networked() { "Press R or A to leave to the menu" } else { "Press R or A to start again" };
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
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        GlobalZIndex(55),
        children![(
            Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(10), padding: UiRect::all(px(28)), ..default() },
            BackgroundColor(BACKDROP),
            children![
                (Text::new("THE TREE HAS FALLEN"), font(28.0), TextColor(TEXT)),
                (Text::new(hint), font(13.0), TextColor(MUTED)),
            ],
        )],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::SceneLibrary;
    use crate::flow::Launch;

    fn overlay_shown(app: &mut App) -> bool {
        let world = app.world_mut();
        world.query::<&Text>().iter(world).any(|text| text.0 == "THE TREE HAS FALLEN")
    }

    /// The overlay shows once the tree falls, and a restart starts the
    /// scene again from scratch, the tree standing.
    #[test]
    fn a_fallen_tree_shows_its_overlay_and_the_scene_restarts() {
        let scenes = SceneLibrary::new();
        let scene = scenes.index("sacred-tree").unwrap();
        let options = crate::control::Options { simulation: true, headless_menus: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, scenes, hub, Launch::single_player(scene));
        crate::start_world(&mut app);
        assert!(!app.world().resource::<Fallen>().shown);
        app.world_mut().resource_mut::<Fallen>().shown = true;
        app.update();
        assert!(overlay_shown(&mut app));
        assert!(app.world_mut().resource_mut::<Flow>().restart());
        // The session ends, then the same scene opens again.
        let fresh = |app: &App| {
            let active = app.world().get_resource::<State<service::WorldState>>().is_some_and(|s| *s.get() == service::WorldState::Active);
            active && app.world().get_resource::<Fallen>().is_some_and(|f| !f.shown)
        };
        for _ in 0..60 {
            app.update();
            if fresh(&app) {
                break;
            }
        }
        assert!(fresh(&app), "the scene started again");
        assert_eq!(app.world().resource::<crate::scene_view::SceneView>().index, scene);
        assert!(!overlay_shown(&mut app));
        let tree = app.world().resource::<SessionService>().settlement().map(|s| (s.health, s.tree_fallen()));
        assert_eq!(tree, Some((20, false)));
    }
}

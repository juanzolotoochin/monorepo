//! What the game does from launch (App layer): the main menu, or a session
//! the command line asked for (hosting, joining, a replay, or single-player).
//! `Flow` is the only code that starts and leaves sessions.

use bevy::prelude::*;
use save::InputLog;
use service::{AppLayer, AppServices, AppState, Phase, Service, SessionState};

use crate::lockstep::Network;

/// How to start a session.
#[derive(Resource)]
pub struct Launch {
    pub network: Option<Network>,
    /// The scene a hosted or single-player session starts in (an index into
    /// the `SceneLibrary`).
    pub scene: usize,
    /// What other players see over this player's avatar.
    pub name: String,
    /// Where to write each world's recording.
    pub record: Option<String>,
    /// A recording to play back instead of live input.
    pub replay: Option<(Vec<u8>, InputLog)>,
}

impl Launch {
    /// A single-player session in a scene, as tests start the game.
    #[cfg(test)]
    pub fn single_player(scene: usize) -> Launch {
        Launch { network: None, scene, name: String::new(), record: None, replay: None }
    }
}

/// What the menu's fields start with: the command line's choices, or defaults.
#[derive(Clone)]
pub struct Defaults {
    pub name: String,
    pub scene: usize,
    pub record: Option<String>,
    pub port: u16,
    pub address: String,
}

/// The port the menu offers for hosting, unless `--host` named one.
pub const DEFAULT_PORT: u16 = 7777;

/// What the App layer shows while no session runs: the main menu, or
/// Character Studio. Studio is not a session: it poses one character on a
/// stage of its own, and nothing it shows lives on once it closes.
#[derive(bevy::state::state::SubStates, Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
#[source(SessionState = SessionState::Off)]
pub enum Front {
    #[default]
    Menu,
    Studio,
}

/// What the game does at launch: start a session, or show the main menu
/// (no launch).
#[derive(Resource)]
pub struct Start {
    pub launch: Option<Launch>,
    pub defaults: Defaults,
}

#[derive(Resource)]
pub struct Flow {
    /// The session to start at the next Input phase.
    launch: Option<Launch>,
    /// The started session, until the Session layer takes it.
    opened: Option<crate::lockstep::Opened>,
    /// Leave the session at the next Input phase.
    leaving: bool,
    /// Why the last session ended or failed to start, for the menu.
    pub ended: Option<String>,
    pub defaults: Defaults,
    /// A text field has the keyboard, so the app's keys don't apply.
    pub typing: bool,
}

impl Service for Flow {
    type Layer = AppLayer;
    type Deps = ();

    fn init(world: &mut World) -> Self {
        let Start { launch, defaults } = world.remove_resource::<Start>().expect("the game starts from a Start");
        Flow { launch, opened: None, leaving: false, ended: None, defaults, typing: false }
    }

    fn systems(app: &mut App) {
        app.add_sub_state::<Front>()
            .add_service_systems::<Self, _>(Phase::Input, (follow, app_keys.run_if(crate::control::interactive)));
    }
}

impl Flow {
    /// Starts a session with this launch at the next Input phase.
    pub fn start(&mut self, launch: Launch) {
        self.launch = Some(launch);
        self.ended = None;
    }

    /// Leaves the session at the next Input phase; `reason` says why, if it
    /// ended on its own.
    pub fn leave(&mut self, reason: Option<String>) {
        self.launch = None;
        self.leaving = true;
        self.ended = reason;
    }

    /// The session about to start, if one is.
    #[cfg(test)]
    pub fn starting(&self) -> Option<&Launch> {
        self.launch.as_ref()
    }

    /// The opened session, for the Session layer starting now.
    pub fn take_opened(&mut self) -> Option<crate::lockstep::Opened> {
        self.opened.take()
    }
}

/// `--host <port>`: hosts on every interface.
pub fn host(port: &str) -> Result<Network, String> {
    let port: u16 = port.trim().parse().map_err(|e| format!("Invalid port {port}: {e}"))?;
    Ok(Network::Host(([0, 0, 0, 0], port).into()))
}

/// `--join <host:port>`: the host may be a name, such as a Tailscale machine
/// name.
pub fn join(address: &str) -> Result<Network, String> {
    use std::net::ToSocketAddrs;
    let address = address.trim();
    let addr = address
        .to_socket_addrs()
        .map_err(|e| format!("Cannot resolve {address}: {e}"))?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| format!("No IPv4 address for {address}"))?;
    Ok(Network::Join(addr))
}

/// Escape quits (when there is no menu to go back to); M mutes.
fn app_keys(
    input: Res<debug_ui::GameInput>,
    flow: Res<Flow>,
    options: Res<crate::control::Options>,
    mut exit: MessageWriter<AppExit>,
    mut sound: ResMut<crate::sound::Sound>,
) {
    if flow.typing {
        return;
    }
    let keys = input.keys();
    if keys.just_pressed(KeyCode::Escape) && !options.menus() {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        sound.muted = !sound.muted;
    }
}

/// Starts the launch's session once the App layer is up, and leaves a
/// session when asked. A session that can't start leaves its reason for the
/// menu, or without a menu ends the game.
fn follow(world: &mut World) {
    let app = *world.resource::<State<AppState>>().get();
    let Some(session) = world.get_resource::<State<SessionState>>().map(|s| *s.get()) else { return };
    if app != AppState::Active {
        return;
    }
    let mut flow = world.resource_mut::<Flow>();
    if std::mem::take(&mut flow.leaving) && session != SessionState::Off {
        world.resource_mut::<NextState<SessionState>>().set(SessionState::Off);
        return;
    }
    if session != SessionState::Off {
        return;
    }
    let Some(launch) = flow.launch.take() else { return };
    match crate::lockstep::open(world, launch) {
        Ok(opened) => {
            world.resource_mut::<Flow>().opened = Some(opened);
            world.resource_mut::<NextState<SessionState>>().set(SessionState::Loading);
        }
        Err(reason) => {
            if world.resource::<crate::control::Options>().menus() {
                warn!("{reason}");
                world.resource_mut::<Flow>().ended = Some(reason);
            } else {
                error!("{reason}");
                world.write_message(AppExit::error());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::SceneLibrary;
    use service::WorldState;

    fn state<S: States + Copy>(app: &App) -> Option<S> {
        app.world().get_resource::<State<S>>().map(|s| *s.get())
    }

    #[test]
    fn a_session_can_be_left_and_another_started() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, SceneLibrary::new(), hub, Launch::single_player(0));
        crate::start_world(&mut app);
        // A launch queued while the menu was closing must not restart the
        // session when the player leaves it.
        app.world_mut().resource_mut::<Flow>().start(Launch::single_player(0));
        app.world_mut().resource_mut::<Flow>().leave(Some("The host left the game.".into()));
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(state::<SessionState>(&app), Some(SessionState::Off));
        assert_eq!(state::<Front>(&app), Some(Front::Menu));
        assert_eq!(state::<WorldState>(&app), None);
        assert!(app.world().get_resource::<crate::lockstep::SessionService>().is_none());
        assert_eq!(app.world().resource::<Flow>().ended.as_deref(), Some("The host left the game."));

        app.world_mut().resource_mut::<Flow>().start(Launch::single_player(1));
        crate::start_world(&mut app);
        assert_eq!(app.world().resource::<crate::scene_view::SceneView>().index, 1);
        assert_eq!(app.world().resource::<Flow>().ended, None);
    }

    #[test]
    fn a_session_that_cannot_start_ends_the_run_without_a_menu() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        // Something else holds the port.
        let taken = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        let launch = Launch { network: Some(host(&taken.local_addr().unwrap().port().to_string()).unwrap()), ..Launch::single_player(0) };
        let mut app = crate::test_app(options, SceneLibrary::new(), hub, launch);
        let mut exit = None;
        for _ in 0..5 {
            app.update();
            exit = exit.or(app.should_exit());
        }
        assert_eq!(state::<SessionState>(&app), Some(SessionState::Off));
        assert!(exit.as_ref().is_some_and(AppExit::is_error), "{exit:?}");
    }
}

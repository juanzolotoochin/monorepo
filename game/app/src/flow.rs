//! What the game does from launch (App layer): the main menu, or a session
//! the command line asked for (hosting, joining, a replay, or single-player).
//! `Flow` is the only code that starts sessions.

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
pub const DEFAULT_PORT: u16 = 7000;

/// What the game does at launch: start a session, or show the main menu
/// (no launch).
#[derive(Resource)]
pub struct Start {
    pub launch: Option<Launch>,
    pub defaults: Defaults,
}

#[derive(Resource)]
pub struct Flow {
    /// The session to start, until the Session layer takes it.
    launch: Option<Launch>,
    pub defaults: Defaults,
    /// A text field has the keyboard, so the app's keys don't apply.
    pub typing: bool,
}

impl Service for Flow {
    type Layer = AppLayer;
    type Deps = ();

    fn init(world: &mut World) -> Self {
        let Start { launch, defaults } = world.remove_resource::<Start>().expect("the game starts from a Start");
        Flow { launch, defaults, typing: false }
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Input, (start_session, app_keys.run_if(crate::control::interactive)));
    }
}

impl Flow {
    /// Starts a session with this launch at the next Input phase.
    pub fn start(&mut self, launch: Launch) {
        self.launch = Some(launch);
    }

    /// The launch, for the session starting now.
    pub fn take_launch(&mut self) -> Option<Launch> {
        self.launch.take()
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
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<Flow>,
    options: Res<crate::control::Options>,
    mut exit: MessageWriter<AppExit>,
    mut sound: ResMut<crate::sound::Sound>,
) {
    if flow.typing {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && !options.menus() {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        sound.muted = !sound.muted;
    }
}

/// Starts the launch's session once the App layer is up.
fn start_session(
    flow: Res<Flow>,
    app: Res<State<AppState>>,
    session: Option<Res<State<SessionState>>>,
    next: Option<ResMut<NextState<SessionState>>>,
) {
    let (Some(session), Some(mut next)) = (session, next) else { return };
    if *app.get() == AppState::Active && *session.get() == SessionState::Off && flow.launch.is_some() {
        next.set(SessionState::Loading);
    }
}

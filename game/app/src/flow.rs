//! What the game does from launch (App layer). Today that is one session,
//! started from the command line: hosting, joining, a replay, or
//! single-player. The main menu will take over choosing it.

use bevy::prelude::*;
use save::InputLog;
use service::{AppLayer, AppServices, AppState, Phase, Service, SessionState};

use crate::lockstep::Network;

/// How the command line asked to play.
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

#[derive(Resource)]
pub struct Flow {
    /// The session to start, until the Session layer takes it.
    launch: Option<Launch>,
}

impl Service for Flow {
    type Layer = AppLayer;
    type Deps = ();

    fn init(world: &mut World) -> Self {
        Flow { launch: world.remove_resource::<Launch>() }
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Input, (start_session, app_keys.run_if(crate::control::interactive)));
    }
}

impl Flow {
    /// The launch, for the session starting now.
    pub fn take_launch(&mut self) -> Option<Launch> {
        self.launch.take()
    }
}

/// Escape quits; M mutes.
fn app_keys(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>, mut sound: ResMut<crate::sound::Sound>) {
    if keys.just_pressed(KeyCode::Escape) {
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

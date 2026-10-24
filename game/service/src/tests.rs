use super::*;

/// What services did, in order.
#[derive(Resource, Default)]
struct Log {
    life: Vec<String>,
    ticks: Vec<String>,
}

/// Whether `Slow` is ready.
#[derive(Resource, Default)]
struct SlowReady(bool);

macro_rules! service {
    ($name:ident, $layer:ty, $deps:ty) => {
        #[derive(Resource)]
        struct $name;

        impl Service for $name {
            type Layer = $layer;
            type Deps = $deps;

            fn init(world: &mut World) -> Self {
                world.resource_mut::<Log>().life.push(concat!("init ", stringify!($name)).into());
                $name
            }

            fn shutdown(self, world: &mut World) {
                world.resource_mut::<Log>().life.push(concat!("shutdown ", stringify!($name)).into());
            }

            fn systems(app: &mut App) {
                // Reading the service proves it exists whenever the system runs.
                app.add_service_systems::<Self, _>(Phase::Input, |_: Res<$name>, mut log: ResMut<Log>| {
                    log.ticks.push(stringify!($name).into())
                });
            }
        }
    };
}

service!(A1, AppLayer, ());
service!(A2, AppLayer, (A1,));
service!(S1, SessionLayer, (A1,));
service!(W1, WorldLayer, ());
service!(W2, WorldLayer, (W1, S1));

#[derive(Resource)]
struct Slow;

impl Service for Slow {
    type Layer = WorldLayer;
    type Deps = ();

    fn init(_: &mut World) -> Self {
        Slow
    }

    fn ready(&self, world: &World) -> bool {
        world.resource::<SlowReady>().0
    }
}

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<Log>().init_resource::<SlowReady>();
    app
}

/// Registered out of layer order on purpose: layers decide first.
fn layered() -> App {
    let mut app = app();
    app.add_service::<W1>()
        .add_service::<A1>()
        .add_service::<S1>()
        .add_service::<W2>()
        .add_service::<A2>();
    app.finish();
    app.cleanup();
    app
}

fn set<S: FreelyMutableState>(app: &mut App, state: S) {
    app.world_mut().resource_mut::<NextState<S>>().set(state);
}

fn state<S: States + Copy>(app: &App) -> Option<S> {
    app.world().get_resource::<State<S>>().map(|s| *s.get())
}

fn take_life(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world_mut().resource_mut::<Log>().life)
}

fn take_ticks(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world_mut().resource_mut::<Log>().ticks)
}

/// Runs frames until the session and its world are active.
fn into_world(app: &mut App) {
    app.update();
    app.update();
    set(app, SessionState::Loading);
    app.update();
    app.update();
    set(app, WorldState::Loading);
    app.update();
    app.update();
}

#[test]
fn layers_start_in_order_and_stop_in_reverse() {
    let mut app = layered();
    app.update();
    assert_eq!(state::<AppState>(&app), Some(AppState::Loading));
    assert_eq!(take_life(&mut app), ["init A1", "init A2"]);
    app.update();
    assert_eq!(state::<AppState>(&app), Some(AppState::Active));
    assert_eq!(state::<SessionState>(&app), Some(SessionState::Off));
    assert!(take_life(&mut app).is_empty());

    set(&mut app, SessionState::Loading);
    app.update();
    assert_eq!(take_life(&mut app), ["init S1"]);
    app.update();
    assert_eq!(state::<SessionState>(&app), Some(SessionState::Active));
    assert_eq!(state::<WorldState>(&app), Some(WorldState::Off));

    set(&mut app, WorldState::Loading);
    app.update();
    assert_eq!(take_life(&mut app), ["init W1", "init W2"]);
    app.update();
    assert_eq!(state::<WorldState>(&app), Some(WorldState::Active));
    assert!(take_life(&mut app).is_empty(), "going from Loading to Active starts nothing");

    // Leaving the session takes the world with it, world first.
    set(&mut app, SessionState::Off);
    app.update();
    assert_eq!(take_life(&mut app), ["shutdown W2", "shutdown W1", "shutdown S1"]);
    assert_eq!(state::<WorldState>(&app), None);
    assert!(!app.world().contains_resource::<W1>());
    assert!(!app.world().contains_resource::<S1>());
    assert!(app.world().contains_resource::<A1>());
}

#[test]
fn a_world_reloads_inside_one_session() {
    let mut app = layered();
    into_world(&mut app);
    take_life(&mut app);
    // Back to the lobby, then into another world.
    set(&mut app, WorldState::Off);
    app.update();
    assert_eq!(take_life(&mut app), ["shutdown W2", "shutdown W1"]);
    assert_eq!(state::<SessionState>(&app), Some(SessionState::Active));
    set(&mut app, WorldState::Loading);
    app.update();
    assert_eq!(take_life(&mut app), ["init W1", "init W2"]);
}

#[test]
fn services_tick_by_layer_then_registration() {
    let mut app = layered();
    app.update();
    // Only the App layer runs: no system reads a missing service.
    assert_eq!(take_ticks(&mut app), ["A1", "A2"]);
    into_world(&mut app);
    take_ticks(&mut app);
    app.update();
    assert_eq!(take_ticks(&mut app), ["A1", "A2", "S1", "W1", "W2"]);
}

#[test]
fn a_layer_waits_for_every_service_to_be_ready() {
    let mut app = app();
    app.add_service::<W1>().add_service::<Slow>();
    app.finish();
    app.cleanup();
    into_world(&mut app);
    for _ in 0..3 {
        app.update();
        assert_eq!(state::<WorldState>(&app), Some(WorldState::Loading));
    }
    app.world_mut().resource_mut::<SlowReady>().0 = true;
    app.update();
    app.update();
    assert_eq!(state::<WorldState>(&app), Some(WorldState::Active));
}

#[test]
fn every_layer_stops_when_the_app_exits() {
    let mut app = layered();
    into_world(&mut app);
    take_life(&mut app);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert_eq!(
        take_life(&mut app),
        ["shutdown W2", "shutdown W1", "shutdown S1", "shutdown A2", "shutdown A1"]
    );
}

#[test]
#[should_panic(expected = "which is registered after it")]
fn a_dependency_registered_later_panics() {
    let mut app = app();
    app.add_service::<A1>().add_service::<S1>().add_service::<W2>().add_service::<W1>();
    app.finish();
}

#[test]
#[should_panic(expected = "which isn't registered")]
fn a_missing_dependency_panics() {
    let mut app = app();
    app.add_service::<A1>().add_service::<W1>().add_service::<W2>();
    app.finish();
}

#[test]
#[should_panic(expected = "registered twice")]
fn a_service_registered_twice_panics() {
    let mut app = app();
    app.add_service::<A1>().add_service::<A1>();
}

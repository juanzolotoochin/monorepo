//! Service layers for the game's front end. See
//! `kestrel/docs/service-layers.md`.
//!
//! A service is a singleton Bevy resource that belongs to one of three
//! layers: App (the whole process), Session (from HOST, JOIN or PLAY until
//! leaving) and World (one loaded simulation world). Lower layers start before
//! and stop after higher ones. Within a layer, services start, tick and (in
//! reverse) stop in registration order.
//!
//! Each layer is a Bevy state with `Loading` and `Active`. `SessionState` and
//! `WorldState` are sub-states with an `Off` value: a layer runs while its
//! state is `Loading` or `Active`. Front ends change layers through
//! `NextState`; a layer moves from `Loading` to `Active` by itself once every
//! service in it is ready.
//!
//! Add [`ServicesPlugin`] after `DefaultPlugins` or `MinimalPlugins`: it adds
//! Bevy's `StatesPlugin` only if nothing else did.

use std::any::{type_name, TypeId};
use std::marker::PhantomData;

use bevy::app::AppExit;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::state::state::{
    last_transition, EnterSchedules, ExitSchedules, FreelyMutableState, StateTransition,
    StateTransitionEvent, StateTransitionSystems,
};

#[cfg(test)]
mod tests;

/// The App layer's state. The layer runs for the whole process.
#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum AppState {
    #[default]
    Loading,
    Active,
}

/// The Session layer's state. It exists once the App layer is active.
#[derive(SubStates, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[source(AppState = AppState::Active)]
pub enum SessionState {
    #[default]
    Off,
    Loading,
    Active,
}

/// The World layer's state. It exists once the Session layer is active; while
/// it is `Off` the session is in its lobby.
#[derive(SubStates, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[source(SessionState = SessionState::Active)]
pub enum WorldState {
    #[default]
    Off,
    Loading,
    Active,
}

/// A layer's state: the layer runs while it is `LOADING` or `ACTIVE`.
pub trait LayerState: FreelyMutableState + Copy {
    const LOADING: Self;
    const ACTIVE: Self;

    fn runs(self) -> bool {
        self == Self::LOADING || self == Self::ACTIVE
    }
}

impl LayerState for AppState {
    const LOADING: Self = AppState::Loading;
    const ACTIVE: Self = AppState::Active;
}

impl LayerState for SessionState {
    const LOADING: Self = SessionState::Loading;
    const ACTIVE: Self = SessionState::Active;
}

impl LayerState for WorldState {
    const LOADING: Self = WorldState::Loading;
    const ACTIVE: Self = WorldState::Active;
}

/// One of the three layers, named by a marker type.
pub trait Layer: Send + Sync + 'static {
    type State: LayerState;
    /// 0 for App, 1 for Session, 2 for World.
    const RANK: u8;
}

pub struct AppLayer;
pub struct SessionLayer;
pub struct WorldLayer;

impl Layer for AppLayer {
    type State = AppState;
    const RANK: u8 = 0;
}

impl Layer for SessionLayer {
    type State = SessionState;
    const RANK: u8 = 1;
}

impl Layer for WorldLayer {
    type State = WorldState;
    const RANK: u8 = 2;
}

/// Holds when `Self` is layer `L` or a layer below it: what a service in `L`
/// may depend on.
pub trait AtOrBelow<L: Layer> {}

impl AtOrBelow<AppLayer> for AppLayer {}
impl AtOrBelow<SessionLayer> for AppLayer {}
impl AtOrBelow<SessionLayer> for SessionLayer {}
impl AtOrBelow<WorldLayer> for AppLayer {}
impl AtOrBelow<WorldLayer> for SessionLayer {}
impl AtOrBelow<WorldLayer> for WorldLayer {}

/// Marks an entity that lives as long as layer `L`: it is despawned when the
/// layer stops, after the layer's services have.
#[derive(Component)]
pub struct Scoped<L: Layer>(PhantomData<L>);

impl<L: Layer> Default for Scoped<L> {
    fn default() -> Self {
        Scoped(PhantomData)
    }
}

/// An entity of the current world.
pub type WorldScoped = Scoped<WorldLayer>;
/// An entity of the current session.
pub type SessionScoped = Scoped<SessionLayer>;

/// A front-end service: one per type, living as long as its layer.
pub trait Service: Resource + Sized {
    type Layer: Layer;
    /// The services this one uses, as a tuple: `()`, `(A,)`, `(A, B)`... Each
    /// must be in this layer or below (checked when compiling) and, if in this
    /// layer, registered earlier (checked when the app is built).
    type Deps: Deps<Self::Layer>;

    /// Runs when the layer starts, after every service registered before it.
    /// Slow work belongs in systems that run while the layer is loading.
    fn init(world: &mut World) -> Self;

    /// Runs when the layer stops, in reverse registration order.
    fn shutdown(self, _world: &mut World) {}

    /// Polled while the layer is loading; the layer becomes active once every
    /// service in it is ready.
    fn ready(&self, _world: &World) -> bool {
        true
    }

    /// Adds the service's systems, each in a phase with
    /// [`AppServices::add_service_systems`].
    fn systems(_app: &mut App) {}
}

/// A dependency, for the checks made when the app is built.
pub struct Dep {
    id: TypeId,
    name: &'static str,
}

impl Dep {
    fn of<S: Service>() -> Self {
        Dep { id: TypeId::of::<S>(), name: type_name::<S>() }
    }
}

/// A tuple of services that a service in layer `L` may depend on.
pub trait Deps<L: Layer> {
    fn list() -> Vec<Dep>;
}

impl<L: Layer> Deps<L> for () {
    fn list() -> Vec<Dep> {
        Vec::new()
    }
}

macro_rules! deps {
    ($($t:ident),+) => {
        impl<L: Layer, $($t: Service),+> Deps<L> for ($($t,)+)
        where
            $(<$t as Service>::Layer: AtOrBelow<L>),+
        {
            fn list() -> Vec<Dep> {
                vec![$(Dep::of::<$t>()),+]
            }
        }
    };
}

deps!(A);
deps!(A, B);
deps!(A, B, C);
deps!(A, B, C, D);
deps!(A, B, C, D, E);
deps!(A, B, C, D, E, F);
deps!(A, B, C, D, E, F, G);
deps!(A, B, C, D, E, F, G, H);

/// The parts of a frame, run in this order in `Update`. Inside each phase, App
/// services run before Session services, which run before World services,
/// and services in one layer run in registration order.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Phase {
    /// Devices and scripts become actions, actions become commands.
    Input,
    /// The session runs the network and every tick due.
    Step,
    /// New ticks are copied into view state.
    Mirror,
    /// Views, animation, the camera and the HUD.
    Present,
}

impl Phase {
    pub const ALL: [Phase; 4] = [Phase::Input, Phase::Step, Phase::Mirror, Phase::Present];
}

/// One service's systems in one phase.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ServiceSet {
    phase: Phase,
    service: TypeId,
    name: &'static str,
}

impl ServiceSet {
    pub fn of<S: Service>(phase: Phase) -> Self {
        ServiceSet { phase, service: TypeId::of::<S>(), name: type_name::<S>() }
    }
}

/// Stops every layer when the app exits, in `Last`. A system that writes
/// `AppExit` in `Last` runs `.before(StopOnExit)` so the services still stop
/// that frame.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StopOnExit;

/// The systems that start and stop a layer's services.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Lifecycle {
    Start(u8),
    Stop(u8),
}

struct Entry {
    id: TypeId,
    name: &'static str,
    rank: u8,
    deps: Vec<Dep>,
    init: fn(&mut World),
    shutdown: fn(&mut World),
    ready: fn(&World) -> bool,
}

/// Every registered service, in registration order.
#[derive(Resource, Default)]
struct Registry {
    entries: Vec<Entry>,
}

impl Registry {
    fn inits(&self, rank: u8) -> Vec<fn(&mut World)> {
        self.entries.iter().filter(|e| e.rank == rank).map(|e| e.init).collect()
    }

    fn shutdowns(&self, rank: u8) -> Vec<fn(&mut World)> {
        self.entries.iter().rev().filter(|e| e.rank == rank).map(|e| e.shutdown).collect()
    }

    fn readies(&self, rank: u8) -> Vec<fn(&World) -> bool> {
        self.entries.iter().filter(|e| e.rank == rank).map(|e| e.ready).collect()
    }
}

fn init_service<S: Service>(world: &mut World) {
    let service = S::init(world);
    world.insert_resource(service);
}

fn shutdown_service<S: Service>(world: &mut World) {
    if let Some(service) = world.remove_resource::<S>() {
        service.shutdown(world);
    }
}

fn service_ready<S: Service>(world: &World) -> bool {
    world.get_resource::<S>().is_some_and(|s| s.ready(world))
}

pub trait AppServices {
    /// Registers a service. Its layer starts it, ticks its systems and stops it.
    fn add_service<S: Service>(&mut self) -> &mut Self;

    /// Adds systems to a service's slot in a phase. They run only while the
    /// service's layer runs.
    fn add_service_systems<S: Service, M>(
        &mut self,
        phase: Phase,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;
}

impl AppServices for App {
    fn add_service<S: Service>(&mut self) -> &mut Self {
        if !self.is_plugin_added::<ServicesPlugin>() {
            self.add_plugins(ServicesPlugin);
        }
        let mut registry = self.world_mut().resource_mut::<Registry>();
        if registry.entries.iter().any(|e| e.id == TypeId::of::<S>()) {
            panic!("service {} is registered twice", type_name::<S>());
        }
        registry.entries.push(Entry {
            id: TypeId::of::<S>(),
            name: type_name::<S>(),
            rank: S::Layer::RANK,
            deps: S::Deps::list(),
            init: init_service::<S>,
            shutdown: shutdown_service::<S>,
            ready: service_ready::<S>,
        });
        S::systems(self);
        self
    }

    fn add_service_systems<S: Service, M>(
        &mut self,
        phase: Phase,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_systems(Update, systems.in_set(ServiceSet::of::<S>(phase)))
    }
}

/// Sets up the layer states, the phases, and starting and stopping services.
/// Checks the services' dependencies and orders their systems when the app
/// is finished.
pub struct ServicesPlugin;

impl Plugin for ServicesPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.init_state::<AppState>()
            .add_sub_state::<SessionState>()
            .add_sub_state::<WorldState>()
            .init_resource::<Registry>()
            .configure_sets(Update, (Phase::Input, Phase::Step, Phase::Mirror, Phase::Present).chain());
        lifecycle::<AppLayer>(app);
        lifecycle::<SessionLayer>(app);
        lifecycle::<WorldLayer>(app);
        // Bevy runs a sub-state's exit schedules before its parent's and its
        // enter schedules after. A layer starts its services just before its
        // own enter schedules and stops them just after its exit schedules,
        // so a service exists during its layer's OnEnter and OnExit systems,
        // and World services stop before Session services.
        app.configure_sets(
            StateTransition,
            (
                Lifecycle::Start(1).after(EnterSchedules::<AppState>::default()),
                Lifecycle::Start(2).after(EnterSchedules::<SessionState>::default()),
                Lifecycle::Stop(2).before(ExitSchedules::<SessionState>::default()),
                Lifecycle::Stop(1).before(ExitSchedules::<AppState>::default()),
            ),
        )
        .add_systems(Last, (promote::<AppLayer>, promote::<SessionLayer>, promote::<WorldLayer>))
        .add_systems(Last, exiting.pipe(stop_all).after(promote::<WorldLayer>).in_set(StopOnExit));
    }

    fn finish(&self, app: &mut App) {
        let registry = app.world().resource::<Registry>();
        let entries = &registry.entries;
        for (index, entry) in entries.iter().enumerate() {
            for dep in &entry.deps {
                match entries.iter().position(|e| e.id == dep.id) {
                    None => panic!("service {} depends on {}, which isn't registered", entry.name, dep.name),
                    Some(at) if entries[at].rank == entry.rank && at > index => panic!(
                        "service {} depends on {}, which is registered after it; register {} first",
                        entry.name, dep.name, dep.name
                    ),
                    Some(_) => {}
                }
            }
        }
        // Lower layers first, then registration order (the sort is stable).
        let mut order: Vec<_> = entries.iter().map(|e| (e.rank, e.id, e.name)).collect();
        order.sort_by_key(|(rank, _, _)| *rank);
        for phase in Phase::ALL {
            let mut previous: Option<ServiceSet> = None;
            for &(rank, service, name) in &order {
                let set = ServiceSet { phase, service, name };
                let mut config: ScheduleConfigs<_> = set.clone().in_set(phase);
                if let Some(previous) = previous.replace(set) {
                    config = config.after(previous);
                }
                config = match rank {
                    0 => config.run_if(runs::<AppLayer>),
                    1 => config.run_if(runs::<SessionLayer>),
                    _ => config.run_if(runs::<WorldLayer>),
                };
                app.configure_sets(Update, config);
            }
        }
    }
}

fn lifecycle<L: Layer>(app: &mut App) {
    app.configure_sets(
        StateTransition,
        (
            Lifecycle::Start(L::RANK)
                .in_set(StateTransitionSystems::EnterSchedules)
                .before(EnterSchedules::<L::State>::default()),
            Lifecycle::Stop(L::RANK)
                .in_set(StateTransitionSystems::ExitSchedules)
                .after(ExitSchedules::<L::State>::default()),
        ),
    )
    .add_systems(
        StateTransition,
        (
            last_transition::<L::State>.pipe(start::<L>).in_set(Lifecycle::Start(L::RANK)),
            last_transition::<L::State>.pipe(stop::<L>).in_set(Lifecycle::Stop(L::RANK)),
        ),
    );
}

/// Present once the app has exited and every service has stopped.
#[derive(Resource)]
struct Stopped;

fn runs<L: Layer>(state: Option<Res<State<L::State>>>, stopped: Option<Res<Stopped>>) -> bool {
    stopped.is_none() && state.is_some_and(|s| s.get().runs())
}

fn changes<S: LayerState>(transition: &StateTransitionEvent<S>) -> (bool, bool) {
    (transition.exited.is_some_and(S::runs), transition.entered.is_some_and(S::runs))
}

fn start<L: Layer>(transition: In<Option<StateTransitionEvent<L::State>>>, world: &mut World) {
    let Some(transition) = transition.0 else { return };
    if let (false, true) = changes(&transition) {
        for init in world.resource::<Registry>().inits(L::RANK) {
            init(world);
        }
    }
}

fn stop<L: Layer>(transition: In<Option<StateTransitionEvent<L::State>>>, world: &mut World) {
    let Some(transition) = transition.0 else { return };
    if let (true, false) = changes(&transition) {
        stop_layer::<L>(world);
    }
}

/// Stops a layer's services, in reverse registration order, then despawns
/// its scoped entities.
fn stop_layer<L: Layer>(world: &mut World) {
    for shutdown in world.resource::<Registry>().shutdowns(L::RANK) {
        shutdown(world);
    }
    let scoped: Vec<_> = world.query_filtered::<Entity, With<Scoped<L>>>().iter(world).collect();
    for entity in scoped {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
    }
}

/// Moves a loading layer to active once all its services are ready, unless
/// something already asked for another state this frame.
fn promote<L: Layer>(world: &mut World) {
    if world.get_resource::<State<L::State>>().map(|s| *s.get()) != Some(L::State::LOADING) {
        return;
    }
    if !matches!(world.resource::<NextState<L::State>>(), NextState::Unchanged) {
        return;
    }
    let readies = world.resource::<Registry>().readies(L::RANK);
    if readies.iter().all(|ready| ready(world)) {
        world.resource_mut::<NextState<L::State>>().set(L::State::ACTIVE);
    }
}

fn exiting(mut exits: MessageReader<AppExit>) -> bool {
    exits.read().next().is_some()
}

/// Stops every layer, highest first, when the app exits.
fn stop_all(exiting: In<bool>, world: &mut World) {
    if !exiting.0 {
        return;
    }
    stop_layer::<WorldLayer>(world);
    stop_layer::<SessionLayer>(world);
    stop_layer::<AppLayer>(world);
    // An app driven by hand (a test) may run more frames: nothing of a
    // service's runs in them.
    world.insert_resource(Stopped);
}

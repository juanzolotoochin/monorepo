//! The game's simulation: every scene runs on the deterministic lockstep
//! simulation (`kestrel/sim`) through a session (`kestrel/session`). Three services
//! (see kestrel/docs/service-layers.md) split the work:
//!
//! - [`SessionService`] (Session layer) owns the session: it builds each
//!   scene's world, steps it on Bevy's clock, and moves the World layer
//!   through its states as worlds come and go.
//! - [`WorldMirror`] (World layer) keeps the last two ticks of the world for
//!   the views to interpolate.
//! - [`PlayerControl`] (World layer) turns input into commands.
//!
//! Views read the mirrors (`legacy`, piles, memory, flight); nothing moves
//! gameplay state except commands.
//!
//! With `--host` or `--join` the session is networked: the host builds the
//! world and clients load it from the host, then each player drives their
//! own avatar (`peers` draws the others).

use std::net::SocketAddr;
use std::time::Duration;

use bevy::prelude::*;
use content::Content;
use fx::{Fx, FxVec2};
use net::NetConfig;
use service::{AppServices, Phase, Service, SessionLayer, WorldLayer, WorldState};
use sim::settlement::avatar::Intent;
use sim::settlement::{PileSetup, SceneSetup, Settlement as SimSettlement};
use sim::{CommandKind, World as SimWorld};

use crate::assets::SceneLibrary;
use crate::flow::Flow;
use crate::character::Player;
use crate::gameplay::Controls;
use crate::scene_view::SceneView;

/// The world seed. Scenes don't use randomness outside combat rolls, which are
/// keyed on ticks and IDs, so one seed serves every scene.
const SEED: u64 = 1;

/// How this machine takes part in a networked game.
#[derive(Clone, Copy, Debug)]
pub enum Network {
    /// Hosts the game, listening for players on this address.
    Host(SocketAddr),
    /// Joins the game hosted at this address.
    Join(SocketAddr),
}

/// The session and everything about it that outlives a world.
#[derive(Resource)]
pub struct SessionService {
    pub session: session::Session,
    content: Content,
    network: Option<Network>,
    /// Simulation time: Bevy's time, minus pauses, times the rate.
    clock: Duration,
    pub paused: bool,
    pub rate: f32,
    /// The clock was held last frame (see `step`).
    holding: bool,
    /// The clock has run; it is never held again.
    started: bool,
    /// The connection, for the HUD.
    pub status: String,
    /// What other players see over this player's avatar.
    name: String,
    /// The world the name was last sent in.
    named: u32,
    /// Why a networked game ended for this player, if it did: the game
    /// returns to the menu, or without one closes shortly after (see
    /// `end_session`).
    pub ended: Option<String>,
    /// `Flow` has been told the session ended.
    left: bool,
    /// Where to write each world's tick-0 save and input log.
    record: Option<String>,
    start_save: Option<Vec<u8>>,
    /// A replay has no label of its own: it shows the scene it was launched in.
    replay_scene: Option<String>,
    /// A scene the host asked to move everyone to.
    request: Option<usize>,
    /// The world the World layer was started for.
    world_live: Option<u32>,
    /// The tick on which the newest command applies.
    applies_at: u64,
    /// This frame's ticks of `frame_world`, for the mirror.
    pub frame: Vec<(u64, Option<SimSettlement>)>,
    pub frame_world: u32,
    pub alpha: f32,
    /// A client's confirmed ticks on which stage 5 changed its avatar, for
    /// telling a knockback from a misprediction (see `prediction`).
    pub stage_five: crate::prediction::StageFive,
}

impl Service for SessionService {
    type Layer = SessionLayer;
    type Deps = (Flow,);

    fn init(world: &mut World) -> Self {
        let Opened { session, content, network, name, record, start_save, replay_scene } =
            world.resource_mut::<Flow>().take_opened().expect("a session starts once Flow has opened it");
        if network.is_some() {
            world.resource_mut::<crate::telemetry::Telemetry>().network.begin();
        }
        SessionService {
            session,
            content,
            network,
            clock: Duration::ZERO,
            paused: false,
            rate: 1.0,
            holding: false,
            started: false,
            status: String::new(),
            name,
            named: 0,
            ended: None,
            left: false,
            record,
            start_save,
            replay_scene,
            request: None,
            world_live: None,
            applies_at: 0,
            frame: Vec::new(),
            frame_world: 0,
            alpha: 0.0,
            stage_five: crate::prediction::StageFive::default(),
        }
    }

    fn shutdown(mut self, _world: &mut World) {
        self.write_recording();
        self.session.shutdown();
    }

    /// A client's session is ready once the host's save has loaded. A
    /// welcome names the world before any of its terrain is available.
    fn ready(&self, _world: &World) -> bool {
        self.world().is_some() || self.ended.is_some()
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Input, scene_keys)
            .add_service_systems::<Self, _>(Phase::Step, (load_requested, step).chain())
            .add_service_systems::<Self, _>(Phase::Mirror, watch_world)
            .add_service_systems::<Self, _>(Phase::Present, end_session);
    }
}

/// A session that has started (bound, connecting or replaying) and the
/// launch details the service keeps.
pub struct Opened {
    session: session::Session,
    content: Content,
    network: Option<Network>,
    name: String,
    record: Option<String>,
    start_save: Option<Vec<u8>>,
    replay_scene: Option<String>,
}

/// Starts a launch's session: builds the scene's world (or loads the
/// replay's), then hosts it, joins, or replays. `Flow` calls this; an error
/// says why the session couldn't start.
pub fn open(world: &mut World, launch: crate::flow::Launch) -> Result<Opened, String> {
    let content = Content::builtin();
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    // Single-player has nobody to wait for, so no input delay.
    let input_delay = if launch.network.is_some() { NetConfig::default().input_delay } else { 0 };
    // A joining client adapts its input margin to the network unless pinned.
    let input_margin = launch.input_margin_ms.map(content::millis_to_ticks);
    let config = NetConfig { threads, input_delay, input_margin, ..NetConfig::default() };
    let scene_id = world.resource::<SceneLibrary>().scenes[launch.scene].scene.id.clone();
    let mut start_save = None;
    let mut replay_scene = None;
    let mode = match (launch.replay, launch.network) {
        (Some((bytes, log)), _) => {
            let replayed = save::read(&bytes, &content).map_err(|e| format!("The recording's save doesn't load: {e}"))?;
            replay_scene = Some(scene_id);
            session::Mode::Replay { world: replayed, log }
        }
        (None, Some(Network::Join(host))) => session::Mode::Join { host, stable_id: stable_id(launch.profile.as_deref()) },
        (None, network) => {
            let built = build_world(world, launch.scene, &content);
            start_save = Some(save::write(&built, &content));
            let listen = match network {
                Some(Network::Host(listen)) => Some(listen),
                _ => None,
            };
            session::Mode::Host { world: built, label: scene_id, listen }
        }
    };
    let session = session::Session::start(mode, content.clone(), config).map_err(|e| format!("Couldn't start the session: {e}"))?;
    if let Some(addr) = session.local_addr() {
        info!("hosting on {addr}");
    }
    Ok(Opened { session, content, network: launch.network, name: launch.name, record: launch.record, start_save, replay_scene })
}

impl SessionService {
    /// The current world's tick-0 save, for replay tests.
    #[cfg(test)]
    pub fn start_save(&self) -> Option<&[u8]> {
        self.start_save.as_deref()
    }

    /// The session's content, for replay tests.
    #[cfg(test)]
    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn world(&self) -> Option<&SimWorld> {
        self.session.world()
    }

    pub fn settlement(&self) -> Option<&SimSettlement> {
        self.world().and_then(SimWorld::settlement)
    }

    pub fn networked(&self) -> bool {
        self.network.is_some()
    }

    pub fn is_client(&self) -> bool {
        matches!(self.network, Some(Network::Join(_)))
    }

    /// The scene the current world belongs to: the session's label, or for a
    /// replay the scene it was launched in.
    pub fn scene_label(&self) -> Option<&str> {
        self.session.world_label().or(self.replay_scene.as_deref())
    }

    /// Asks to move everyone to another scene. Only a host can; it takes
    /// effect in the next Step.
    pub fn request_scene(&mut self, index: usize) {
        if self.session.is_host() {
            self.request = Some(index);
        }
    }

    /// The local player's avatar index. Replays watch the host's.
    pub fn player(&self) -> Option<u16> {
        let me = self.session.player().unwrap_or(net::HOST_PLAYER);
        self.settlement()?.avatar_of(me)
    }

    /// The local player's character definition.
    pub fn player_character(&self) -> Option<&content::character::CharacterDef> {
        self.settlement()?.character_of(self.player()? as usize)
    }

    /// The radius the local player's routes and reach checks use.
    pub fn player_radius(&self) -> Option<Fx> {
        Some(self.player_character()?.body.radius)
    }

    /// The local player's avatar ID, as views name agents.
    pub fn player_id(&self) -> Option<String> {
        Some(self.settlement()?.agents[self.player()? as usize].id.clone())
    }

    /// The confirmed player position in view coordinates, for gameplay input.
    pub fn player_view_position(&self) -> Option<Vec3> {
        Some(self.to_view(self.settlement()?.agents[self.player()? as usize].body.position))
    }

    /// Sends a command; it applies on the next host tick locally, or after
    /// the input delay when joining another host.
    pub fn command(&mut self, kind: CommandKind) {
        let tick = self.world().map_or(0, |w| w.tick());
        self.applies_at = tick + self.session.input_delay();
        self.session.queue(kind);
    }

    /// Puts a weapon in the avatar's hand, for every peer to see; the local
    /// view switches right away.
    pub fn equip(&mut self, weapon: content::scenario::Weapon) {
        let Some(agent) = self.player() else { return };
        self.command(CommandKind::Equip { agent, weapon });
    }

    /// Digs out a layer under the avatar, where the scene allows it.
    pub fn dig(&mut self) {
        let Some(agent) = self.player() else { return };
        if self.world().is_some_and(|w| w.terrain().diggable) {
            self.command(CommandKind::DigDown { agent });
        }
    }

    /// Whether every command sent so far has been applied.
    pub fn caught_up(&self) -> bool {
        self.world().is_some_and(|w| w.tick() > self.applies_at) && self.session.pending_inputs() == 0
    }

    /// The arena center in world tiles; views work in arena coordinates.
    pub fn center(&self) -> FxVec2 {
        self.world().map_or(FxVec2::ZERO, |w| w.terrain().center)
    }

    /// Arena coordinates (meters from the center, Bevy x/z) to world tiles.
    pub fn to_sim(&self, p: Vec3) -> FxVec2 {
        to_sim(self.center(), p)
    }

    /// World tiles to arena coordinates on the ground.
    pub fn to_view(&self, p: FxVec2) -> Vec3 {
        to_view(self.center(), p)
    }

    /// Ground at a rendered position, including generated terraces and dug pits.
    /// Character posing samples this; physics continues to use fixed-point terrain.
    pub fn ground_height(&self, p: Vec3) -> Option<f32> {
        self.world()?.terrain().ground(self.to_sim(p)).map(|height| height.to_f32())
    }

    /// Writes the current world's recording, if asked to: `<path>.gsav` is
    /// its tick-0 save and `<path>` its input log. Later worlds in the same
    /// session add their number: `<path>.2`, `<path>.2.gsav`, and so on.
    fn write_recording(&self) {
        let (Some(path), Some(save)) = (&self.record, &self.start_save) else { return };
        let Some(log) = self.session.input_log() else { return };
        let path = match self.session.world_number() {
            1 => path.clone(),
            n => format!("{path}.{n}"),
        };
        let written = std::fs::write(format!("{path}.gsav"), save).and_then(|()| std::fs::write(&path, log.to_bytes()));
        if let Err(e) = written {
            error!("could not write the recording {path}: {e}");
        }
    }
}

fn to_sim(center: FxVec2, p: Vec3) -> FxVec2 {
    FxVec2::new(center.x + Fx::quantize(p.x), center.y + Fx::quantize(p.z))
}

fn to_view(center: FxVec2, p: FxVec2) -> Vec3 {
    Vec3::new((p.x - center.x).to_f32(), 0.0, (p.y - center.y).to_f32())
}

/// Builds a scene's world from its data. Loads the scene's models first:
/// boulder radii come from them.
fn build_world(world: &mut World, index: usize, content: &Content) -> SimWorld {
    world.resource_scope(|world, library: Mut<SceneLibrary>| {
        world.resource_scope(|world, mut cache: Mut<crate::assets::ModelCache>| {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            cache.load(&library.scenes[index], &mut meshes);
        });
        let cache = world.resource::<crate::assets::ModelCache>();
        let setup = scene_setup(&library, index, cache);
        let id = &library.scenes[index].scene.id;
        SimWorld::new_scene(SEED, content, &setup).unwrap_or_else(|e| panic!("scene {id} does not build a world: {e}"))
    })
}

fn scene_setup(library: &SceneLibrary, index: usize, cache: &crate::assets::ModelCache) -> SceneSetup {
    let disc = |d: &crate::navigation::Disc| sim::terrain::Disc {
        center: FxVec2::new(Fx::quantize(d.center.x), Fx::quantize(d.center.z)),
        radius: Fx::quantize(d.radius),
    };
    let pack = &library.scenes[index];
    let scene = &pack.scene;
    let found = crate::obstacles::scene_obstacles(pack, |instance| cache.collision_radius(pack, instance));
    let q = Fx::quantize;
    let surface = |min: [f32; 2], max: [f32; 2], height: f32| sim::terrain::Surface {
        min: FxVec2::new(q(min[0]), q(min[1])),
        max: FxVec2::new(q(max[0]), q(max[1])),
        height: q(height),
    };
    let (surfaces, platforms_only) = match &scene.terrain {
        Some(t) => (t.platforms.iter().map(|p| surface(p.min, p.max, p.height)).collect(), true),
        None => (pack.environment.iter().flat_map(|e| &e.walkable).map(|w| surface(w.min, w.max, w.height)).collect(), false),
    };
    let material = |m: voxel_assets::Mineral| match m {
        voxel_assets::Mineral::AmberOre => 0u8,
        voxel_assets::Mineral::BlueCrystal => 1,
        voxel_assets::Mineral::GreenMalachite => 2,
    };
    SceneSetup {
        scenario: scene.scenario.clone(),
        player: scene.player.clone(),
        extent: q(crate::assets::extent(pack)),
        obstacles: found.discs.iter().map(disc).collect(),
        destructibles: pack.destructibles.iter().map(|piece| sim::terrain::Destructible {
            asset: piece.asset,
            disc: sim::terrain::Disc {
                center: FxVec2::new(q(piece.center[0]), q(piece.center[2])),
                radius: q(piece.radius),
            },
            health: piece.health,
            max_health: piece.health,
            repairable: piece.repairable,
            repair_progress: 0,
        }).collect(),
        pianos: scene.pianos.iter().map(|piano| FxVec2::new(q(piano.position[0]), q(piano.position[2]))).collect(),
        doors: found.doors.iter().map(disc).collect(),
        surfaces,
        platforms_only,
        spawn: scene.spawn.map(q),
        piles: scene.piles.as_ref().map(|p| PileSetup {
            capacity: p.capacity,
            dispensers: p.dispensers.iter().map(|d| (material(d.material), [q(d.output[0]), q(d.output[2])])).collect(),
            piles: p.piles.iter().map(|s| (material(s.material), [q(s.position[0]), q(s.position[2])], s.units)).collect(),
            collector: p.collector.map(|c| [q(c[0]), q(c[2])]),
        }),
        memory_trial: scene.features.contains(&voxel_assets::SceneFeature::MemoryTrial),
        diggable: scene.features.contains(&voxel_assets::SceneFeature::Digging),
        mine: scene.mine.as_ref().map(|m| m.seed),
    }
}

/// The digit keys and F4 ask to move everyone to a scene (a host's request; see
/// `SessionService::request_scene`).
fn scene_keys(input: Res<debug_ui::GameInput>, library: Res<SceneLibrary>, mut service: ResMut<SessionService>) {
    let keys = input.keys();
    const KEYS: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    let current = service.scene_label().and_then(|label| library.index(label).ok());
    // F4 steps to the next scene.
    let next = keys.just_pressed(KeyCode::F4).then(|| current.map_or(0, |c| (c + 1) % library.scenes.len()));
    let Some(index) = KEYS.iter().position(|&key| keys.just_pressed(key)).or(next) else { return };
    if current != Some(index) && index < library.scenes.len() {
        service.request_scene(index);
    }
}

/// Loads the scene the host asked for into the session: a new world for
/// everyone. Each world gets its own recording.
fn load_requested(world: &mut World) {
    let Some(index) = world.resource_mut::<SessionService>().request.take() else { return };
    let content = world.resource::<SessionService>().content.clone();
    let built = build_world(world, index, &content);
    let label = world.resource::<SceneLibrary>().scenes[index].scene.id.clone();
    let mut service = world.resource_mut::<SessionService>();
    service.write_recording();
    service.start_save = Some(save::write(&built, &content));
    let clock = service.clock;
    service.session.load_world(built, label, clock);
}

/// Runs every tick due by this frame's time, keeping them for the mirror.
fn step(
    time: Res<Time>,
    hub: Option<Res<crate::control::Hub>>,
    mut service: ResMut<SessionService>,
    mut telemetry: ResMut<crate::telemetry::Telemetry>,
) {
    service.frame.clear();
    // Scripted runs warm up for different lengths with and without rendering,
    // and start once the first world is up; holding the clock until then
    // lines up ticks with script frames, so a rendered replay of a headless
    // recording matches it command for command. A client can't hold past the
    // warmup: it has to connect to get a world at all.
    let client = service.is_client();
    let hold = !service.started && hub.is_some_and(|hub| hub.in_warmup() || (!client && hub.waiting_for_world()));
    if hold {
        service.holding = true;
        return;
    }
    if std::mem::take(&mut service.holding) {
        // The first tick runs on the first frame after warmup, as it does
        // without one (Bevy's first frame has no delta).
        service.clock = Duration::ZERO;
    } else if service.networked() {
        // Everyone shares one clock: pausing or speeding up alone would
        // only fall behind or run out of turns.
        service.clock += time.delta();
    } else if !service.paused {
        let rate = service.rate;
        service.clock += time.delta().mul_f32(rate);
    }
    service.started = true;
    let clock = service.clock;
    let service = &mut *service;
    // A client's own commands, to tell which ones its confirmed ticks applied.
    let pending: Vec<_> = service.session.pending_commands().map(|p| (p.id, p.kind.clone())).collect();
    let mut ticks = Vec::new();
    let started = web_time::Instant::now();
    service.session.update(clock, |world| {
        let copying = web_time::Instant::now();
        let settlement = world.settlement().cloned();
        ticks.push((world.tick(), settlement, world.stage_times(), copying.elapsed(), world.terrain().revision));
    });
    let update_time = started.elapsed();
    telemetry.session_frame(update_time, ticks.iter().map(|(_, _, times, copy, _)| (*times, *copy)));
    if service.is_client() {
        observe_stage_five(service, ticks.iter().map(|(next, settlement, _, _, revision)| (*next, *revision, settlement.as_ref())), &pending);
    }
    if service.networked() {
        telemetry.network_frame(crate::telemetry::NetworkFrame {
            at: web_time::Instant::now(),
            role: if service.is_client() { "Client" } else { "Host" },
            status: service.session.status(),
            world: service.session.world_number(),
            tick: service.world().map(|world| world.tick()),
            pending: service.session.pending_inputs(),
            stats: service.session.stats(),
            diagnostics: service.session.diagnostics(clock),
            update_ms: update_time.as_secs_f64() * 1000.,
            tick_ms: ticks.iter().map(|(_, _, times, _, _)| times.total().as_secs_f64() * 1000.).max_by(f64::total_cmp),
            frame_ms: None,
        });
    }
    service.frame = ticks.into_iter().map(|(tick, settlement, _, _, _)| (tick, settlement)).collect();
    service.frame_world = service.session.world_number();
    service.alpha = service.session.alpha(clock);
    let client = service.is_client();
    for event in service.session.events.drain(..) {
        // Without its host a client's game is over: there's no migrating
        // the host (yet), so it doesn't wait in a world that can't move.
        let ended = match &event {
            net::NetEvent::HostLost => Some("The host left the game.".to_string()),
            net::NetEvent::Rejected { reason } => Some(format!("The host turned us away: {reason}.")),
            net::NetEvent::Desync { tick, .. } if client => Some(format!("Out of sync with the host at tick {tick}.")),
            _ => None,
        };
        match ended {
            Some(reason) => {
                warn!("{event:?}");
                service.ended.get_or_insert(reason);
            }
            None => info!("{event:?}"),
        }
    }
    if service.networked() {
        service.status = service.session.status();
    }
    // Introduce ourselves once our avatar exists in this world (a client's
    // appears after it joins).
    let number = service.session.world_number();
    if service.networked() && service.named != number && service.player().is_some() {
        let name = service.name.clone();
        service.command(CommandKind::Name { name });
        service.named = number;
    }
}

/// Checks a client's confirmed ticks this frame (each settlement with the
/// tick it runs next and the terrain revision then) for stage 5 changing
/// its avatar (`prediction::StageFive`). `pending` are its commands that
/// hadn't applied before the frame's ticks ran; the client's record of
/// which applied on which tick says which a tick ran.
fn observe_stage_five<'s>(
    service: &mut SessionService,
    ticks: impl Iterator<Item = (u64, u64, Option<&'s SimSettlement>)>,
    pending: &[(net::CommandId, CommandKind)],
) {
    let Some(agent) = service.player() else { return };
    let SessionService { session, stage_five, .. } = service;
    let Some(world) = session.world() else { return };
    let applied: Vec<(u64, net::CommandId)> = session.applied_commands().collect();
    let applied_on = |tick: u64| {
        applied
            .iter()
            .filter(|(at, _)| *at == tick)
            .map(|(_, id)| pending.iter().find(|(pending, _)| pending == id).map(|(_, kind)| kind))
            .collect::<Option<Vec<_>>>()
    };
    let ticks = ticks.filter_map(|(next, revision, settlement)| Some((next, revision, settlement?)));
    stage_five.observe(session.world_number(), agent, ticks, world.terrain(), applied_on);
}

/// Moves the World layer with the session's world: off while a world is
/// replaced (for a frame, so its services stop and its entities go), loading
/// once a new one is in the slot. Its services decide when it is active.
fn watch_world(mut service: ResMut<SessionService>, state: Option<Res<State<WorldState>>>, next: Option<ResMut<NextState<WorldState>>>) {
    let (Some(state), Some(mut next)) = (state, next) else { return };
    let number = service.session.world_number();
    match state.get() {
        WorldState::Off if number > 0 && service.world().is_some() && service.ended.is_none() => {
            service.world_live = Some(number);
            next.set(WorldState::Loading);
        }
        WorldState::Loading | WorldState::Active if service.world_live != Some(number) => next.set(WorldState::Off),
        _ => {}
    }
}

/// How long the ending banner shows before the game closes.
const CLOSING_SECONDS: f32 = 4.0;

#[derive(Component)]
pub struct EndBanner;

/// A networked game ended for this player: says why, counts down, and closes.
fn end_session(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut service: ResMut<SessionService>,
    mut flow: ResMut<Flow>,
    options: Res<crate::control::Options>,
    mut banner: Query<&mut Text, With<EndBanner>>,
    mut exit: MessageWriter<AppExit>,
    mut left: Local<Option<f32>>,
) {
    let Some(reason) = service.ended.clone() else { return };
    if options.menus() {
        if !service.left {
            service.left = true;
            flow.leave(Some(reason));
        }
        return;
    }
    let remaining = left.get_or_insert(CLOSING_SECONDS);
    *remaining -= time.delta_secs();
    if *remaining <= 0.0 {
        exit.write(AppExit::Success);
        return;
    }
    let text = format!("{}
Closing in {}...", reason.to_uppercase(), remaining.ceil());
    match banner.single_mut() {
        Ok(mut current) => {
            if current.0 != text {
                current.0 = text;
            }
        }
        Err(_) => {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100.),
                        height: percent(100.),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                    GlobalZIndex(100),
                ))
                .with_child((
                    EndBanner,
                    Text::new(text),
                    TextFont { font_size: FontSize::Px(26.), ..default() },
                    TextColor(Color::srgb(1.0, 0.55, 0.3)),
                    TextLayout::justify(Justify::Center),
                ));
        }
    }
}

/// Tells this player apart from others when joining, so a player who comes
/// back gets their old slot. Kept in a profile file (one per `--profile`
/// name) where menu settings live; a fresh random ID if it can't be read
/// or written, or on the web.
fn stable_id(profile: Option<&str>) -> net::StableId {
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let fresh = nanos ^ (u64::from(std::process::id()) << 32);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = profile;
        fresh
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let name = match profile {
            Some(name) => format!("kestrel-profile-{}.id", name.chars().filter(char::is_ascii_alphanumeric).collect::<String>()),
            None => "kestrel-profile.id".to_string(),
        };
        let path = std::env::temp_dir().join(name);
        if let Some(id) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()) {
            return id;
        }
        if let Err(e) = std::fs::write(&path, fresh.to_string()) {
            warn!("Cannot save the player ID to {}: {e}", path.display());
        }
        fresh
    }
}

/// One world's last two ticks, for the views to interpolate.
#[derive(Resource)]
pub struct WorldMirror {
    /// The session's world these ticks come from.
    world_number: u32,
    /// The world's scene (an index into the `SceneLibrary`), from the
    /// session's label; `usize::MAX` if the label names no known scene.
    pub scene: usize,
    /// The last two ticks' settlements, for interpolation.
    pub previous: Option<SimSettlement>,
    pub current: Option<SimSettlement>,
    /// Every event since the last frame.
    pub events: Vec<sim::settlement::Event>,
    pub tick: u64,
    pub alpha: f32,
    /// The arena center in world tiles; views work in arena coordinates.
    pub center: FxVec2,
    /// The local player (replays watch the host).
    me: sim::PlayerId,
}

impl Service for WorldMirror {
    type Layer = WorldLayer;
    type Deps = (SessionService,);

    fn init(world: &mut World) -> Self {
        let service = world.resource::<SessionService>();
        let label = service.scene_label().map(str::to_string);
        let mirror = WorldMirror {
            world_number: service.session.world_number(),
            scene: usize::MAX,
            previous: None,
            current: service.settlement().cloned(),
            events: Vec::new(),
            tick: service.world().map_or(0, |w| w.tick().saturating_sub(1)),
            alpha: 0.0,
            center: service.center(),
            me: service.session.player().unwrap_or(net::HOST_PLAYER),
        };
        // The views show the world's scene, on the host and on clients.
        match label.as_deref().map(|label| world.resource::<SceneLibrary>().index(label)) {
            Some(Ok(index)) => WorldMirror { scene: index, ..mirror },
            Some(Err(e)) => {
                warn!("the session's world: {e}");
                mirror
            }
            None => mirror,
        }
    }

    /// Ready once the world is in the slot and every peer has it.
    fn ready(&self, world: &World) -> bool {
        let session = &world.resource::<SessionService>().session;
        session.world().is_some() && !session.world_loading()
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Mirror, mirror);
    }
}

impl WorldMirror {
    /// Arena coordinates (meters from the center, Bevy x/z) to world tiles.
    pub fn to_sim(&self, p: Vec3) -> FxVec2 {
        to_sim(self.center, p)
    }

    /// World tiles to arena coordinates on the ground.
    pub fn to_view(&self, p: FxVec2) -> Vec3 {
        to_view(self.center, p)
    }

    /// The local player's avatar index in these ticks. Views use this one:
    /// for a frame after a world is replaced, the session's world is already
    /// the new one.
    pub fn player(&self) -> Option<u16> {
        self.current.as_ref()?.avatar_of(self.me)
    }

    /// The local player's avatar ID, as views name agents.
    pub fn player_id(&self) -> Option<String> {
        Some(self.current.as_ref()?.agents[self.player()? as usize].id.clone())
    }

    /// A hash of the current state, to tell runs apart in traces.
    pub fn fingerprint(&self) -> String {
        self.current.as_ref().map_or_else(String::new, |s| format!("{:016x}", sim::hash::hash_of(s)))
    }
}

/// Takes this frame's ticks, keeping the last two for interpolation. Ticks of
/// a newer world are left alone: this world is about to stop.
fn mirror(service: Res<SessionService>, mut mirror: ResMut<WorldMirror>) {
    if service.frame_world != mirror.world_number {
        return;
    }
    mirror.events.clear();
    for (tick, settlement) in &service.frame {
        if let Some(s) = settlement {
            mirror.events.extend(s.events.iter().cloned());
        }
        mirror.previous = std::mem::replace(&mut mirror.current, settlement.clone());
        mirror.tick = tick.saturating_sub(1);
    }
    mirror.alpha = service.alpha;
    // A client learns where the arena is, and who it is, once the host's
    // save loads.
    mirror.center = service.center();
    mirror.me = service.session.player().unwrap_or(net::HOST_PLAYER);
}

/// Turns the local player's input into commands.
#[derive(Resource, Default)]
pub struct PlayerControl {
    /// The last movement intent sent, so held keys don't resend it every frame.
    walking: Option<(FxVec2, bool)>,
    last_aim: Option<FxVec2>,
}

impl Service for PlayerControl {
    type Layer = WorldLayer;
    type Deps = (SessionService, WorldMirror, SceneView);

    fn init(_world: &mut World) -> Self {
        PlayerControl::default()
    }

    /// Actions from every device go through the shared dispatcher
    /// (`input::gameplay`); mouse aiming runs between its two parts.
    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(
            Phase::Input,
            (
                crate::input::gameplay::prepare,
                attack_input.run_if(crate::control::interactive).run_if(pointer_allowed),
                crate::input::gameplay::dispatch,
                crate::preview::playback.run_if(crate::control::interactive),
            )
                .chain()
                .run_if(in_state(WorldState::Active)),
        );
    }
}

/// Mouse targeting runs while keyboard/mouse owns gameplay and no UI
/// context has it.
pub fn pointer_allowed(input: Option<Res<crate::input::InputService>>) -> bool {
    // Conditions are checked even once services have stopped (on exit).
    input.is_some_and(|input| crate::input::gameplay::pointer_allowed(&input))
}

impl PlayerControl {
    /// Whether the last movement sent was a directional walk.
    pub fn walking_direction(&self) -> bool {
        self.walking.is_some_and(|(d, _)| d != FxVec2::ZERO)
    }

    /// Forgets the cached movement after direct control was released, so
    /// the next deliberate movement is always sent.
    pub fn forget_direct_input(&mut self) {
        self.walking = None;
    }

    pub fn aim(&mut self, service: &mut SessionService, direction: Vec3) {
        let Some(agent) = service.player() else { return };
        let aim = FxVec2::new(Fx::quantize(direction.x), Fx::quantize(direction.z)).normalize_or_zero();
        if aim == FxVec2::ZERO || self.last_aim.is_some_and(|previous| (previous - aim).length() < Fx::from_ratio(1, 50)) {
            return;
        }
        self.last_aim = Some(aim);
        service.command(CommandKind::AimAvatar { agent, aim });
    }

    /// Asks the avatar to walk in a direction (camera-relative, already
    /// rotated into arena space); a zero direction stops it.
    pub fn walk(&mut self, service: &mut SessionService, direction: Vec2, run: bool) {
        let Some(agent) = service.player() else { return };
        let direction = FxVec2::new(Fx::quantize(direction.x), Fx::quantize(direction.y)).normalize_or_zero();
        if self.walking == Some((direction, run)) {
            return;
        }
        self.walking = Some((direction, run));
        let intent = if direction == FxVec2::ZERO { Intent::Stop } else { Intent::Walk { direction, run } };
        service.command(CommandKind::MoveAvatar { agent, intent });
    }

    /// Asks the avatar to walk to a point (arena coordinates).
    pub fn go_to(&mut self, service: &mut SessionService, target: Vec3, run: bool) {
        let Some(agent) = service.player() else { return };
        self.walking = None;
        let target = service.to_sim(target);
        service.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run } });
    }

    /// Starts an attack: the swing animates now, and the simulation resolves
    /// it on its windup tick.
    pub fn strike(&mut self, service: &mut SessionService, controls: &mut Controls, scene: &voxel_assets::ScenePack, aim: Vec3) {
        let Some(agent) = service.player() else { return };
        let weapon = controls.weapon;
        let muzzle = scene
            .scene
            .style
            .as_ref()
            .and_then(|style| style.weapon_skins.iter().find(|skin| skin.weapon == weapon).map(|skin| skin.muzzle.map(|v| v * style.actor_scale)))
            .unwrap_or([0.0, 0.6, 0.25]);
        let flat = crate::navigation::flat(aim).normalize_or(Vec3::Z);
        if weapon != content::scenario::Weapon::Laser {
            self.walking = None;
        }
        // The tick the command asks for: a client's input horizon, or on a
        // host the next tick.
        let asked = service.session.input_horizon().or_else(|| service.world().map(sim::World::tick));
        service.command(CommandKind::Attack {
            agent,
            weapon,
            aim: FxVec2::new(Fx::quantize(flat.x), Fx::quantize(flat.z)),
            muzzle: muzzle.map(Fx::quantize),
        });
        controls.strike_at(aim, asked);
    }
}

#[derive(Default)]
pub struct VisualGroundStep {
    height: Option<f32>,
    agent: Option<u16>,
    scene: Option<usize>,
}

/// Places the player where the simulation has the avatar, between ticks. A
/// joined player's own avatar is predicted ahead of the confirmed world (see
/// `prediction`).
pub fn follow_avatar(
    mirror: Res<WorldMirror>,
    service: Res<SessionService>,
    scene: Res<SceneView>,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    sound: Res<crate::sound::Sound>,
    mut prediction: Local<crate::prediction::AvatarPrediction>,
    mut ground_step: Local<VisualGroundStep>,
    mut telemetry: ResMut<crate::telemetry::Telemetry>,
    mut ledger: ResMut<crate::event_prediction::EventPrediction>,
    mut pending_lasers: ResMut<crate::effects::PendingLasers>,
    mut pending_arcs: ResMut<crate::effects::PendingArcs>,
    mut predicted_arrows: ResMut<crate::settlement::PredictedArrows>,
    mut drawn_avatar: ResMut<crate::prediction::DrawnAvatar>,
    withdraw: Option<Res<crate::prediction::WithdrawPrediction>>,
) {
    let (Some(current), Some(index)) = (&mirror.current, mirror.player()) else {
        drawn_avatar.0 = None;
        return;
    };
    let a = &current.agents[index as usize];
    let before = mirror.previous.as_ref().and_then(|p| current.same_agent_in(index, p)).unwrap_or(a);
    let withdrawn = withdraw.is_some_and(|until| service.world().is_some_and(|world| world.tick() <= until.0));
    let predicted = if service.is_client() && !withdrawn { predict(&mut prediction, &mirror, &service, before, time.delta_secs()) } else { None };
    // What the avatar's lasting effects are drawn from (D5): the newest
    // confirmed state at render time, unless it is predicted.
    drawn_avatar.0 = Some(match &predicted {
        Some(drawn) => crate::prediction::DrawnState { agent: index, from: drawn.from.clone(), to: drawn.to.clone(), time: drawn.time },
        None => crate::prediction::DrawnState { agent: index, from: before.clone(), to: a.clone(), time: mirror.tick as f32 - 1.0 + mirror.alpha },
    });
    if predicted.is_none() {
        prediction.reset();
    }
    if service.is_client() {
        let now = time.elapsed_secs_f64();
        predict_events(&mut ledger, &mirror, &service, index, predicted.as_ref(), now, &mut pending_lasers, &mut pending_arcs, &mut predicted_arrows, &sound, &mut telemetry);
    } else {
        ledger.reset();
        predicted_arrows.0.retain(|arrow| arrow.projectile.is_some());
    }
    let (from, to, alpha) = match &predicted {
        Some(drawn) => (&drawn.from, &drawn.to, drawn.fraction),
        None => {
            // The local single-player avatar can use the newest confirmed pose.
            // Interpolating from the previous tick adds a full tick of visible lag.
            let alpha = if !service.is_client() && !service.session.is_replay() { 1.0 } else { mirror.alpha };
            // A teleport (respawn, deploying a recording) snaps instead of sliding.
            let alpha = if mirror.to_view(before.body.position).distance(mirror.to_view(a.body.position)) > 1.5 { 1.0 } else { alpha };
            (before, a, alpha)
        }
    };
    let (Some(from_avatar), Some(to_avatar)) = (from.avatar(), to.avatar()) else { return };
    let (start, end) = (mirror.to_view(from.body.position), mirror.to_view(to.body.position));
    let mut position = start.lerp(end, alpha);
    if let Some(drawn) = &predicted {
        position.x = drawn.position.x;
        position.z = drawn.position.z;
    }
    position.y = from_avatar.height.to_f32() + (to_avatar.height - from_avatar.height).to_f32() * alpha;
    let reset_step = ground_step.agent != Some(index)
        || ground_step.scene != Some(scene.index)
        || start.distance(end) > 1.5
        || predicted.as_ref().is_some_and(|drawn| drawn.snapped || drawn.knockback_snapped);
    position.y = crate::character::visual_step_height(
        if reset_step { None } else { ground_step.height },
        position.y,
        time.delta_secs(),
        !to_avatar.flight.airborne && matches!(to_avatar.jump, sim::settlement::avatar::JumpPhase::Grounded),
    );
    ground_step.height = Some(position.y);
    ground_step.agent = Some(index);
    ground_step.scene = Some(scene.index);
    if let Some(drawn) = &predicted {
        telemetry.network.preview(
            web_time::Instant::now(),
            crate::telemetry::PreviewUpdate {
                shown: position,
                confirmed: mirror.to_view(a.body.position),
                target: Vec3::new(drawn.target.x, position.y, drawn.target.z),
                mispredicted: drawn.mispredicted,
                snapped: drawn.snapped,
                knockback: drawn.knockback,
                knockback_snapped: drawn.knockback_snapped,
            },
        );
    }
    let motion = crate::character::motion_profile(&scene.pack);
    let previous = player.translation;
    let teleport = previous.distance(position) > 1.5;
    player.translation = position;
    let heading = from.body.heading.to_f32() + angle_between(from.body.heading.to_f32(), to.body.heading.to_f32()) * alpha;
    player.rotation = Quat::from_rotation_y(heading);
    let velocity = |s: &sim::settlement::AgentState| Vec3::new(s.body.velocity.x.to_f32(), 0.0, s.body.velocity.y.to_f32());
    controls.velocity = velocity(a);
    if predicted.is_some() {
        // A predicted avatar faces the way it is predicted to move.
        let predicted_velocity = velocity(from).lerp(velocity(to), alpha);
        if predicted_velocity.length_squared() > 0.01 {
            player.rotation = Quat::from_rotation_y(predicted_velocity.x.atan2(predicted_velocity.z));
        }
        controls.velocity = predicted_velocity;
    }
    let moved = if teleport { Vec3::ZERO } else { crate::navigation::flat(position - previous) };
    let relative_speed = controls.velocity.length() / motion.walk_speed.max(0.1);
    let step_distance = crate::character::step_distance_for_speed(motion.step_distance, relative_speed).clamp(0.2, 4.0);
    let contacts = controls.gait.advance(moved.length(), step_distance, !to_avatar.flight.airborne);
    for _ in 0..contacts {
        sound.play(crate::sound::Cue::Step);
    }
}

/// A joined player's avatar this frame, replayed from the session's
/// confirmed world (which the pending commands match). `None` while there is
/// nothing to predict from, or for a frame while the world is replaced.
fn predict(
    prediction: &mut crate::prediction::AvatarPrediction,
    mirror: &WorldMirror,
    service: &SessionService,
    previous: &sim::settlement::AgentState,
    dt: f32,
) -> Option<crate::prediction::Drawn> {
    let world = service.world()?;
    let number = service.session.world_number();
    if number != mirror.world_number {
        return None;
    }
    let pending: Vec<_> = service.session.pending_commands().map(|p| (p.tick, p.kind)).collect();
    let agent = service.player()?;
    let confirmed = crate::prediction::Confirmed {
        world: number,
        agent,
        settlement: world.settlement()?,
        terrain: world.terrain(),
        tick: world.tick(),
        previous,
        alpha: mirror.alpha,
        pending: &pending,
        horizon: service.session.input_horizon()?,
        to_view: &|p| mirror.to_view(p),
        touched: service.stage_five.touched(number, agent),
    };
    Some(prediction.update(&confirmed, dt))
}

/// A joined player's own predicted events (see `event_prediction`): this
/// frame's confirmed ones are matched to those already presented, then the
/// events the drawn avatar shows are presented: a laser shot drawn, with its
/// sound, from where the avatar is drawn, an arc coil shot's arc to its
/// first target, with its sound, and a bow shot's arrow, in flight from the
/// bow with its sound, until its confirmed projectile takes it over
/// (`settlement::PredictedArrows`). A shot the confirmed world never fires
/// withdraws its arrow. Other players' events, and kinds not presented
/// early, are presented as they are confirmed.
#[allow(clippy::too_many_arguments)]
fn predict_events(
    ledger: &mut crate::event_prediction::EventPrediction,
    mirror: &WorldMirror,
    service: &SessionService,
    agent: u16,
    drawn: Option<&crate::prediction::Drawn>,
    now: f64,
    pending: &mut crate::effects::PendingLasers,
    arcs: &mut crate::effects::PendingArcs,
    arrows: &mut crate::settlement::PredictedArrows,
    sound: &crate::sound::Sound,
    telemetry: &mut crate::telemetry::Telemetry,
) {
    use crate::event_prediction::{end_of, Outcome};
    let Some(world) = service.world().filter(|_| service.session.world_number() == mirror.world_number) else {
        ledger.reset();
        arrows.0.retain(|arrow| arrow.projectile.is_some());
        return;
    };
    ledger.begin(mirror.world_number, agent);
    // Which command applied on which tick: the drain record, then the
    // pending commands where the replay applies them.
    let tick = world.tick();
    let applied: Vec<_> = service
        .session
        .applied_commands()
        .chain(service.session.pending_commands().map(|p| (p.tick.max(tick), p.id)))
        .collect();
    // Where a beam ending at `p` is drawn ending.
    let end = |p: FxVec2| {
        let at = mirror.to_view(p);
        Vec3::new(at.x, service.ground_height(at).unwrap_or(0.0) + 0.8, at.z)
    };
    // An event without an end is marked where the avatar is drawn.
    let avatar = drawn.map(|d| d.position).or_else(|| service.player_view_position()).unwrap_or(Vec3::ZERO);
    let at = |event: &sim::settlement::Event| end_of(event).map_or(avatar, end);
    let mut outcomes: Vec<Outcome> = mirror.events.iter().filter_map(|event| ledger.confirm(event, &applied, now)).collect();
    outcomes.extend(ledger.expire(tick));
    let mut new = Vec::new();
    if let Some(drawn) = drawn {
        let predicted = ledger.predict(&drawn.events, &applied, now);
        outcomes.extend(predicted.dropped);
        new = predicted.new;
    }
    let instant = web_time::Instant::now();
    for outcome in outcomes {
        if outcome.cancel {
            stop_presented(outcome.kind, outcome.id, pending);
        }
        if outcome.kind == "bow" && outcome.confirmed.is_none() {
            // Never fired: its arrow is gone. One confirmed, even
            // differently, is taken over by its projectile.
            arrows.withdraw(outcome.id);
        }
        telemetry.network.event_outcome(instant, crate::telemetry::EventOutcome {
            kind: outcome.kind,
            predicted: at(&outcome.predicted),
            confirmed: outcome.confirmed.as_ref().map(at),
            lead: outcome.lead,
            exact: outcome.exact,
        });
    }
    for presented in new {
        match presented.event {
            sim::settlement::Event::LaserFired { to, hit_actor, cover, .. } => {
                pending.0.push(crate::effects::LaserShot {
                    agent,
                    target: end(to),
                    hit_actor,
                    cover,
                    seed: crate::effects::laser_impact_seed(presented.tick, agent),
                    predicted: Some(presented.id),
                });
                sound.play(crate::sound::Cue::Laser);
            }
            sim::settlement::Event::ArcFired { from, to, .. } => {
                // Its arc hits are stage 5: they chain on from the confirmed
                // shot (`settlement::ArcChain`).
                let at = |p: FxVec2| mirror.to_view(p).with_y(crate::settlement::ARC_HEIGHT);
                arcs.0.push((at(from), at(to), presented.tick as u32));
                sound.play(crate::sound::Cue::Arc);
            }
            sim::settlement::Event::BowFired { from, to, height, flight, .. } => {
                let at = |p: FxVec2| {
                    let at = mirror.to_view(p);
                    [at.x, at.z]
                };
                arrows.0.push(crate::settlement::PredictedArrow {
                    id: presented.id,
                    fired: presented.tick,
                    from: at(from),
                    to: at(to),
                    height: height.to_f32(),
                    flight,
                    projectile: None,
                });
                sound.play(crate::sound::Cue::Bow);
            }
            _ => continue,
        }
        telemetry.network.event_presented(presented.kind);
    }
}

/// Stops what a presented event of `kind` started, when its kind's miss
/// policy is `Cancel` and it missed. Events start only one-shots so far
/// (D5 in `ABILITY_PREDICTION_HANDOFF.md`): a laser shot not drawn yet is
/// dropped, and anything else has nothing left to stop. A kind that starts
/// something lasting stops it here.
fn stop_presented(kind: &str, id: u64, pending: &mut crate::effects::PendingLasers) {
    if kind == "laser" {
        pending.0.retain(|shot| shot.predicted != Some(id));
    }
}

/// The content every session plays with (see `open`), parsed once, for views
/// that need definitions outside a session, such as Character Studio.
pub fn builtin_content() -> &'static Content {
    static CONTENT: std::sync::LazyLock<Content> = std::sync::LazyLock::new(Content::builtin);
    &CONTENT
}

/// A character from the builtin content the sim also uses.
pub fn builtin_character(name: &str) -> &'static content::character::CharacterDef {
    let content = builtin_content();
    let ix = content.names.character_index(name).unwrap_or_else(|| panic!("unknown character {name:?}"));
    &content.rules.characters[ix as usize]
}

/// How that character is drawn: its skin and motion tuning.
pub fn builtin_character_view(name: &str) -> &'static content::character::CharacterView {
    let content = builtin_content();
    let ix = content.names.character_index(name).unwrap_or_else(|| panic!("unknown character {name:?}"));
    &content.views.characters[ix as usize]
}

/// A simulation duration in view seconds, so view timings follow the sim's
/// tick constants instead of copying them.
pub const fn seconds(ticks: u64) -> f32 {
    ticks as f32 / content::TICKS_PER_SECOND as f32
}

/// A simulation constant as a view `f32`, usable in `const` items.
pub const fn view_f32(value: Fx) -> f32 {
    value.to_bits() as f32 / 4_294_967_296.0
}

pub(crate) fn angle_between(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Fills the settlement view from the simulation.
pub fn mirror_settlement(
    mirror: Res<WorldMirror>,
    service: Res<SessionService>,
    scene: Res<SceneView>,
    mut settlement: ResMut<crate::settlement::Settlement>,
) {
    let (Some(current), Some(scenario)) = (&mirror.current, scene.scene().scenario.as_ref()) else {
        settlement.simulation = None;
        return;
    };
    let config = settlement.config.get_or_insert_with(|| crate::legacy::Scenario::new(scenario)).clone();
    let mut view = crate::legacy::Simulation::new(&config, mirror.previous.as_ref(), current, mirror.tick, mirror.alpha, mirror.center, &mirror.events);
    view.paused = service.paused;
    settlement.simulation = Some(view);
}

/// Left click attacks toward the cursor (Z/X/C/V pick a weapon in
/// `input::gameplay::prepare`, which runs first).
pub fn attack_input(
    input: Res<debug_ui::GameInput>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    scene: Res<SceneView>,
    settlement: Option<Res<crate::settlement::Settlement>>,
    piles: Option<Res<crate::piles::PileLab>>,
    mut controls: ResMut<Controls>,
    mut service: ResMut<SessionService>,
    mut control: ResMut<PlayerControl>,
    player: Option<Single<&Transform, With<Player>>>,
) {
    let (keys, buttons) = (input.keys(), input.mouse());
    use content::scenario::Weapon;
    let origin = aim_origin(&service, player.map(|player| player.translation));
    // Update on every input frame, including frames without a shot.
    let cursor_aim = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &scene, &service)
        .zip(origin)
        .map(|(point, origin)| crate::navigation::flat(point - origin).normalize_or_zero())
        .filter(|aim| aim.length_squared() > 0.0);
    // While the debug UI has the pointer, keep the last aim.
    let pointer_free = !input.capture().pointer && !input.ui_owns_pointer();
    if controls.weapon == Weapon::Laser {
        if let Some(aim) = cursor_aim.filter(|_| pointer_free) {
            controls.aim = Some(aim);
            control.aim(&mut service, aim);
        }
    } else {
        controls.aim = None;
    }
    if !attack_click(keys, buttons, pointer_free, piles.as_ref().is_some_and(|lab| lab.stock.is_some())) {
        return;
    }
    if settlement.as_ref().and_then(|s| s.simulation.as_ref()).is_some_and(|sim| {
        crate::gameplay::cursor_ground(camera.0, camera.1, &window)
            .is_some_and(|point| crate::settlement::depot_at(sim, point).is_some())
    }) { return; }
    if let Some(aim) = cursor_aim {
        control.strike(&mut service, &mut controls, &scene.pack, aim);
    }
}

/// Where the mouse aims from: where the avatar is `drawn`. A joined
/// player's avatar is predicted about an input lead ahead of the confirmed
/// one, and its shot fires from there (`event_prediction`). Elsewhere the
/// avatar is drawn where it is confirmed, so the confirmed position is used
/// exactly, as before prediction.
fn aim_origin(service: &SessionService, drawn: Option<Vec3>) -> Option<Vec3> {
    if service.is_client() {
        drawn.or_else(|| service.player_view_position())
    } else {
        service.player_view_position()
    }
}

fn attack_click(keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>, pointer_free: bool, pile_active: bool) -> bool {
    buttons.just_pressed(MouseButton::Left) && pointer_free && !pile_active
        && !keys.pressed(KeyCode::ControlLeft) && !keys.pressed(KeyCode::ControlRight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::Launch;

    #[test]
    fn plain_left_click_attacks_and_shift_does_not_gate_it() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Right);
        assert!(!attack_click(&keys, &buttons, true, false));
        buttons.press(MouseButton::Left);
        assert!(attack_click(&keys, &buttons, true, false));
        keys.press(KeyCode::ShiftLeft);
        assert!(attack_click(&keys, &buttons, true, false));
        assert!(!attack_click(&keys, &buttons, false, false));
        assert!(!attack_click(&keys, &buttons, true, true));
        keys.press(KeyCode::ControlLeft);
        assert!(!attack_click(&keys, &buttons, true, false));
    }

    #[test]
    fn rendered_feet_can_sample_walkable_mine_terraces() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let library = SceneLibrary::new();
        let mine = library.index("dig-lab").unwrap();
        let mut app = crate::test_app(options, library, hub, Launch::single_player(mine));
        crate::start_world(&mut app);
        let service = app.world().resource::<SessionService>();
        assert!(service.world().unwrap().terrain().mine.is_some());
        let mut walkable_step = None;
        for z in -30..30 {
            for x in -30..30 {
                let from = Vec3::new(x as f32, 0.0, z as f32);
                let to = from + Vec3::X;
                let (Some(low), Some(high)) = (service.ground_height(from), service.ground_height(to)) else { continue };
                if (high - low).abs() == 0.25 && service.world().unwrap().terrain().walkable(
                    service.to_sim(from), service.to_sim(to), service.player_radius().unwrap(),
                ) {
                    walkable_step = Some((from, low, high));
                    break;
                }
            }
            if walkable_step.is_some() { break; }
        }
        let (point, low, high) = walkable_step.expect("seeded mine should contain a quarter-meter terrace");
        assert_eq!(service.ground_height(point), Some(low));
        assert_eq!(service.ground_height(point + Vec3::X), Some(high));
    }

    fn player_position(app: &mut App) -> Vec3 {
        let mut players = app.world_mut().query_filtered::<&Transform, With<Player>>();
        players.single(app.world()).unwrap().translation
    }

    /// The local avatar's confirmed position in the newest tick.
    fn confirmed(app: &App) -> Vec3 {
        let mirror = app.world().resource::<WorldMirror>();
        let index = mirror.player().unwrap() as usize;
        mirror.to_view(mirror.current.as_ref().unwrap().agents[index].body.position)
    }

    /// An app on the field at a fixed frame rate, launching straight into a
    /// session.
    fn field_app(network: Option<Network>, fps: f64) -> App {
        scene_app("field", network, fps)
    }

    fn scene_app(scene: &str, network: Option<Network>, fps: f64) -> App {
        scene_app_with(scene, network, fps, &|_, _| {})
    }

    /// Changes a scene's pack before a test app loads it; it may read the
    /// other scenes.
    type EditScene = dyn Fn(&SceneLibrary, &mut voxel_assets::ScenePack);

    /// `scene_app` with the scene's pack changed by `edit` first.
    fn scene_app_with(scene: &str, network: Option<Network>, fps: f64, edit: &EditScene) -> App {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut library = SceneLibrary::new();
        let index = library.index(scene).unwrap();
        let mut pack = (*library.scenes[index]).clone();
        edit(&library, &mut pack);
        library.scenes[index] = std::sync::Arc::new(pack);
        let mut app = crate::test_app(options, library, hub, Launch { network, ..Launch::single_player(index) });
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / fps)));
        app
    }

    fn world_active(app: &App) -> bool {
        app.world().get_resource::<State<WorldState>>().is_some_and(|s| *s.get() == WorldState::Active)
    }

    fn assert_ground_visible(app: &mut App) -> usize {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &Visibility), With<crate::digging::GroundChunk>>();
        let mut count = 0;
        for (mesh, material, visibility) in query.iter(world) {
            assert_ne!(*visibility, Visibility::Hidden);
            assert!(world.resource::<Assets<Mesh>>().get(&mesh.0).unwrap().count_vertices() > 0);
            assert!(world.resource::<Assets<StandardMaterial>>().get(&material.0).is_some());
            count += 1;
        }
        assert!(count > 0, "Dig Lab must draw terrain when its default deck is hidden");
        count
    }

    #[test]
    fn a_joined_mine_draws_terrain_after_the_save_arrives_and_on_world_changes() {
        let mut host = field_app(Some(Network::Host("127.0.0.1:0".parse().unwrap())), 60.0);
        // MinimalPlugins omits the renderer's VisibilityPlugin.
        host.world_mut().register_required_components::<Mesh3d, Visibility>();
        crate::start_world(&mut host);
        let mine = host.world().resource::<SceneLibrary>().index("dig-lab").unwrap();
        crate::switch_world(&mut host, mine);
        let chunks = assert_ground_visible(&mut host);
        let address = host.world().resource::<SessionService>().session.local_addr().unwrap();
        let mut client = field_app(Some(Network::Join(address)), 60.0);
        client.world_mut().register_required_components::<Mesh3d, Visibility>();

        // Stop polling the host as soon as its welcome arrives. No save
        // chunks can arrive until it processes the client's acknowledgement.
        let mut welcomed = false;
        for _ in 0..240 {
            client.update();
            if client.world().get_resource::<SessionService>().is_some_and(|s| s.session.world_number() > 0 && s.world().is_none()) {
                welcomed = true;
                break;
            }
            host.update();
        }
        assert!(welcomed, "the test must exercise the gap between welcome and save");
        for _ in 0..5 {
            client.update();
            assert!(client.world().get_resource::<SceneView>().is_none(), "views started without the host's terrain");
        }

        for _ in 0..600 {
            host.update();
            client.update();
            if world_active(&client) && client.world().resource::<WorldMirror>().player().is_some() { break; }
        }
        assert!(world_active(&client));
        assert_eq!(client.world().resource::<SceneView>().index, mine);
        assert_eq!(assert_ground_visible(&mut client), chunks);

        // Returning to a mine must rebuild its ground, including on peers
        // that have to download another save within an existing session.
        for index in [0, mine] {
            host.world_mut().resource_mut::<SessionService>().request_scene(index);
            for _ in 0..600 {
                host.update();
                client.update();
                if world_active(&host) && world_active(&client)
                    && host.world().resource::<SceneView>().index == index
                    && client.world().resource::<SceneView>().index == index { break; }
            }
            assert!(world_active(&client));
            assert_eq!(client.world().resource::<SceneView>().index, index);
        }
        assert_eq!(assert_ground_visible(&mut client), chunks);
    }

    #[test]
    fn solo_avatar_shows_the_latest_tick_on_the_frame_it_runs() {
        let mut app = field_app(None, 120.0);
        crate::start_world(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        let mut observed_movement = false;
        for _ in 0..12 {
            app.update();
            let mirror = app.world().resource::<WorldMirror>();
            let Some(index) = mirror.player() else { continue };
            let current_position = confirmed(&app);
            let previous_position = mirror
                .previous
                .as_ref()
                .map(|previous| mirror.to_view(previous.agents[index as usize].body.position))
                .unwrap_or(current_position);
            if current_position.distance(previous_position) < 0.001 {
                continue;
            }
            let shown = player_position(&mut app);
            assert!(
                crate::navigation::flat(shown - current_position).length() < 0.001,
                "local avatar lagged behind its latest simulated pose: {shown:?} vs {current_position:?}"
            );
            observed_movement = true;
            break;
        }
        assert!(observed_movement, "movement never reached the simulation");
    }

    #[test]
    fn host_avatar_responds_on_the_next_tick_and_shows_that_pose() {
        let mut app = field_app(Some(Network::Host("127.0.0.1:0".parse().unwrap())), 120.0);
        crate::start_world(&mut app);
        let start = player_position(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        for frame in 1..=4 {
            app.update();
            let confirmed = confirmed(&app);
            if crate::navigation::flat(confirmed - start).length() > 0.001 {
                let shown = player_position(&mut app);
                assert!(
                    crate::navigation::flat(shown - confirmed).length() < 0.001,
                    "host avatar displayed an older tick after movement applied"
                );
                assert!(frame <= 2, "host movement waited {frame} frames");
                return;
            }
        }
        panic!("host input did not move the avatar by the next simulation tick");
    }

    #[test]
    fn joined_player_movement_response_is_measured_in_frames() {
        let mut host = field_app(Some(Network::Host("127.0.0.1:0".parse().unwrap())), 60.0);
        crate::start_world(&mut host);
        let address = host.world().resource::<SessionService>().session.local_addr().unwrap();
        let mut client = field_app(Some(Network::Join(address)), 60.0);
        let mut joined = false;
        for _ in 0..240 {
            host.update();
            client.update();
            if world_active(&client) && client.world().resource::<WorldMirror>().player().is_some() {
                joined = true;
                break;
            }
        }
        assert!(joined, "client never acquired an avatar");
        host.update();
        client.update();
        let start = player_position(&mut client);
        client.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        let mut visible_after = None;
        for frame in 1..=60 {
            client.update();
            host.update();
            if crate::navigation::flat(player_position(&mut client) - start).length() > 0.001 {
                assert!(
                    crate::navigation::flat(confirmed(&client) - start).length() < 0.001,
                    "the test did not observe a visual preview before the confirmed move"
                );
                visible_after = Some(frame);
                break;
            }
        }
        let frames = visible_after.expect("joined player's input produced no visible movement");
        println!("joined player's first visible movement: {frames} frames after key press");
        assert!(frames <= 2, "joined player took {frames} frames to visibly respond");
        for _ in 0..20 {
            client.update();
            host.update();
        }
        client.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyW);
        let mut last = player_position(&mut client);
        for _ in 0..45 {
            client.update();
            host.update();
            let shown = player_position(&mut client);
            assert!(crate::navigation::flat(shown - last).length() < 0.2, "the local avatar jumped while reconciling with the host");
            last = shown;
        }
        let settled = confirmed(&client);
        assert!(
            crate::navigation::flat(last - settled).length() < 0.1,
            "the local avatar did not settle on the host's confirmed pose"
        );
        let goal = settled + Vec3::new(0.6, 0.0, 0.0);
        client.world_mut().resource_scope(|world, mut service: Mut<SessionService>| {
            world.resource_mut::<PlayerControl>().go_to(&mut service, goal, false);
        });
        client.update();
        host.update();
        // A walk to a point (the dock walk, scripted walks) is predicted:
        // the avatar follows the route the replay plans, then settles on the
        // confirmed one without jumps.
        let route_preview = player_position(&mut client);
        let mut followed_route = false;
        let mut previous_shown = route_preview;
        for _ in 0..90 {
            client.update();
            host.update();
            let shown = player_position(&mut client);
            assert!(
                crate::navigation::flat(shown - previous_shown).length() < 0.2,
                "routed movement preview jumped while reconciling with the host"
            );
            previous_shown = shown;
            if crate::navigation::flat(confirmed(&client) - settled).length() > 0.05 {
                followed_route = true;
            }
        }
        assert!(followed_route, "the joined avatar stopped following confirmed routed movement");
        assert!(
            crate::navigation::flat(previous_shown - confirmed(&client)).length() < 0.1,
            "routed movement preview did not settle on the confirmed route"
        );
    }

    /// A UDP relay that holds every datagram for `delay ± jitter` of virtual
    /// time, like `tools/network_lab/relay.mjs`. The client joins `listen`;
    /// the relay talks to the host from `upstream` and answers the client
    /// from `listen`, which the client checks.
    struct DelayRelay {
        listen: std::net::UdpSocket,
        upstream: std::net::UdpSocket,
        host: SocketAddr,
        client: Option<SocketAddr>,
        delay: f64,
        jitter: f64,
        rng: u64,
        /// (due, sequence, to the host, bytes)
        held: Vec<(f64, u64, bool, Vec<u8>)>,
        sent: u64,
    }

    impl DelayRelay {
        fn new(host: SocketAddr, delay_ms: f64, jitter_ms: f64, seed: u64) -> DelayRelay {
            let bind = || {
                let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
                socket.set_nonblocking(true).unwrap();
                socket
            };
            DelayRelay {
                listen: bind(),
                upstream: bind(),
                host,
                client: None,
                delay: delay_ms / 1000.0,
                jitter: jitter_ms / 1000.0,
                rng: seed | 1,
                held: Vec::new(),
                sent: 0,
            }
        }

        fn address(&self) -> SocketAddr {
            self.listen.local_addr().unwrap()
        }

        /// Uniform in [-1, 1), from a seeded xorshift so runs repeat.
        fn noise(&mut self) -> f64 {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            (self.rng >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        }

        fn hold(&mut self, now: f64, to_host: bool, bytes: Vec<u8>) {
            let due = now + (self.delay + self.jitter * self.noise()).max(0.0);
            self.held.push((due, self.sent, to_host, bytes));
            self.sent += 1;
        }

        /// Takes what arrived and sends what is due at `now` (seconds).
        fn pump(&mut self, now: f64) {
            let mut buffer = [0u8; 65536];
            while let Ok((len, from)) = self.listen.recv_from(&mut buffer) {
                self.client = Some(from);
                self.hold(now, true, buffer[..len].to_vec());
            }
            while let Ok((len, from)) = self.upstream.recv_from(&mut buffer) {
                if from == self.host {
                    self.hold(now, false, buffer[..len].to_vec());
                }
            }
            self.held.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let due = self.held.iter().take_while(|(at, ..)| *at <= now).count();
            for (_, _, to_host, bytes) in self.held.drain(..due) {
                if to_host {
                    let _ = self.upstream.send_to(&bytes, self.host);
                } else if let Some(client) = self.client {
                    let _ = self.listen.send_to(&bytes, client);
                }
            }
        }
    }

    /// A host and a client joined through a `DelayRelay`, stepped together
    /// on the 60 Hz virtual clock.
    struct RelayedGame {
        host: App,
        client: App,
        relay: DelayRelay,
        frame: u64,
    }

    impl RelayedGame {
        fn new(delay_ms: f64, jitter_ms: f64, seed: u64) -> RelayedGame {
            RelayedGame::in_scene("field", delay_ms, jitter_ms, seed)
        }

        fn in_scene(scene: &str, delay_ms: f64, jitter_ms: f64, seed: u64) -> RelayedGame {
            RelayedGame::in_scene_with(scene, delay_ms, jitter_ms, seed, &|_, _| {})
        }

        /// `in_scene` with the scene's pack changed by `edit` on both
        /// machines (the client draws threats from its own copy).
        fn in_scene_with(scene: &str, delay_ms: f64, jitter_ms: f64, seed: u64, edit: &EditScene) -> RelayedGame {
            let mut host = scene_app_with(scene, Some(Network::Host("127.0.0.1:0".parse().unwrap())), 60.0, edit);
            crate::start_world(&mut host);
            let address = host.world().resource::<SessionService>().session.local_addr().unwrap();
            let relay = DelayRelay::new(address, delay_ms, jitter_ms, seed);
            let client = scene_app_with(scene, Some(Network::Join(relay.address())), 60.0, edit);
            let mut game = RelayedGame { host, client, relay, frame: 0 };
            for _ in 0..1200 {
                game.frame();
                if world_active(&game.client) && game.client.world().resource::<WorldMirror>().player().is_some() {
                    return game;
                }
            }
            panic!("the client never acquired an avatar through the relay");
        }

        fn frame(&mut self) {
            self.frame += 1;
            let now = self.frame as f64 / 60.0;
            self.client.update();
            self.relay.pump(now);
            self.host.update();
            self.relay.pump(now);
        }

        /// Holds exactly `held` of the movement keys for `frames` frames.
        fn hold(&mut self, held: &[KeyCode], frames: u32) {
            press_only(&mut self.client, held);
            for _ in 0..frames {
                self.frame();
            }
        }

        /// The client fires its laser toward `direction` (arena space), as a
        /// click there would.
        fn fire(&mut self, direction: Vec3) {
            fire_laser(&mut self.client, direction);
        }

        fn telemetry(&mut self) -> Mut<'_, crate::telemetry::Telemetry> {
            self.client.world_mut().resource_mut::<crate::telemetry::Telemetry>()
        }

        /// Client inputs the host moved to a later tick so far, and the ticks
        /// they moved in all.
        fn late_inputs(&self) -> (u64, u64) {
            let diagnostics = self.host.world().resource::<SessionService>().session.diagnostics(Duration::ZERO);
            diagnostics.peers.iter().fold((0, 0), |(n, ticks), peer| (n + peer.late_inputs, ticks + peer.shifted_ticks))
        }

        /// The client's account of the same, from the turns it ran, its
        /// input margin (ticks) and round trip (ms). Panics if it disagrees
        /// with the host's once every input has applied.
        fn client_margin(&self) -> String {
            let session = &self.client.world().resource::<SessionService>().session;
            let d = session.diagnostics(Duration::ZERO);
            assert_eq!(session.pending_inputs(), 0, "inputs still in flight");
            assert_eq!((d.late_inputs, d.shifted_ticks), self.late_inputs(), "the client saw other late inputs than the host");
            let rtt = session.stats().rtt.map_or(0.0, |rtt| rtt.as_secs_f64() * 1000.0);
            format!(
                "client RTT {rtt:.0} ms, input margin {} ticks ({:.0} ms), late inputs seen by the client {} ({} ticks)",
                d.input_delay,
                d.input_delay as f64 * net::TICK.as_secs_f64() * 1000.0,
                d.late_inputs,
                d.shifted_ticks
            )
        }
    }

    /// `app`'s player fires its laser toward `direction` (arena space), as a
    /// click there would.
    fn fire_laser(app: &mut App, direction: Vec3) {
        strike_with(app, content::scenario::Weapon::Laser, direction);
    }

    /// `app`'s player strikes with `weapon` toward `direction` (arena space).
    fn strike_with(app: &mut App, weapon: content::scenario::Weapon, direction: Vec3) {
        let world = app.world_mut();
        world.resource_mut::<Controls>().weapon = weapon;
        world.resource_scope(|world, mut service: Mut<SessionService>| {
            world.resource_scope(|world, mut controls: Mut<Controls>| {
                world.resource_scope(|world, mut control: Mut<PlayerControl>| {
                    control.strike(&mut service, &mut controls, &world.resource::<SceneView>().pack, direction);
                });
            });
        });
    }

    /// Holds exactly `held` of the movement keys.
    fn press_only(app: &mut App, held: &[KeyCode]) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::ShiftLeft] {
            if held.contains(&key) {
                if !keys.pressed(key) {
                    keys.press(key);
                }
            } else if keys.pressed(key) {
                keys.release(key);
            }
        }
    }

    /// One scenario's movement script: (keys held, frames) steps.
    fn movement_scenarios() -> Vec<(&'static str, Vec<(Vec<KeyCode>, u32)>)> {
        use KeyCode::{KeyA as Left, KeyD as Right, KeyW as Up, ShiftLeft as Run};
        let reversals = |run: bool| {
            let with = |key| if run { vec![key, Run] } else { vec![key] };
            (0..3).flat_map(|_| [(with(Right), 60), (with(Left), 36), (vec![], 36)]).collect()
        };
        vec![
            ("base running", reversals(true)),
            ("base walking", reversals(false)),
            ("quick taps", (0..10).flat_map(|_| [(vec![Right], 3), (vec![], 18)]).collect()),
            ("90 degree turn", vec![(vec![Right, Run], 60), (vec![Up, Run], 60)]),
            ("zig-zag", (0..14).map(|i| (vec![if i % 2 == 0 { Left } else { Right }], 9)).collect()),
            ("speed change", (0..5).map(|i| (if i % 2 == 0 { vec![Right] } else { vec![Right, Run] }, 24)).collect()),
        ]
    }

    #[derive(Default)]
    struct KindTotal {
        count: u32,
        distance: f32,
        largest: f32,
    }

    #[test]
    fn joined_player_movement_needs_no_corrections_under_latency() {
        assert_no_corrections(&[("no added delay", 0.0, 0.0, 0x5eed), ("75 ± 20 ms", 75.0, 20.0, 0x5eed + 1)]);
    }

    #[test]
    fn joined_player_movement_needs_no_corrections_at_100_ms() {
        assert_no_corrections(&[("100 ± 20 ms", 100.0, 20.0, 0x5eed + 2)]);
    }

    #[test]
    fn joined_player_input_margin_settles_near_one_way_on_a_lan() {
        let mut game = RelayedGame::new(0.0, 0.0, 0x1a7);
        assert_eq!(game.client.world().resource::<SessionService>().session.input_delay(), NetConfig::default().input_delay, "the host's margin until measured");
        game.hold(&[KeyCode::KeyD], 120);
        game.hold(&[], 120);
        let session = &game.client.world().resource::<SessionService>().session;
        let rtt = session.stats().rtt.unwrap();
        let one_way = (rtt / 2).as_nanos().div_ceil(net::TICK.as_nanos()) as u64;
        let margin = session.input_delay();
        let report = game.client_margin();
        assert_eq!(margin, one_way + 1, "{report}");
        assert!(net::TICK * margin as u32 <= Duration::from_millis(50), "{report}");
    }

    #[test]
    fn joined_player_movement_needs_no_corrections_at_150_ms() {
        assert_no_corrections(&[("150 ± 20 ms", 150.0, 20.0, 0x5eed + 3)]);
    }

    /// Runs every movement scenario under each (name, one-way delay ms,
    /// ± jitter ms, relay seed) case and fails on any kept correction.
    fn assert_no_corrections(cases: &[(&str, f64, f64, u64)]) {
        let mut kept_total = 0;
        let mut report = String::new();
        for &(name, delay, jitter, seed) in cases {
            let mut game = RelayedGame::new(delay, jitter, seed);
            for (scenario, script) in movement_scenarios() {
                // Start each scenario at the spawn point, settled, with an empty log.
                let agent = game.client.world().resource::<SessionService>().player().unwrap();
                game.client.world_mut().resource_mut::<SessionService>().command(CommandKind::Respawn { agent });
                game.hold(&[], 90);
                game.telemetry().network.clear_corrections();
                let late_before = game.late_inputs();
                for (keys, frames) in script {
                    game.hold(&keys, frames);
                }
                // Long enough for the confirmed avatar to stop, so a final pull shows.
                game.hold(&[], 90);
                let mut kinds = std::collections::BTreeMap::<String, KindTotal>::new();
                let telemetry = game.telemetry();
                let network = &telemetry.network;
                for c in network.corrections() {
                    let total = kinds.entry(format!("{:?}", c.kind)).or_default();
                    total.count += 1;
                    total.distance += c.distance;
                    total.largest = total.largest.max(c.distance);
                    kept_total += 1;
                }
                let (small, small_distance) = network.small_corrections();
                let kept = if kinds.is_empty() {
                    "none".to_string()
                } else {
                    kinds
                        .iter()
                        .map(|(kind, t)| format!("{kind} {} ({:.2} u, largest {:.2})", t.count, t.distance, t.largest))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let late = game.late_inputs();
                let (late, shifted) = (late.0 - late_before.0, late.1 - late_before.1);
                let margin = game.client.world().resource::<SessionService>().session.input_delay();
                report += &format!(
                    "{name:>15} | {scenario:>15} | kept: {kept} | under 0.05: {small} ({small_distance:.3} u) | late inputs: {late} ({shifted} ticks) | margin {margin} ticks\n"
                );
            }
            report += &format!("{name:>15} | {}\n", game.client_margin());
        }
        println!("corrections per latency case and scenario:\n{report}");
        assert_eq!(kept_total, 0, "the joined player's avatar was corrected:\n{report}");
    }

    #[test]
    fn mouse_aim_starts_at_the_drawn_avatar_only_when_it_is_predicted() {
        let elsewhere = Some(Vec3::new(9.0, 0.0, 9.0));
        let mut solo = field_app(None, 60.0);
        crate::start_world(&mut solo);
        let mut host = field_app(Some(Network::Host("127.0.0.1:0".parse().unwrap())), 60.0);
        crate::start_world(&mut host);
        for app in [&solo, &host] {
            let service = app.world().resource::<SessionService>();
            assert!(service.player_view_position().is_some());
            assert_eq!(aim_origin(service, elsewhere), service.player_view_position(), "unpredicted avatars aim from the confirmed position");
        }
        let game = RelayedGame::new(0.0, 0.0, 1);
        let service = game.client.world().resource::<SessionService>();
        assert_eq!(aim_origin(service, elsewhere), elsewhere);
        assert_eq!(aim_origin(service, None), service.player_view_position());
    }

    /// One laser scenario: (keys held, frames, fire toward this first) steps.
    fn laser_scenarios() -> Vec<(&'static str, Vec<(Vec<KeyCode>, u32, Option<Vec3>)>)> {
        use KeyCode::{KeyA as Left, KeyD as Right, ShiftLeft as Run};
        // West toward the hall's far side wall, south toward the low front
        // rail close behind the spawn, and north from the spawn toward the
        // mixing desk (cover). The movement keys run diagonally on this
        // scene's camera, away from the host's avatar.
        let (west, south, north) = (Some(Vec3::NEG_X), Some(Vec3::Z), Some(Vec3::NEG_Z));
        let run = |key| vec![key, Run];
        vec![
            ("standing still", vec![(vec![], 24, west), (vec![], 24, south), (vec![], 24, north), (vec![], 24, west), (vec![], 24, south)]),
            (
                "running",
                vec![(run(Right), 20, None), (run(Right), 20, south), (run(Right), 20, west), (run(Right), 20, south), (run(Right), 20, west)],
            ),
            // The second shot is pressed during the first one's swing, so it waits for it.
            ("right after reversing", (0..3).flat_map(|_| [(run(Right), 40, None), (run(Left), 6, south), (run(Left), 30, west)]).collect()),
        ]
    }

    #[test]
    fn joined_player_laser_shots_are_predicted_under_latency() {
        assert_lasers_predicted(&[("no added delay", 0.0, 0.0, 0x1a5e), ("75 ± 20 ms", 75.0, 20.0, 0x1a5e + 1)]);
    }

    #[test]
    fn joined_player_laser_shots_are_predicted_at_100_ms() {
        assert_lasers_predicted(&[("100 ± 20 ms", 100.0, 20.0, 0x1a5e + 2)]);
    }

    #[test]
    fn joined_player_laser_shots_are_predicted_at_150_ms() {
        assert_lasers_predicted(&[("150 ± 20 ms", 150.0, 20.0, 0x1a5e + 3)]);
    }

    /// Runs every laser scenario in the laser pit under each (name, one-way
    /// delay ms, ± jitter ms, relay seed) case. Each shot the client fires
    /// is drawn once, with one laser sound, before the confirmed world
    /// fires it, and the confirmed shot ends exactly where the drawn one
    /// did. No correction of any kind is kept.
    fn assert_lasers_predicted(cases: &[(&str, f64, f64, u64)]) {
        let mut failures = Vec::new();
        let mut report = String::new();
        for &(name, delay, jitter, seed) in cases {
            let mut game = RelayedGame::in_scene("laser-pit", delay, jitter, seed);
            // The host's avatar shares the spawn point: move it off the
            // client's lines of fire, into the southwest corner behind the client.
            let spawn = confirmed(&game.client);
            let walk = |game: &mut RelayedGame, intent| {
                let service = &mut *game.host.world_mut().resource_mut::<SessionService>();
                let agent = service.player().unwrap();
                service.command(CommandKind::MoveAvatar { agent, intent });
            };
            let southwest = FxVec2::new(Fx::from_int(-1), Fx::ONE).normalize_or_zero();
            walk(&mut game, Intent::Walk { direction: southwest, run: false });
            game.hold(&[], 150);
            walk(&mut game, Intent::Stop);
            game.hold(&[], 30);
            let host_at = confirmed(&game.host);
            assert!(host_at.z > spawn.z + 1.5 && host_at.x < spawn.x - 3.0, "the host's avatar is in the way: {host_at} (spawn {spawn})");
            let agent = game.client.world().resource::<SessionService>().player().unwrap();
            for (scenario, script) in laser_scenarios() {
                game.client.world_mut().resource_mut::<SessionService>().command(CommandKind::Respawn { agent });
                game.hold(&[], 90);
                game.telemetry().network.clear_corrections();
                let sounds = |game: &RelayedGame| game.client.world().resource::<crate::sound::Sound>().recorded_events().iter().filter(|(_, cue)| *cue == crate::sound::Cue::Laser as i32).count();
                let sounds_before = sounds(&game);
                let late_before = game.late_inputs();
                let (mut fired, mut confirmed_shots, mut cover_hits, mut player_hits) = (0, 0, 0, 0);
                let mut beams = std::collections::HashMap::<Entity, bool>::new();
                // Then long enough for every shot to be confirmed, or given up.
                let settle = (Vec::new(), 90, None);
                for (keys, frames, fire) in script.iter().chain([&settle]) {
                    let (frames, fire) = (*frames, *fire);
                    press_only(&mut game.client, keys);
                    if let Some(direction) = fire {
                        game.fire(direction);
                        fired += 1;
                    }
                    for _ in 0..frames {
                        game.frame();
                        let world = game.client.world_mut();
                        for event in &world.resource::<WorldMirror>().events {
                            if let sim::settlement::Event::LaserFired { agent: shooter, hit_actor, cover, .. } = event {
                                if *shooter == agent {
                                    confirmed_shots += 1;
                                    cover_hits += usize::from(cover.is_some());
                                    player_hits += usize::from(*hit_actor);
                                }
                            }
                        }
                        let mut bolts = world.query::<(Entity, &crate::effects::LaserBolt)>();
                        for (entity, bolt) in bolts.iter(world) {
                            beams.entry(entity).or_insert(bolt.predicted());
                        }
                    }
                }
                let telemetry = game.telemetry();
                let network = &telemetry.network;
                let outcomes: Vec<_> = network.event_outcomes("laser").cloned().collect();
                let kinds: Vec<_> = network.corrections().map(|c| format!("{:?} {:.2}", c.kind, c.distance)).collect();
                let laser_mispredictions = network.corrections().filter(|c| c.kind == crate::telemetry::CorrectionKind::EventMispredict { kind: "laser" }).count();
                let drawn = network.events_presented("laser") as usize;
                let sounds = sounds(&game) - sounds_before;
                let predicted_beams = beams.values().filter(|p| **p).count();
                let end_difference = outcomes.iter().map(|o| o.confirmed.map_or(f32::INFINITY, |c| c.distance(o.predicted))).fold(0.0, f32::max);
                let leads: Vec<f64> = outcomes.iter().filter_map(|o| o.lead).collect();
                let (lead_min, lead_max) = leads.iter().fold((f64::INFINITY, 0.0f64), |(lo, hi), &l| (lo.min(l), hi.max(l)));
                let late = game.late_inputs();
                let (late, shifted) = (late.0 - late_before.0, late.1 - late_before.1);
                let client_at = confirmed(&game.client);
                let margin = game.client.world().resource::<SessionService>().session.input_delay();
                report += &format!(
                    "{name:>15} | {scenario:>21} | ends at ({:.1}, {:.1}) |",
                    client_at.x,
                    client_at.z
                );
                report += &format!(
                    " fired {fired}, confirmed {confirmed_shots} ({cover_hits} on cover), drawn early {drawn}, beams {} ({predicted_beams} predicted), laser sounds {sounds} | laser mispredictions {laser_mispredictions}, largest end difference {end_difference:.4} u | drawn {:.0}–{:.0} ms before confirmation | other corrections: {} | late inputs: {late} ({shifted} ticks) | margin {margin} ticks\n",
                    beams.len(),
                    lead_min * 1000.0,
                    lead_max * 1000.0,
                    kinds.len() - laser_mispredictions,
                );
                let exact = outcomes.len() == fired && outcomes.iter().all(|o| o.exact && o.confirmed.is_some());
                let early = leads.len() == fired && leads.iter().all(|&lead| lead > 0.0);
                if player_hits > 0 {
                    failures.push(format!("{name} / {scenario}: {player_hits} shots hit the host's avatar"));
                }
                if scenario == "standing still" && cover_hits == 0 {
                    failures.push(format!("{name} / {scenario}: no shot hit the mixing desk"));
                }
                if !(confirmed_shots == fired && drawn == fired && exact && early && kinds.is_empty() && beams.len() == fired && predicted_beams == fired && sounds == fired) {
                    failures.push(format!("{name} / {scenario}: corrections {kinds:?}, outcomes {outcomes:?}"));
                }
            }
            report += &format!("{name:>15} | {}, host avatar at {host_at}\n", game.client_margin());
        }
        println!("laser shots per latency case and scenario:\n{report}");
        assert!(failures.is_empty(), "laser shots were not predicted exactly:\n{report}\n{}", failures.join("\n"));
    }

    /// Under 100 ± 20 ms, the client fires the arc coil in the combat arena.
    /// A shot's arc and sound are presented once, an input lead before the
    /// confirmed world fires it, and the confirmed shot isn't presented
    /// again (`settlement::ArcChain`). With the prediction withdrawn, the
    /// shot is presented once, as it is confirmed.
    #[test]
    fn joined_player_arc_shots_are_presented_early_once() {
        let mut game = RelayedGame::in_scene("combat-arena", 100.0, 20.0, 0xa2c);
        game.hold(&[], 60);
        let agent = game.client.world().resource::<SessionService>().player().unwrap();
        let sounds = |game: &RelayedGame| game.client.world().resource::<crate::sound::Sound>().recorded_events().iter().filter(|(_, cue)| *cue == crate::sound::Cue::Arc as i32).count();
        let presented = |game: &mut RelayedGame| game.telemetry().network.events_presented("arc");
        let mut report = String::new();
        for (shot, withdraw) in [(0, false), (1, true), (2, false)] {
            let (sounds_before, presented_before) = (sounds(&game), presented(&mut game));
            if withdraw {
                // Withdrawn until well past the shot's confirmation.
                let horizon = game.client.world().resource::<SessionService>().session.input_horizon().unwrap();
                game.client.world_mut().insert_resource(crate::prediction::WithdrawPrediction(horizon + 60));
            }
            strike_with(&mut game.client, content::scenario::Weapon::ArcCoil, Vec3::NEG_Z);
            let (mut played_at, mut confirmed_at) = (None, None);
            for frame in 0..120 {
                game.frame();
                if played_at.is_none() && sounds(&game) > sounds_before {
                    played_at = Some(frame);
                }
                let fired = game.client.world().resource::<WorldMirror>().events.iter().any(|e| matches!(e, sim::settlement::Event::ArcFired { agent: by, .. } if *by == agent));
                if fired && confirmed_at.is_none() {
                    confirmed_at = Some(frame);
                }
            }
            game.client.world_mut().remove_resource::<crate::prediction::WithdrawPrediction>();
            let (played, early) = (sounds(&game) - sounds_before, presented(&mut game) - presented_before);
            report += &format!("shot {shot}{}: presented early {early}, arc sounds {played}, sound on frame {played_at:?}, confirmed on {confirmed_at:?}\n", if withdraw { " (withdrawn)" } else { "" });
            let (Some(played_at), Some(confirmed_at)) = (played_at, confirmed_at) else { panic!("the shot was never shown:\n{report}") };
            assert_eq!(played, 1, "the shot was presented other than once:\n{report}");
            if withdraw {
                assert_eq!(early, 0, "a withdrawn prediction presented the shot:\n{report}");
                assert!(played_at >= confirmed_at, "the withdrawn shot was presented before it was confirmed:\n{report}");
            } else {
                assert_eq!(early, 1, "{report}");
                assert!(played_at + 4 <= confirmed_at, "the shot waited for its confirmation:\n{report}");
            }
            game.hold(&[], 60);
        }
        let telemetry = game.telemetry();
        let outcomes: Vec<_> = telemetry.network.event_outcomes("arc").cloned().collect();
        report += &format!("outcomes {outcomes:?}\n");
        assert!(outcomes.len() == 2 && outcomes.iter().all(|o| o.confirmed.is_some() && o.lead.is_some_and(|lead| lead > 0.0)), "{report}");
        println!("{report}");
    }

    /// Under 100 ± 20 ms, the client looses arrows in the combat arena. A
    /// shot's arrow is drawn in flight on the frame its sound plays, an input
    /// lead before the confirmed world fires it, and the confirmed
    /// projectile takes that arrow over on the frame it is confirmed: the
    /// sound isn't played again, and the arrow is gone once the projectile
    /// is. With the prediction withdrawn, no arrow is predicted, and the
    /// sound comes as the shot is confirmed.
    #[test]
    fn joined_player_bow_shots_draw_their_arrow_early_and_hand_it_over() {
        let mut game = RelayedGame::in_scene("combat-arena", 100.0, 20.0, 0xb0e);
        game.hold(&[], 60);
        let agent = game.client.world().resource::<SessionService>().player().unwrap();
        let sounds = |game: &RelayedGame| game.client.world().resource::<crate::sound::Sound>().recorded_events().iter().filter(|(_, cue)| *cue == crate::sound::Cue::Bow as i32).count();
        let presented = |game: &mut RelayedGame| game.telemetry().network.events_presented("bow");
        let arrows = |game: &RelayedGame| game.client.world().resource::<crate::settlement::PredictedArrows>().0.clone();
        let mut report = String::new();
        for (shot, withdraw) in [(0, false), (1, true), (2, false)] {
            let (sounds_before, presented_before) = (sounds(&game), presented(&mut game));
            if withdraw {
                let horizon = game.client.world().resource::<SessionService>().session.input_horizon().unwrap();
                game.client.world_mut().insert_resource(crate::prediction::WithdrawPrediction(horizon + 60));
            }
            strike_with(&mut game.client, content::scenario::Weapon::Bow, Vec3::NEG_Z);
            let (mut played_at, mut confirmed_at, mut drawn_at, mut taken_over_at) = (None, None, None, None);
            for frame in 0..120 {
                game.frame();
                if played_at.is_none() && sounds(&game) > sounds_before {
                    played_at = Some(frame);
                }
                let fired = game.client.world().resource::<WorldMirror>().events.iter().any(|e| matches!(e, sim::settlement::Event::BowFired { agent: by, .. } if *by == agent));
                if fired && confirmed_at.is_none() {
                    confirmed_at = Some(frame);
                }
                let arrows = arrows(&game);
                if drawn_at.is_none() && !arrows.is_empty() {
                    drawn_at = Some(frame);
                }
                if taken_over_at.is_none() && arrows.iter().any(|arrow| arrow.projectile.is_some()) {
                    taken_over_at = Some(frame);
                }
            }
            game.client.world_mut().remove_resource::<crate::prediction::WithdrawPrediction>();
            let (played, early) = (sounds(&game) - sounds_before, presented(&mut game) - presented_before);
            report += &format!(
                "shot {shot}{}: presented early {early}, bow sounds {played}, sound on frame {played_at:?}, arrow drawn on {drawn_at:?}, taken over on {taken_over_at:?}, confirmed on {confirmed_at:?}\n",
                if withdraw { " (withdrawn)" } else { "" }
            );
            let (Some(played_at), Some(confirmed_at)) = (played_at, confirmed_at) else { panic!("the shot was never shown:\n{report}") };
            assert_eq!(played, 1, "the shot's sound played other than once:\n{report}");
            assert!(arrows(&game).is_empty(), "a predicted arrow outlived its shot:\n{report}");
            if withdraw {
                assert_eq!((early, drawn_at), (0, None), "a withdrawn prediction drew the arrow:\n{report}");
                assert!(played_at >= confirmed_at, "the withdrawn shot was presented before it was confirmed:\n{report}");
            } else {
                assert_eq!(early, 1, "{report}");
                assert_eq!(drawn_at, Some(played_at), "the sound and the arrow came apart:\n{report}");
                assert!(played_at + 4 <= confirmed_at, "the shot waited for its confirmation:\n{report}");
                assert_eq!(taken_over_at, Some(confirmed_at), "the projectile didn't take the arrow over as it was confirmed:\n{report}");
            }
            game.hold(&[], 60);
        }
        let telemetry = game.telemetry();
        let outcomes: Vec<_> = telemetry.network.event_outcomes("bow").cloned().collect();
        report += &format!("outcomes {outcomes:?}\n");
        assert!(outcomes.len() == 2 && outcomes.iter().all(|o| o.confirmed.is_some() && o.lead.is_some_and(|lead| lead > 0.0)), "{report}");
        println!("{report}");
    }

    /// Under 100 ± 20 ms, the host shoots the client's running avatar with
    /// the laser in the laser pit until it dies. A hit is stage 5, which the
    /// client's prediction never runs: one that doesn't move the avatar
    /// changes nothing drawn, and the death, which stops it, is a
    /// `Knockback`. Neither is a misprediction. The respawn that follows is
    /// the avatar's own step, so it is predicted and corrects nothing.
    #[test]
    fn joined_player_hit_by_the_laser_is_knocked_back_not_mispredicted() {
        use crate::telemetry::CorrectionKind;
        use KeyCode::{KeyA as Left, KeyD as Right, ShiftLeft as Run};
        let mut game = RelayedGame::in_scene("laser-pit", 100.0, 20.0, 0xb1a5);
        let agent = game.client.world().resource::<SessionService>().player().unwrap();
        let shooter = game.host.world().resource::<SessionService>().player().unwrap();
        let touched = |game: &RelayedGame| {
            let service = game.client.world().resource::<SessionService>();
            service.stage_five.touched(service.session.world_number(), agent).to_vec()
        };
        // Away from the host's avatar at the spawn.
        game.hold(&[Right, Run], 60);
        assert_eq!(touched(&game), Vec::<u64>::new(), "running is no stage 5 touch");
        game.telemetry().network.clear_corrections();
        let health = |game: &RelayedGame| game.host.world().resource::<SessionService>().settlement().unwrap().agents[agent as usize].body.health;
        let (mut shots, mut hits, mut died_at, mut touches) = (0, 0, None, 0);
        for frame in 0..900u32 {
            if died_at.is_none() {
                press_only(&mut game.client, if frame % 70 < 40 { &[Right, Run] } else { &[Left, Run] });
                if frame % 15 == 0 {
                    // At the client's avatar where the host has it, a little ahead.
                    let mirror = game.host.world().resource::<WorldMirror>();
                    let agents = &mirror.current.as_ref().unwrap().agents;
                    let (target, from) = (&agents[agent as usize], &agents[shooter as usize]);
                    let velocity = Vec3::new(target.body.velocity.x.to_f32(), 0.0, target.body.velocity.y.to_f32());
                    let aim = crate::navigation::flat(mirror.to_view(target.body.position) + velocity * 0.05 - mirror.to_view(from.body.position));
                    fire_laser(&mut game.host, aim.normalize_or_zero());
                    shots += 1;
                }
            } else {
                press_only(&mut game.client, &[]);
            }
            game.frame();
            for event in &game.host.world().resource::<WorldMirror>().events {
                if let sim::settlement::Event::LaserFired { agent: by, hit_actor: true, .. } = event {
                    hits += usize::from(*by == shooter);
                }
            }
            touches = touches.max(touched(&game).len());
            if died_at.is_none() && health(&game) == 0 {
                died_at = Some(frame);
            }
            if died_at.is_some_and(|at| frame > at + 240) {
                break;
            }
        }
        let telemetry = game.telemetry();
        let kinds: Vec<_> = telemetry.network.corrections().map(|c| format!("{:?} {:.2}{}", c.kind, c.distance, if c.snapped { " snapped" } else { "" })).collect();
        let report = format!("{shots} shots, {hits} hits, died on frame {died_at:?}, up to {touches} touched ticks remembered, corrections {kinds:?}");
        println!("{report}");
        assert!(died_at.is_some() && hits >= 6, "the host never killed the client: {report}");
        assert!(touches > 0, "no hit was seen as a stage 5 touch: {report}");
        let knockbacks = telemetry.network.corrections().filter(|c| c.kind == CorrectionKind::Knockback).count();
        assert!(knockbacks > 0, "the death stopped the running avatar, a knockback: {report}");
        assert!(telemetry.network.corrections().all(|c| c.kind == CorrectionKind::Knockback && !c.snapped), "only knockbacks, smoothed: {report}");
        let snapshot = telemetry.network.snapshot();
        assert_eq!((snapshot["preview_corrections"].as_u64(), snapshot["preview_mispredicted_frames"].as_u64()), (Some(0), Some(0)), "{report}");
    }

    /// Under 100 ± 20 ms, a ghoul walking across the laser pit (given one
    /// wave; the pit has player health) hits the client's avatar while it
    /// runs back and forth across the ghoul. The swing is stage 5, which the
    /// replay never runs, so its push is a `Knockback`: smoothed (below the
    /// 3 u knockback snap), and nothing is a misprediction.
    #[test]
    fn joined_player_pushed_by_a_ghoul_is_knocked_back_not_mispredicted() {
        use crate::telemetry::CorrectionKind;
        use KeyCode::{KeyA as Left, KeyD as Right, KeyS as Down, KeyW as Up, ShiftLeft as Run};
        let mut game = RelayedGame::in_scene_with("laser-pit", 100.0, 20.0, 0x6001, &|library, pack| {
            // The ghoul's skin comes from the forest, which has ghouls, with
            // the forest's materials after the pit's.
            let forest = &library.scenes[library.index("forest-outpost").unwrap()];
            let style = pack.scene.style.as_mut().unwrap();
            let base = style.materials.len();
            style.materials.extend(forest.scene.style.as_ref().unwrap().materials.iter().cloned());
            let mut skin = forest.skins.iter().find(|skin| skin.id == "ghoul").unwrap().clone();
            skin.parts.iter_mut().flat_map(|part| &mut part.meshes).for_each(|mesh| mesh.material += base);
            pack.skins.push(skin);
            // From beside the joiner (at the spawn's z, 0.9 m along x:
            // `Settlement::join`) toward the stage, clear of the cover.
            let q = Fx::quantize;
            pack.scene.scenario.as_mut().unwrap().waves = vec![content::scenario::Wave {
                id: "ghoul".into(),
                position: [q(2.5), q(11.5)],
                target: [q(2.5), q(-4.0)],
                character: "ghoul_forest".into(),
                first: q(1.0),
                every: q(60.0),
                count: 1,
            }];
        });
        let agent = game.client.world().resource::<SessionService>().player().unwrap();
        assert!(game.host.world().resource::<SessionService>().settlement().unwrap().player_health > 0, "players can be hurt");
        let ghoul = |game: &RelayedGame| {
            let mirror = game.client.world().resource::<WorldMirror>();
            mirror.current.as_ref().unwrap().threats.first().map(|t| mirror.to_view(t.position))
        };
        let drawn = |game: &RelayedGame| {
            let mirror = game.client.world().resource::<WorldMirror>();
            crate::navigation::flat(mirror.to_view(drawn_avatar(&game.client).to.body.position))
        };
        // The pit's follow camera turns the keys: find where D and W go.
        let axis = |game: &mut RelayedGame, key: KeyCode| {
            game.hold(&[], 30);
            let from = drawn(game);
            game.hold(&[key], 10);
            (drawn(game) - from).normalize()
        };
        let (along, across) = (axis(&mut game, Right), axis(&mut game, Up));
        game.hold(&[], 30);
        game.telemetry().network.clear_corrections();
        // Runs at the ghoul and across it diagonally, turning on each axis
        // once 0.15 m past it.
        let (mut ahead, mut hit_at, mut speed, mut swings) = ([true, true], None, 0.0f32, 0);
        for frame in 0..1200u32 {
            let ghoul = ghoul(&game).expect("the ghoul walks");
            let to = ghoul - drawn(&game);
            for (ahead, d) in ahead.iter_mut().zip([to.dot(along), to.dot(across)]) {
                if *ahead && d < -0.15 {
                    *ahead = false;
                } else if !*ahead && d > 0.15 {
                    *ahead = true;
                }
            }
            press_only(&mut game.client, &[if ahead[0] { Right } else { Left }, if ahead[1] { Up } else { Down }, Run]);
            game.frame();
            let mirror = game.client.world().resource::<WorldMirror>();
            for event in &mirror.events {
                if let sim::settlement::Event::ThreatSwing { agent: target, hit, .. } = *event {
                    if target == agent {
                        swings += 1;
                        if hit && hit_at.is_none() {
                            hit_at = Some(frame);
                            // Before the hit's tick (the push is in the newest).
                            let v = mirror.previous.as_ref().unwrap().agents[agent as usize].body.velocity;
                            speed = Vec2::new(v.x.to_f32(), v.y.to_f32()).length();
                        }
                    }
                }
            }
            if hit_at.is_some_and(|at| frame > at + 60) {
                break;
            }
        }
        let health = game.host.world().resource::<SessionService>().settlement().unwrap().agents[agent as usize].body.health;
        let telemetry = game.telemetry();
        let kinds: Vec<_> = telemetry.network.corrections().map(|c| format!("{:?} {:.3} u over {} frames{}", c.kind, c.distance, c.frames, if c.snapped { " snapped" } else { "" })).collect();
        let report = format!("{swings} swings at the client, first hit on frame {hit_at:?} at {speed:.2} m/s, health now {health}, corrections {kinds:?}");
        println!("{report}");
        assert!(hit_at.is_some(), "the ghoul never hit the client: {report}");
        assert!(speed > 1.0, "the avatar was running when hit: {report}");
        // How far the push moved the drawn (predicted) avatar from where it
        // was predicted without it: the knockback's distance. The offset it
        // leaves starts there and decays, so under 3 u it never snaps.
        let pushed: f32 = telemetry.network.corrections().filter(|c| c.kind == CorrectionKind::Knockback).map(|c| c.distance).sum();
        assert!(pushed > 0.05, "the push moved the running avatar measurably: {report}");
        assert!(telemetry.network.corrections().all(|c| c.kind == CorrectionKind::Knockback && !c.snapped && c.distance < 3.0), "only knockbacks, smoothed: {report}");
        let snapshot = telemetry.network.snapshot();
        assert_eq!((snapshot["preview_corrections"].as_u64(), snapshot["preview_mispredicted_frames"].as_u64()), (Some(0), Some(0)), "{report}");
    }

    /// The client's avatar as drawn this frame.
    fn drawn_avatar(app: &App) -> crate::prediction::DrawnState {
        app.world().resource::<crate::prediction::DrawnAvatar>().0.clone().expect("the avatar is drawn")
    }

    /// The client's avatar in its newest confirmed tick.
    fn confirmed_avatar(app: &App) -> sim::settlement::AgentState {
        let mirror = app.world().resource::<WorldMirror>();
        mirror.current.as_ref().unwrap().agents[mirror.player().unwrap() as usize].clone()
    }

    /// Withdraws the prediction (`prediction::WithdrawPrediction`): the
    /// confirmed avatar is drawn until the confirmed world reaches the
    /// input horizon, where a command sent now would apply.
    fn withdraw_prediction(app: &mut App) {
        let horizon = app.world().resource::<SessionService>().session.input_horizon().unwrap();
        app.world_mut().insert_resource(crate::prediction::WithdrawPrediction(horizon));
    }

    /// Lifts off in `scene` (the jetpack, or a jump), then once more and
    /// withdraws the prediction while the drawn avatar is up but the
    /// confirmed one isn't yet. The views' flight (`jetpack::Flight`: flame,
    /// thrust sound, landing sound, jump crouch) is the drawn avatar's on
    /// every frame, so it starts an input lead before the confirmed avatar
    /// lifts off and ends on the frame the prediction is withdrawn.
    fn assert_lift_follows_the_drawn_avatar(scene: &str, seed: u64) {
        let lifted = |a: &sim::settlement::avatar::AvatarState| a.flight.airborne || a.jump != sim::settlement::avatar::JumpPhase::Grounded;
        let mut game = RelayedGame::in_scene(scene, 100.0, 20.0, seed);
        game.hold(&[], 60);
        let thrust = |game: &RelayedGame| game.client.world().resource::<crate::sound::Sound>().recorded_events().iter().filter(|(_, cue)| *cue == crate::sound::Cue::Jetpack as i32).count();
        let space = |game: &mut RelayedGame, down: bool| {
            let mut keys = game.client.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            if down { keys.press(KeyCode::Space) } else { keys.release(KeyCode::Space) }
        };
        let jetpack = scene == "jetpack-lab";
        let mut report = String::new();
        for withdraw in [false, true] {
            space(&mut game, true);
            let (mut drawn_at, mut confirmed_at, mut withdrawn_at, mut thrust_early) = (None, None, None, 0);
            let thrust_before = thrust(&game);
            for frame in 0..90 {
                game.frame();
                // The test app has no input plugin to clear the press.
                game.client.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
                let drawn = drawn_avatar(&game.client);
                let flight = game.client.world().resource::<crate::jetpack::Flight>();
                let shown = (flight.airborne, flight.jump, flight.requested);
                let a = drawn.avatar();
                assert_eq!(shown, (a.flight.airborne, a.jump, a.flight.requested), "{scene}, frame {frame}: the views' flight isn't the drawn avatar's");
                let up = lifted(a);
                let confirmed_up = lifted(&confirmed_avatar(&game.client).avatar().unwrap());
                if up && drawn_at.is_none() {
                    drawn_at = Some(frame);
                    if withdraw {
                        withdraw_prediction(&mut game.client);
                    }
                }
                if withdraw && drawn_at.is_some() && !up && !confirmed_up && withdrawn_at.is_none() {
                    withdrawn_at = Some(frame);
                }
                if confirmed_up && confirmed_at.is_none() {
                    confirmed_at = Some(frame);
                    thrust_early = thrust(&game) - thrust_before;
                }
            }
            report += &format!("{scene}{}: drawn up on frame {drawn_at:?}, confirmed on {confirmed_at:?}, withdrawn on {withdrawn_at:?}, thrust sounds before confirmed {thrust_early}\n", if withdraw { " (withdrawn)" } else { "" });
            let (Some(drawn_at), Some(confirmed_at)) = (drawn_at, confirmed_at) else { panic!("never lifted off:\n{report}") };
            assert!(confirmed_at >= drawn_at + 4, "the views lifted off with the confirmed avatar, not the drawn one:\n{report}");
            if withdraw {
                assert!(withdrawn_at.is_some_and(|at| at < confirmed_at), "the withdrawn lift went on being shown:\n{report}");
            } else if jetpack {
                assert!(thrust_early > 0, "the thrust sound waited for the confirmed avatar:\n{report}");
            }
            space(&mut game, false);
            game.frame();
            game.client.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
            game.hold(&[], 150);
        }
        println!("{report}");
    }

    #[test]
    fn joined_player_jetpack_follows_the_drawn_avatar() {
        assert_lift_follows_the_drawn_avatar("jetpack-lab", 0xf1a7);
    }

    #[test]
    fn joined_player_jump_follows_the_drawn_avatar() {
        assert_lift_follows_the_drawn_avatar("moonlit-glade", 0x7a3d);
    }

    /// Digs in the dig lab: the local dig's bar, dust and pose
    /// (`digging::shown_digs`, `DrawnState::dig_age`) are the drawn
    /// avatar's, an input lead before the confirmed avatar digs, and end on
    /// the frame the prediction is withdrawn. A sword swing pressed while
    /// the drawn avatar digs, which the simulation refuses, ends as soon as
    /// the drawn avatar doesn't swing for it.
    #[test]
    fn joined_player_dig_follows_the_drawn_avatar() {
        let mut game = RelayedGame::in_scene("dig-lab", 100.0, 20.0, 0xd16);
        game.hold(&[], 60);
        let mut report = String::new();
        for withdraw in [false, true] {
            game.client.world_mut().resource_mut::<SessionService>().dig();
            let (mut drawn_at, mut confirmed_at, mut withdrawn_at) = (None, None, None);
            for frame in 0..60 {
                game.frame();
                let drawn = drawn_avatar(&game.client);
                let world = game.client.world_mut();
                let player = world.query_filtered::<&Transform, With<Player>>().single(world).unwrap().translation;
                let mirror = world.resource::<WorldMirror>();
                let shown = crate::digging::shown_digs(mirror, Some(&drawn), player);
                let mine: Vec<_> = shown.iter().filter(|d| d.agent == drawn.agent).collect();
                assert!(mine.len() <= 1);
                let digging = drawn.avatar().dig.clone();
                assert_eq!(mine.first().map(|d| (d.dig.clone(), d.at, d.now)), digging.clone().map(|dig| (dig, player, drawn.time)), "frame {frame}: the shown dig isn't the drawn avatar's");
                assert_eq!(drawn.dig_age().is_some(), digging.is_some());
                let confirmed_digging = confirmed_avatar(&game.client).avatar().unwrap().dig.is_some();
                if digging.is_some() && drawn_at.is_none() {
                    drawn_at = Some(frame);
                    if withdraw {
                        withdraw_prediction(&mut game.client);
                    }
                }
                if withdraw && drawn_at.is_some() && digging.is_none() && !confirmed_digging && withdrawn_at.is_none() {
                    withdrawn_at = Some(frame);
                }
                if confirmed_digging && confirmed_at.is_none() {
                    confirmed_at = Some(frame);
                }
            }
            report += &format!("dig{}: drawn on frame {drawn_at:?}, confirmed on {confirmed_at:?}, withdrawn on {withdrawn_at:?}\n", if withdraw { " (withdrawn)" } else { "" });
            let (Some(drawn_at), Some(confirmed_at)) = (drawn_at, confirmed_at) else { panic!("never dug:\n{report}") };
            assert!(confirmed_at >= drawn_at + 4, "the dig was shown with the confirmed avatar, not the drawn one:\n{report}");
            if withdraw {
                assert!(withdrawn_at.is_some_and(|at| at < confirmed_at), "the withdrawn dig went on being shown:\n{report}");
            } else {
                // Still digging: a sword swing is refused.
                assert!(drawn_avatar(&game.client).avatar().dig.is_some());
                let world = game.client.world_mut();
                world.resource_mut::<Controls>().weapon = content::scenario::Weapon::Sword;
                world.resource_scope(|world, mut service: Mut<SessionService>| {
                    world.resource_scope(|world, mut controls: Mut<Controls>| {
                        world.resource_scope(|world, mut control: Mut<PlayerControl>| {
                            control.strike(&mut service, &mut controls, &world.resource::<SceneView>().pack, Vec3::X);
                        });
                    });
                });
                let mut ended = None;
                for frame in 0..30 {
                    game.frame();
                    assert!(drawn_avatar(&game.client).avatar().swing.is_none(), "the simulation swung while digging");
                    if game.client.world().resource::<Controls>().attack.is_none() {
                        ended = Some(frame);
                        break;
                    }
                }
                report += &format!("refused swing ended after {ended:?} frames\n");
                assert!(ended.is_some_and(|frames| (frames as f32) < crate::combat::attack_end() * 60.0 / 2.0), "the refused swing played on:\n{report}");
            }
            game.hold(&[], 150);
        }
        println!("{report}");
    }
}

//! The game's simulation: every scene runs on the deterministic lockstep
//! simulation (`game/sim`) through a session (`game/session`). Three services
//! (see game/docs/proposals/service_layers.md) split the work:
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
use crate::gameplay::{Controls, Player};
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
}

impl Service for SessionService {
    type Layer = SessionLayer;
    type Deps = (Flow,);

    fn init(world: &mut World) -> Self {
        let Opened { session, content, network, name, record, start_save, replay_scene } =
            world.resource_mut::<Flow>().take_opened().expect("a session starts once Flow has opened it");
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
        }
    }

    fn shutdown(mut self, _world: &mut World) {
        self.write_recording();
        self.session.shutdown();
    }

    /// A client's session is ready once the host has welcomed it.
    fn ready(&self, _world: &World) -> bool {
        self.session.world_number() > 0 || self.ended.is_some()
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
    let config = NetConfig { threads, input_delay, ..NetConfig::default() };
    let scene_id = world.resource::<SceneLibrary>().scenes[launch.scene].scene.id.clone();
    let mut start_save = None;
    let mut replay_scene = None;
    let mode = match (launch.replay, launch.network) {
        (Some((bytes, log)), _) => {
            let replayed = save::read(&bytes, &content).map_err(|e| format!("The recording's save doesn't load: {e}"))?;
            replay_scene = Some(scene_id);
            session::Mode::Replay { world: replayed, log }
        }
        (None, Some(Network::Join(host))) => session::Mode::Join { host, stable_id: stable_id() },
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

    /// The local player's avatar ID, as views name agents.
    pub fn player_id(&self) -> Option<String> {
        Some(self.settlement()?.agents[self.player()? as usize].id.clone())
    }

    /// Sends a command; it applies on the next tick in single-player, and
    /// after the input delay in a networked game.
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
        SimWorld::new_scene(SEED, content.rules.clone(), &setup).unwrap_or_else(|e| panic!("scene {id} does not build a world: {e}"))
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
        extent: q(crate::assets::extent(pack)),
        obstacles: found.discs.iter().map(disc).collect(),
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

/// The digit keys ask to move everyone to a scene (a host's request; see
/// `SessionService::request_scene`).
fn scene_keys(keys: Res<ButtonInput<KeyCode>>, library: Res<SceneLibrary>, mut service: ResMut<SessionService>) {
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
    let Some(index) = KEYS.iter().position(|&key| keys.just_pressed(key)) else { return };
    let current = service.scene_label().and_then(|label| library.index(label).ok());
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
    let mut ticks = Vec::new();
    let started = std::time::Instant::now();
    service.session.update(clock, |world| {
        let copying = std::time::Instant::now();
        let settlement = world.settlement().cloned();
        ticks.push((world.tick(), settlement, world.stage_times(), copying.elapsed()));
    });
    telemetry.session_frame(started.elapsed(), ticks.iter().map(|(_, _, times, copy)| (*times, *copy)));
    service.frame = ticks.into_iter().map(|(tick, settlement, _, _)| (tick, settlement)).collect();
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

/// Moves the World layer with the session's world: off while a world is
/// replaced (for a frame, so its services stop and its entities go), loading
/// once a new one is in the slot. Its services decide when it is active.
fn watch_world(mut service: ResMut<SessionService>, state: Option<Res<State<WorldState>>>, next: Option<ResMut<NextState<WorldState>>>) {
    let (Some(state), Some(mut next)) = (state, next) else { return };
    let number = service.session.world_number();
    match state.get() {
        WorldState::Off if number > 0 => {
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

/// Tells this machine apart from others when joining. A random ID per run:
/// a rejoining player gets a new avatar.
fn stable_id() -> net::StableId {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    nanos ^ (u64::from(std::process::id()) << 32)
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
}

impl Service for PlayerControl {
    type Layer = WorldLayer;
    type Deps = (SessionService, WorldMirror, SceneView);

    fn init(_world: &mut World) -> Self {
        PlayerControl::default()
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(
            Phase::Input,
            (
                (mouse_input, attack_input).chain().run_if(crate::control::interactive),
                keyboard_input,
                crate::preview::playback.run_if(crate::control::interactive),
            )
                .chain()
                .run_if(in_state(WorldState::Active)),
        );
    }
}

impl PlayerControl {
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
        self.walking = None;
        service.command(CommandKind::Attack {
            agent,
            weapon,
            aim: FxVec2::new(Fx::quantize(flat.x), Fx::quantize(flat.z)),
            muzzle: muzzle.map(Fx::quantize),
        });
        controls.strike(aim);
    }
}

/// Places the player where the simulation has the avatar, between ticks.
pub fn follow_avatar(
    mirror: Res<WorldMirror>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    sound: Res<crate::sound::Sound>,
) {
    let (Some(current), Some(index)) = (&mirror.current, mirror.player()) else { return };
    let a = &current.agents[index as usize];
    let before = mirror.previous.as_ref().and_then(|p| p.agents.get(index as usize)).unwrap_or(a);
    let alpha = mirror.alpha;
    let from = mirror.to_view(before.position);
    let to = mirror.to_view(a.position);
    // A teleport (respawn, deploying a recording) snaps instead of sliding.
    let alpha = if from.distance(to) > 1.5 { 1.0 } else { alpha };
    let mut position = from.lerp(to, alpha);
    position.y = before.avatar.height.to_f32() + (a.avatar.height - before.avatar.height).to_f32() * alpha;
    let previous = player.translation;
    let teleport = previous.distance(position) > 1.5;
    player.translation = position;
    let heading = before.heading.to_f32() + angle_between(before.heading.to_f32(), a.heading.to_f32()) * alpha;
    player.rotation = Quat::from_rotation_y(heading);
    let moved = if teleport { Vec3::ZERO } else { crate::navigation::flat(position - previous) };
    controls.velocity = Vec3::new(a.avatar.velocity.x.to_f32(), 0.0, a.avatar.velocity.y.to_f32());
    controls.stride += moved.length() * 8.5;
    if a.avatar.flight.airborne {
        controls.foot_distance = 0.0;
    } else {
        controls.foot_distance += moved.length();
        if controls.foot_distance > 0.72 {
            controls.foot_distance -= 0.72;
            sound.play(crate::sound::Cue::Step);
        }
    }
    controls.route = a.route.iter().map(|&p| mirror.to_view(p)).collect();
}

fn angle_between(from: f32, to: f32) -> f32 {
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

/// Keyboard movement: camera-relative walking, the jetpack, digging, and respawning.
pub fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut controls: ResMut<Controls>,
    mut service: ResMut<SessionService>,
    mut control: ResMut<PlayerControl>,
) {
    let Some(agent) = service.player() else { return };
    if keys.just_pressed(KeyCode::KeyR) {
        service.command(CommandKind::Respawn { agent });
        let attack_serial = controls.attack_serial;
        *controls = Controls { attack_serial, ..default() };
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        service.command(CommandKind::Jetpack { agent, enabled: true });
    }
    if keys.just_released(KeyCode::Space) {
        service.command(CommandKind::Jetpack { agent, enabled: false });
    }
    // X picks the spear in combat scenes, so digging is F.
    if keys.just_pressed(KeyCode::KeyF) {
        service.dig();
    }
    let pressed = |a, b| if keys.pressed(a) || keys.pressed(b) { 1.0 } else { 0.0 };
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = Vec3::new(-1.0, 0.0, -1.0).normalize();
    let input = Vec2::new(
        pressed(KeyCode::KeyD, KeyCode::ArrowRight) - pressed(KeyCode::KeyA, KeyCode::ArrowLeft),
        pressed(KeyCode::KeyW, KeyCode::ArrowUp) - pressed(KeyCode::KeyS, KeyCode::ArrowDown),
    )
    .normalize_or_zero();
    let run = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let direction = right * input.x + up * input.y;
    if input != Vec2::ZERO || control.walking.is_some_and(|(d, _)| d != FxVec2::ZERO) {
        control.walk(&mut service, Vec2::new(direction.x, direction.z), run);
    }
}

/// Right click (held) walks to the cursor.
pub fn mouse_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    scene: Res<SceneView>,
    mut controls: ResMut<Controls>,
    mut service: ResMut<SessionService>,
    mut control: ResMut<PlayerControl>,
) {
    let run = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    controls.repath -= time.delta_secs();
    if buttons.pressed(MouseButton::Right) && controls.attack.is_none() && controls.repath <= 0.0 {
        controls.repath = 0.12;
        if let Some(point) = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &scene, &service) {
            if controls.route.last().is_none_or(|last| crate::navigation::flat(*last - point).length() > 0.25) {
                control.go_to(&mut service, point, run);
            }
        }
    }
}

/// Shift + left click attacks toward the cursor; Z/X/C/V pick a weapon.
pub fn attack_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    scene: Res<SceneView>,
    player: Single<&Transform, With<Player>>,
    mut controls: ResMut<Controls>,
    mut service: ResMut<SessionService>,
    mut control: ResMut<PlayerControl>,
) {
    use content::scenario::Weapon;
    if let Some(scenario) = scene.scene().scenario.as_ref() {
        for (key, weapon) in [(KeyCode::KeyZ, Weapon::Sword), (KeyCode::KeyX, Weapon::Spear), (KeyCode::KeyC, Weapon::Bow), (KeyCode::KeyV, Weapon::ArcCoil)] {
            if keys.just_pressed(key) && scenario.weapons.contains(&weapon) {
                controls.weapon = weapon;
                service.equip(weapon);
            }
        }
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !(shift && buttons.just_pressed(MouseButton::Left)) {
        return;
    }
    if let Some(point) = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &scene, &service) {
        control.strike(&mut service, &mut controls, &scene.pack, point - player.translation);
    }
}

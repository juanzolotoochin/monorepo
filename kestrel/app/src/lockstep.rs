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
        Some(self.to_view(self.settlement()?.agents[self.player()? as usize].position))
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
            disc: sim::terrain::Disc {
                center: FxVec2::new(q(piece.center[0]), q(piece.center[2])),
                radius: q(piece.radius),
            },
            health: piece.health,
        }).collect(),
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
    let mut ticks = Vec::new();
    let started = web_time::Instant::now();
    service.session.update(clock, |world| {
        let copying = web_time::Instant::now();
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

/// Tells this machine apart from others when joining. A random ID per run:
/// a rejoining player gets a new avatar.
fn stable_id() -> net::StableId {
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
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
    /// A local click target while the host has not applied its command yet.
    visual_goal: Option<(Vec3, bool)>,
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

    /// Forgets the cached movement and click goal after direct control was
    /// released, so the next deliberate movement is always sent.
    pub fn forget_direct_input(&mut self) {
        self.walking = None;
        self.visual_goal = None;
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
        self.visual_goal = None;
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
        self.visual_goal = Some((target, run));
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
            self.visual_goal = None;
        }
        service.command(CommandKind::Attack {
            agent,
            weapon,
            aim: FxVec2::new(Fx::quantize(flat.x), Fx::quantize(flat.z)),
            muzzle: muzzle.map(Fx::quantize),
        });
        controls.strike(aim);
    }
}

/// Where a joined player's avatar shows while the host hasn't confirmed its
/// latest input: walked ahead locally, then eased back onto confirmed ticks.
#[derive(Default)]
pub struct LocalAvatarPreview {
    position: Option<Vec3>,
    velocity: Vec3,
    active: bool,
    agent: Option<u16>,
}

#[derive(Default)]
pub struct VisualGroundStep {
    height: Option<f32>,
    agent: Option<u16>,
    scene: Option<usize>,
}

/// Places the player where the simulation has the avatar, between ticks.
/// `PlayerControl` is a World service registered after this one's; its input
/// state exists whenever the world is active.
pub fn follow_avatar(
    mirror: Res<WorldMirror>,
    service: Res<SessionService>,
    control: Res<PlayerControl>,
    scene: Res<SceneView>,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    sound: Res<crate::sound::Sound>,
    mut preview: Local<LocalAvatarPreview>,
    mut ground_step: Local<VisualGroundStep>,
) {
    let (Some(current), Some(index)) = (&mirror.current, mirror.player()) else { return };
    let a = &current.agents[index as usize];
    let before = mirror.previous.as_ref().and_then(|p| p.agents.get(index as usize)).unwrap_or(a);
    // The local single-player avatar can use the newest confirmed pose.
    // Interpolating from the previous tick adds a full tick of visible lag.
    let alpha = if !service.is_client() && !service.session.is_replay() { 1.0 } else { mirror.alpha };
    let from = mirror.to_view(before.position);
    let to = mirror.to_view(a.position);
    // A teleport (respawn, deploying a recording) snaps instead of sliding.
    let alpha = if from.distance(to) > 1.5 { 1.0 } else { alpha };
    let mut position = from.lerp(to, alpha);
    position.y = before.avatar.height.to_f32() + (a.avatar.height - before.avatar.height).to_f32() * alpha;
    let reset_step = ground_step.agent != Some(index)
        || ground_step.scene != Some(scene.index)
        || from.distance(to) > 1.5;
    position.y = crate::character::visual_step_height(
        if reset_step { None } else { ground_step.height },
        position.y,
        time.delta_secs(),
        !a.avatar.flight.airborne
            && matches!(a.avatar.jump, sim::settlement::avatar::JumpPhase::Grounded),
    );
    ground_step.height = Some(position.y);
    ground_step.agent = Some(index);
    ground_step.scene = Some(scene.index);
    let motion = crate::character::motion_profile(&scene.pack);
    let mut preview_velocity = None;
    if service.is_client() {
        let reset = preview.agent != Some(index) || preview.position.is_none_or(|p| p.distance(to) > 1.5);
        if reset {
            *preview = LocalAvatarPreview { position: Some(to), agent: Some(index), ..default() };
        }
        let pending_goal = control.visual_goal.filter(|_| service.session.pending_inputs() > 0);
        let movement = control
            .walking
            .map(|(direction, run)| (Vec3::new(direction.x.to_f32(), 0.0, direction.y.to_f32()), run))
            .or_else(|| {
                pending_goal.map(|(goal, run)| {
                    let delta = crate::navigation::flat(goal - preview.position.unwrap());
                    (if delta.length() < 0.1 { Vec3::ZERO } else { delta.normalize() }, run)
                })
            })
            .filter(|_| (controls.attack.is_none() || controls.weapon == content::scenario::Weapon::Laser) && a.avatar.dig.is_none());
        let moving_input = movement.is_some_and(|(direction, _)| direction != Vec3::ZERO);
        if let Some((direction, run)) = movement {
            preview.active = true;
            let dt = time.delta_secs().min(0.05);
            let desired = direction * if run { motion.run_speed } else { motion.walk_speed };
            // The definition stores per-tick changes; the preview runs per second.
            let tuning = service.player_character().and_then(|c| c.avatar.as_ref());
            let per_tick = tuning.map_or(Fx::ZERO, |t| if desired == Vec3::ZERO { t.braking } else { t.acceleration });
            let acceleration = view_f32(per_tick) * content::TICKS_PER_SECOND as f32;
            let change = (desired - preview.velocity).clamp_length_max(acceleration * dt);
            preview.velocity += change;
            let start = preview.position.unwrap();
            let candidate = start + preview.velocity * dt;
            let passable = service.world().is_some_and(|world| {
                a.avatar.flight.airborne
                    || service.player_radius().is_some_and(|radius| world.terrain().walkable(service.to_sim(start), service.to_sim(candidate), radius))
            });
            if passable {
                // Limit how far the displayed avatar can get ahead while
                // waiting for the host's confirmed turns.
                preview.position = Some(to + (candidate - to).clamp_length_max(0.9));
            } else {
                preview.velocity = Vec3::ZERO;
            }
            preview_velocity = Some(preview.velocity);
        } else {
            preview.velocity = Vec3::ZERO;
            preview_velocity = Some(Vec3::ZERO);
            if preview.active {
                let blend = 1.0 - (-8.0 * time.delta_secs()).exp();
                preview.position = Some(preview.position.unwrap().lerp(to, blend));
            }
        }
        if preview.active && !moving_input && service.session.pending_inputs() == 0 && a.avatar.velocity == FxVec2::ZERO {
            let remaining = to - preview.position.unwrap();
            preview.position = Some(preview.position.unwrap() + remaining.clamp_length_max(2.0 * time.delta_secs()));
        }
        if preview.active && preview.position.unwrap().distance(to) < 0.02 && !moving_input {
            preview.active = false;
        }
        if preview.active {
            position.x = preview.position.unwrap().x;
            position.z = preview.position.unwrap().z;
        } else {
            preview.position = Some(to);
        }
    } else {
        *preview = LocalAvatarPreview::default();
    }
    let previous = player.translation;
    let teleport = previous.distance(position) > 1.5;
    player.translation = position;
    let heading = before.heading.to_f32() + angle_between(before.heading.to_f32(), a.heading.to_f32()) * alpha;
    player.rotation = Quat::from_rotation_y(heading);
    if let Some(velocity) = preview_velocity.filter(|v| v.length_squared() > 0.01) {
        player.rotation = Quat::from_rotation_y(velocity.x.atan2(velocity.z));
    }
    let moved = if teleport { Vec3::ZERO } else { crate::navigation::flat(position - previous) };
    controls.velocity = Vec3::new(a.avatar.velocity.x.to_f32(), 0.0, a.avatar.velocity.y.to_f32());
    if let Some(velocity) = preview_velocity {
        controls.velocity = velocity;
    }
    let relative_speed = controls.velocity.length() / motion.walk_speed.max(0.1);
    let step_distance = crate::character::step_distance_for_speed(motion.step_distance, relative_speed).clamp(0.2, 4.0);
    let contacts = controls.gait.advance(moved.length(), step_distance, !a.avatar.flight.airborne);
    for _ in 0..contacts {
        sound.play(crate::sound::Cue::Step);
    }
    controls.route = a.route.iter().map(|&p| mirror.to_view(p)).collect();
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
) {
    let (keys, buttons) = (input.keys(), input.mouse());
    use content::scenario::Weapon;
    // Update on every input frame, including frames without a shot.
    let cursor_aim = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &scene, &service)
        .zip(service.player_view_position())
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
    // Aim from the confirmed position: a joined player's preview must not
    // change gameplay commands.
    if let Some(aim) = cursor_aim {
        control.strike(&mut service, &mut controls, &scene.pack, aim);
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
        mirror.to_view(mirror.current.as_ref().unwrap().agents[index].position)
    }

    /// An app on the field at a fixed frame rate, launching straight into a
    /// session.
    fn field_app(network: Option<Network>, fps: f64) -> App {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let library = SceneLibrary::new();
        let field = library.index("field").unwrap();
        let mut app = crate::test_app(options, library, hub, Launch { network, ..Launch::single_player(field) });
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
                .map(|previous| mirror.to_view(previous.agents[index as usize].position))
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
        let route_preview = player_position(&mut client);
        assert!(
            crate::navigation::flat(route_preview - last).length() > 0.001,
            "routed movement gave no immediate visual response"
        );
        assert!(
            crate::navigation::flat(confirmed(&client) - settled).length() < 0.001,
            "routed movement did not show prediction before confirmation"
        );
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
}

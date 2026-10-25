//! The game's simulation: every scene runs on the deterministic lockstep
//! simulation (`kestrel/sim`) through a session (`kestrel/session`). This module
//! builds each scene's world, steps it on Bevy's clock, and turns input into
//! commands. Views read the mirrors it fills (`legacy`, piles, memory,
//! flight); nothing moves gameplay state except commands.
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
use save::InputLog;
use sim::settlement::avatar::Intent;
use sim::settlement::{PileSetup, SceneSetup, Settlement as SimSettlement};
use sim::{CommandKind, World as SimWorld};

use crate::assets::SceneLibrary;
use crate::character::Player;
use crate::combat::Obstacle;
use crate::gameplay::Controls;

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

#[derive(Resource)]
pub struct Game {
    pub session: Option<session::Session>,
    content: Content,
    /// The scene the session belongs to.
    scene: Option<usize>,
    /// Simulation time: Bevy's time, minus pauses, times the rate.
    clock: Duration,
    pub paused: bool,
    pub rate: f32,
    /// The last two ticks' settlements, for interpolation.
    pub previous: Option<SimSettlement>,
    pub current: Option<SimSettlement>,
    /// Every event since the last frame.
    pub events: Vec<sim::settlement::Event>,
    pub tick: u64,
    pub alpha: f32,
    /// The arena center in world tiles; views work in arena coordinates.
    pub center: FxVec2,
    /// The last movement intent sent, so held keys don't resend it every frame.
    walking: Option<(FxVec2, bool)>,
    /// A local click target while the host has not applied its command yet.
    visual_goal: Option<(Vec3, bool)>,
    /// The tick on which the newest command applies.
    pub applies_at: u64,
    /// Where to write the session's tick-0 save and input log on exit.
    pub record: Option<String>,
    /// A recording to play back instead of live input.
    pub replay: Option<(Vec<u8>, InputLog)>,
    start_save: Option<Vec<u8>>,
    /// The control hub held the clock last frame.
    warming_up: bool,
    pub network: Option<Network>,
    /// The connection, for the HUD.
    pub status: String,
    /// What other players see over this player's avatar.
    pub name: String,
    /// The name was sent for this session's avatar.
    named: bool,
    /// Why a networked game ended for this player, if it did: the game
    /// closes shortly after (see `end_session`).
    pub ended: Option<String>,
}

impl Default for Game {
    fn default() -> Self {
        Game {
            session: None,
            content: Content::builtin(),
            scene: None,
            clock: Duration::ZERO,
            paused: false,
            rate: 1.0,
            previous: None,
            current: None,
            events: Vec::new(),
            tick: 0,
            alpha: 0.0,
            center: FxVec2::ZERO,
            walking: None,
            visual_goal: None,
            applies_at: 0,
            record: None,
            replay: None,
            start_save: None,
            warming_up: false,
            network: None,
            status: String::new(),
            name: String::new(),
            named: false,
            ended: None,
        }
    }
}

impl Game {
    pub fn world(&self) -> Option<&SimWorld> {
        self.session.as_ref().and_then(|s| s.world())
    }

    pub fn settlement(&self) -> Option<&SimSettlement> {
        self.world().and_then(SimWorld::settlement)
    }

    /// The local player's avatar index. Replays watch the host's.
    pub fn player(&self) -> Option<u16> {
        let me = self.session.as_ref()?.player().unwrap_or(net::HOST_PLAYER);
        self.settlement()?.avatar_of(me)
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
        let Some(session) = self.session.as_mut() else { return };
        let tick = session.world().map_or(0, |w| w.tick());
        self.applies_at = tick + session.input_delay();
        session.queue(kind);
    }

    /// A hash of the current state, to tell runs apart in traces.
    pub fn fingerprint(&self) -> String {
        self.current.as_ref().map_or_else(String::new, |s| format!("{:016x}", sim::hash::hash_of(s)))
    }

    /// Whether every command sent so far has been applied.
    pub fn caught_up(&self) -> bool {
        let pending = self.session.as_ref().map_or(0, session::Session::pending_inputs);
        self.world().is_some_and(|w| w.tick() > self.applies_at) && pending == 0
    }

    /// Arena coordinates (meters from the center, Bevy x/z) to world tiles.
    pub fn to_sim(&self, p: Vec3) -> FxVec2 {
        FxVec2::new(self.center.x + Fx::quantize(p.x), self.center.y + Fx::quantize(p.z))
    }

    /// World tiles to arena coordinates on the ground.
    pub fn to_view(&self, p: FxVec2) -> Vec3 {
        Vec3::new((p.x - self.center.x).to_f32(), 0.0, (p.y - self.center.y).to_f32())
    }

    /// Asks the avatar to walk in a direction (camera-relative, already
    /// rotated into arena space); a zero direction stops it.
    pub fn walk(&mut self, direction: Vec2, run: bool) {
        let Some(agent) = self.player() else { return };
        self.visual_goal = None;
        let direction = FxVec2::new(Fx::quantize(direction.x), Fx::quantize(direction.y)).normalize_or_zero();
        if self.walking == Some((direction, run)) {
            return;
        }
        self.walking = Some((direction, run));
        let intent = if direction == FxVec2::ZERO { Intent::Stop } else { Intent::Walk { direction, run } };
        self.command(CommandKind::MoveAvatar { agent, intent });
    }

    /// Asks the avatar to walk to a point (arena coordinates).
    pub fn go_to(&mut self, target: Vec3, run: bool) {
        let Some(agent) = self.player() else { return };
        self.walking = None;
        self.visual_goal = Some((target, run));
        let target = self.to_sim(target);
        self.command(CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run } });
    }

    /// Writes the recording, if asked to: `<path>.gsav` is the session's
    /// tick-0 save and `<path>` its input log.
    pub fn write_recording(&self) {
        let (Some(path), Some(save), Some(log)) =
            (&self.record, &self.start_save, self.session.as_ref().and_then(session::Session::input_log))
        else {
            return;
        };
        let written = std::fs::write(format!("{path}.gsav"), save).and_then(|()| std::fs::write(path, log.to_bytes()));
        if let Err(e) = written {
            error!("could not write the recording {path}: {e}");
        }
    }
}

/// Builds the scene's world once its entities (and obstacles) exist.
pub fn build_world(world: &mut World) {
    let index = world.resource::<SceneLibrary>().current;
    if world.resource::<Game>().scene == Some(index) || index == usize::MAX {
        return;
    }
    let mut doors = world.query_filtered::<&Obstacle, With<crate::palimpsest::Door>>();
    let door_discs: Vec<_> = doors.iter(world).map(|o| o.0).collect();
    let mut obstacles = world.query_filtered::<&Obstacle, Without<crate::palimpsest::Door>>();
    let disc = |d: &crate::navigation::Disc| sim::terrain::Disc {
        center: FxVec2::new(Fx::quantize(d.center.x), Fx::quantize(d.center.z)),
        radius: Fx::quantize(d.radius),
    };
    let obstacle_discs: Vec<_> = obstacles.iter(world).map(|o| disc(&o.0)).collect();
    let library = world.resource::<SceneLibrary>();
    let pack = &library.scenes[index];
    let scene = &pack.scene;
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
    let setup = SceneSetup {
        scenario: scene.scenario.clone(),
        extent: q(library.extent()),
        obstacles: obstacle_discs,
        doors: door_discs.iter().map(disc).collect(),
        surfaces,
        platforms_only,
        spawn: scene.spawn.map(q),
        piles: scene.piles.as_ref().map(|p| PileSetup {
            capacity: p.capacity,
            dispensers: p.dispensers.iter().map(|d| (material(d.material), [q(d.output[0]), q(d.output[2])])).collect(),
            piles: p.piles.iter().map(|s| (material(s.material), [q(s.position[0]), q(s.position[2])], s.units)).collect(),
            collector: p.collector.map(|c| [q(c[0]), q(c[2])]),
        }),
        memory_trial: library.has_feature(voxel_assets::SceneFeature::MemoryTrial),
        diggable: library.has_feature(voxel_assets::SceneFeature::Digging),
        jump_enabled: library.has_feature(voxel_assets::SceneFeature::Jump),
        locomotion: scene.style.as_ref().and_then(|style| {
            let motion = style.character_motion;
            let defaults = voxel_assets::CharacterMotion::default();
            (motion.walk_speed != defaults.walk_speed || motion.run_speed != defaults.run_speed)
                .then_some(sim::terrain::Locomotion {
                    walk: q(motion.walk_speed),
                    run: q(motion.run_speed),
                })
        }),
        swift_sword: library.has_feature(voxel_assets::SceneFeature::SwiftSword),
        mine: scene.mine.as_ref().map(|m| m.seed),
    };
    let scene_id = scene.id.clone();
    let mut game = world.resource_mut::<Game>();
    let content = game.content.clone();
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    // Single-player has nobody to wait for, so no input delay.
    let input_delay = if game.network.is_some() { NetConfig::default().input_delay } else { 0 };
    let config = NetConfig { threads, input_delay, ..NetConfig::default() };
    let built = SimWorld::new_scene(SEED, content.rules.clone(), &setup);
    let world_state = match built {
        Ok(world) => world,
        Err(e) => panic!("scene {scene_id} does not build a world: {e}"),
    };
    game.center = world_state.terrain().center;
    let start = save::write(&world_state, &content);
    let fresh = || save::read(&start, &content).expect("a fresh save loads");
    let mode = match (game.replay.take(), game.network) {
        (Some((bytes, log)), _) => {
            let replayed = save::read(&bytes, &content).expect("the recording's save loads");
            session::Mode::Replay { world: replayed, log }
        }
        // A client's world comes from the host; the scene built here is
        // only for the view.
        (None, Some(Network::Join(host))) => session::Mode::Join { host, stable_id: stable_id() },
        (None, Some(Network::Host(listen))) => session::Mode::Host { world: fresh(), listen: Some(listen) },
        (None, None) => session::Mode::Host { world: fresh(), listen: None },
    };
    let session = session::Session::start(mode, content, config).unwrap_or_else(|e| panic!("could not start the session: {e}"));
    if let Some(addr) = session.local_addr() {
        info!("hosting on {addr}");
    }
    game.start_save = Some(start);
    game.session = Some(session);
    game.scene = Some(index);
    game.clock = Duration::ZERO;
    game.previous = None;
    game.current = game.settlement().cloned();
    game.tick = 0;
    game.walking = None;
    game.visual_goal = None;
    game.named = false;
    game.applies_at = 0;
    game.events.clear();
}

/// Runs every tick due by this frame's time and keeps the last two for interpolation.
pub fn step(
    time: Res<Time>,
    hub: Option<Res<crate::control::Hub>>,
    mut game: ResMut<Game>,
    mut telemetry: ResMut<crate::telemetry::Telemetry>,
) {
    // Scripted runs warm up for different lengths with and without rendering;
    // holding the clock until then lines up ticks with script frames, so a
    // rendered replay of a headless recording matches it command for command.
    if hub.is_some_and(|hub| hub.warming_up()) {
        game.warming_up = true;
        return;
    }
    if std::mem::take(&mut game.warming_up) {
        // The first tick runs on the first frame after warmup, as it does
        // without one (Bevy's first frame has no delta).
        game.clock = Duration::ZERO;
    } else if game.network.is_some() {
        // Everyone shares one clock: pausing or speeding up alone would
        // only fall behind or run out of turns.
        game.clock += time.delta();
    } else if !game.paused {
        let rate = game.rate;
        game.clock += time.delta().mul_f32(rate);
    }
    let clock = game.clock;
    let Some(mut session) = game.session.take() else { return };
    let mut ticks = Vec::new();
    let started = std::time::Instant::now();
    session.update(clock, |world| {
        let copying = std::time::Instant::now();
        let settlement = world.settlement().cloned();
        ticks.push((world.tick(), settlement, world.stage_times(), copying.elapsed()));
    });
    telemetry.session_frame(started.elapsed(), ticks.iter().map(|(_, _, times, copy)| (*times, *copy)));
    for (tick, settlement, _, _) in ticks {
        if let Some(s) = &settlement {
            game.events.extend(s.events.iter().cloned());
        }
        game.previous = std::mem::replace(&mut game.current, settlement);
        game.tick = tick.saturating_sub(1);
    }
    game.alpha = session.alpha(clock);
    let client = matches!(game.network, Some(Network::Join(_)));
    for event in session.events.drain(..) {
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
                game.ended.get_or_insert(reason);
            }
            None => info!("{event:?}"),
        }
    }
    if let Some(world) = session.world() {
        // A client learns where the arena is once the host's save loads.
        game.center = world.terrain().center;
    }
    if game.network.is_some() {
        game.status = session.status();
    }
    game.session = Some(session);
    // Introduce ourselves once our avatar exists (a client's appears after
    // it joins).
    if game.network.is_some() && !game.named && game.player().is_some() {
        let name = game.name.clone();
        game.command(CommandKind::Name { name });
        game.named = true;
    }
}

/// How long the ending banner shows before the game closes.
const CLOSING_SECONDS: f32 = 4.0;

#[derive(Component)]
pub struct EndBanner;

/// A networked game ended for this player: says why, counts down, and closes.
pub fn end_session(
    mut commands: Commands,
    time: Res<Time<Real>>,
    game: Res<Game>,
    mut banner: Query<&mut Text, With<EndBanner>>,
    mut exit: MessageWriter<AppExit>,
    mut left: Local<Option<f32>>,
) {
    let Some(reason) = &game.ended else { return };
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

#[derive(Default)]
pub struct LocalAvatarPreview {
    position: Option<Vec3>,
    velocity: Vec3,
    active: bool,
    agent: Option<u16>,
    scene: Option<usize>,
}

/// Places the player where the simulation has the avatar, between ticks.
pub fn follow_avatar(
    game: Res<Game>,
    library: Res<SceneLibrary>,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    sound: Res<crate::sound::Sound>,
    mut preview: Local<LocalAvatarPreview>,
) {
    let (Some(current), Some(index)) = (&game.current, game.player()) else { return };
    let a = &current.agents[index as usize];
    let before = game.previous.as_ref().and_then(|p| p.agents.get(index as usize)).unwrap_or(a);
    // The local single-player avatar can use the newest confirmed pose.
    // Interpolating from the previous tick adds a full tick of visible lag.
    let alpha = if !matches!(game.network, Some(Network::Join(_)))
        && game.session.as_ref().is_some_and(|session| !session.is_replay()) {
        1.0
    } else {
        game.alpha
    };
    let from = game.to_view(before.position);
    let to = game.to_view(a.position);
    // A teleport (respawn, deploying a recording) snaps instead of sliding.
    let alpha = if from.distance(to) > 1.5 { 1.0 } else { alpha };
    let mut position = from.lerp(to, alpha);
    position.y = before.avatar.height.to_f32() + (a.avatar.height - before.avatar.height).to_f32() * alpha;
    let mut preview_velocity = None;
    if matches!(game.network, Some(Network::Join(_))) {
        let reset = preview.agent != Some(index)
            || preview.scene != game.scene
            || preview.position.is_none_or(|p| p.distance(to) > 1.5);
        if reset {
            preview.position = Some(to);
            preview.velocity = Vec3::ZERO;
            preview.active = false;
            preview.agent = Some(index);
            preview.scene = game.scene;
        }
        let pending_goal = game.visual_goal.filter(|_| {
            game.session.as_ref().is_some_and(|session| session.pending_inputs() > 0)
        });
        let movement = game.walking
            .map(|(direction, run)| (Vec3::new(direction.x.to_f32(), 0.0, direction.y.to_f32()), run))
            .or_else(|| pending_goal.map(|(goal, run)| {
                let delta = crate::navigation::flat(goal - preview.position.unwrap());
                (if delta.length() < 0.1 { Vec3::ZERO } else { delta.normalize() }, run)
            }))
            .filter(|_| controls.attack.is_none() && a.avatar.dig.is_none());
        let moving_input = movement.is_some_and(|(direction, _)| direction != Vec3::ZERO);
        if let Some((direction, run)) = movement {
            preview.active = true;
            let dt = time.delta_secs().min(0.05);
            let motion = library.scenes[library.current].scene.style.as_ref()
                .map_or_else(voxel_assets::CharacterMotion::default, |style| style.character_motion);
            let desired = direction * if run { motion.run_speed } else { motion.walk_speed };
            let acceleration = if desired == Vec3::ZERO { 32.0 } else { 26.0 };
            let change = (desired - preview.velocity).clamp_length_max(acceleration * dt);
            preview.velocity += change;
            let start = preview.position.unwrap();
            let candidate = start + preview.velocity * dt;
            let passable = game.world().is_some_and(|world| {
                a.avatar.flight.airborne || world.terrain().walkable(
                    game.to_sim(start), game.to_sim(candidate), Fx::quantize(0.32),
                )
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
        if preview.active
            && !moving_input
            && game.session.as_ref().is_some_and(|session| session.pending_inputs() == 0)
            && a.avatar.velocity == FxVec2::ZERO
        {
            let remaining = to - preview.position.unwrap();
            preview.position = Some(preview.position.unwrap() + remaining.clamp_length_max(2.0 * time.delta_secs()));
        }
        if preview.active && preview.position.unwrap().distance(to) < 0.02
            && !moving_input
        {
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
    let dt = time.delta_secs().max(1e-4);
    let moved = if teleport { Vec3::ZERO } else { crate::navigation::flat(position - previous) };
    controls.velocity = Vec3::new(a.avatar.velocity.x.to_f32(), 0.0, a.avatar.velocity.y.to_f32());
    if let Some(velocity) = preview_velocity {
        controls.velocity = velocity;
    }
    let motion = library.scenes[library.current]
        .scene
        .style
        .as_ref()
        .map_or_else(voxel_assets::CharacterMotion::default, |s| s.character_motion);
    let relative_speed = controls.velocity.length() / motion.walk_speed.max(0.1);
    let step_distance = crate::character::step_distance_for_speed(
        motion.step_distance,
        relative_speed,
    ).clamp(0.2, 4.0);
    let contacts = controls.gait.advance(moved.length(), step_distance, !a.avatar.flight.airborne);
    for _ in 0..contacts {
        sound.play(crate::sound::Cue::Step);
    }
    controls.route = a.route.iter().map(|&p| game.to_view(p)).collect();
    let _ = dt;
}

fn angle_between(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Fills the settlement view from the simulation.
pub fn mirror_settlement(game: Res<Game>, library: Res<SceneLibrary>, mut settlement: ResMut<crate::settlement::Settlement>) {
    let (Some(current), Some(scenario)) = (&game.current, library.scenes.get(library.current).and_then(|p| p.scene.scenario.as_ref())) else {
        settlement.simulation = None;
        return;
    };
    let config = settlement.config.get_or_insert_with(|| crate::legacy::Scenario::new(scenario)).clone();
    let mut view = crate::legacy::Simulation::new(&config, game.previous.as_ref(), current, game.tick, game.alpha, game.center, &game.events);
    view.paused = game.paused;
    settlement.simulation = Some(view);
}

/// Clears the frame's events once every view has seen them.
pub fn end_frame(mut game: ResMut<Game>) {
    game.events.clear();
}

/// Keyboard movement: camera-relative walking, the jetpack, digging, and respawning.
pub fn keyboard_input(keys: Res<ButtonInput<KeyCode>>, library: Res<SceneLibrary>, mut controls: ResMut<Controls>, mut game: ResMut<Game>) {
    let Some(agent) = game.player() else { return };
    if keys.just_pressed(KeyCode::KeyR) {
        game.command(CommandKind::Respawn { agent });
        let attack_serial = controls.attack_serial;
        *controls = Controls { attack_serial, ..default() };
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        if library.has_feature(voxel_assets::SceneFeature::Jump) {
            game.command(CommandKind::Jump { agent });
        } else {
            game.command(CommandKind::Jetpack { agent, enabled: true });
        }
    }
    if keys.just_released(KeyCode::Space) && !library.has_feature(voxel_assets::SceneFeature::Jump) {
        game.command(CommandKind::Jetpack { agent, enabled: false });
    }
    // X picks the spear in combat scenes, so digging is F.
    if keys.just_pressed(KeyCode::KeyF) {
        game.dig();
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
    if input != Vec2::ZERO || game.walking.is_some_and(|(d, _)| d != FxVec2::ZERO) {
        game.walk(Vec2::new(direction.x, direction.z), run);
    }
}

/// Right click (held) walks to the cursor.
pub fn mouse_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    library: Res<SceneLibrary>,
    mut controls: ResMut<Controls>,
    mut game: ResMut<Game>,
) {
    let run = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    controls.repath -= time.delta_secs();
    if buttons.pressed(MouseButton::Right) && controls.attack.is_none() && controls.repath <= 0.0 {
        controls.repath = 0.12;
        if let Some(point) = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &library, &game) {
            if controls.route.last().is_none_or(|last| crate::navigation::flat(*last - point).length() > 0.25) {
                game.go_to(point, run);
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
    library: Res<SceneLibrary>,
    mut controls: ResMut<Controls>,
    mut game: ResMut<Game>,
) {
    use content::scenario::Weapon;
    if let Some(scenario) = library.scenes[library.current].scene.scenario.as_ref() {
        for (key, weapon) in [(KeyCode::KeyZ, Weapon::Sword), (KeyCode::KeyX, Weapon::Spear), (KeyCode::KeyC, Weapon::Bow), (KeyCode::KeyV, Weapon::ArcCoil)] {
            if keys.just_pressed(key) && scenario.weapons.contains(&weapon) {
                controls.weapon = weapon;
                game.equip(weapon);
            }
        }
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !(shift && buttons.just_pressed(MouseButton::Left)) {
        return;
    }
    if let Some(point) = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &library, &game) {
        if let Some(origin) = game.player_view_position() {
            strike(&mut game, &mut controls, &library, point - origin);
        }
    }
}

/// Starts an attack: the swing animates now, and the simulation resolves it
/// on its windup tick.
pub fn strike(game: &mut Game, controls: &mut Controls, library: &SceneLibrary, aim: Vec3) {
    let Some(agent) = game.player() else { return };
    let weapon = controls.weapon;
    let muzzle = library.scenes[library.current]
        .scene
        .style
        .as_ref()
        .and_then(|style| style.weapon_skins.iter().find(|skin| skin.weapon == weapon).map(|skin| skin.muzzle.map(|v| v * style.actor_scale)))
        .unwrap_or([0.0, 0.6, 0.25]);
    let flat = crate::navigation::flat(aim).normalize_or(Vec3::Z);
    game.walking = None;
    game.visual_goal = None;
    game.command(CommandKind::Attack {
        agent,
        weapon,
        aim: FxVec2::new(Fx::quantize(flat.x), Fx::quantize(flat.z)),
        muzzle: muzzle.map(Fx::quantize),
    });
    controls.strike(aim);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_position(app: &mut App) -> Vec3 {
        let mut players = app.world_mut().query_filtered::<&Transform, With<Player>>();
        players.single(app.world()).unwrap().translation
    }

    #[test]
    fn solo_avatar_shows_the_latest_tick_on_the_frame_it_runs() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::build_app(options, SceneLibrary::new(), hub);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 120.0),
        ));
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);

        let mut observed_movement = false;
        for _ in 0..12 {
            app.update();
            let game = app.world().resource::<Game>();
            let Some(index) = game.player() else { continue };
            let current = game.current.as_ref().unwrap();
            let current_position = game.to_view(current.agents[index as usize].position);
            let previous_position = game.previous.as_ref()
                .map(|previous| game.to_view(previous.agents[index as usize].position))
                .unwrap_or(current_position);
            if current_position.distance(previous_position) < 0.001 {
                continue;
            }
            let shown = player_position(&mut app);
            assert!(crate::navigation::flat(shown - current_position).length() < 0.001,
                "local avatar lagged behind its latest simulated pose: {shown:?} vs {current_position:?}");
            observed_movement = true;
            break;
        }
        assert!(observed_movement, "movement never reached the simulation");
    }

    #[test]
    fn host_avatar_responds_on_the_next_tick_and_shows_that_pose() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut library = SceneLibrary::new();
        library.initial = library.index("field").unwrap();
        let mut app = crate::build_app(options, library, hub);
        let mut game = Game::default();
        game.network = Some(Network::Host("127.0.0.1:0".parse().unwrap()));
        app.insert_resource(game);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 120.0),
        ));
        app.update();
        let start = player_position(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        for frame in 1..=4 {
            app.update();
            let game = app.world().resource::<Game>();
            let index = game.player().unwrap() as usize;
            let confirmed = game.to_view(game.current.as_ref().unwrap().agents[index].position);
            if crate::navigation::flat(confirmed - start).length() > 0.001 {
                let shown = player_position(&mut app);
                assert!(crate::navigation::flat(shown - confirmed).length() < 0.001,
                    "host avatar displayed an older tick after movement applied");
                assert!(frame <= 2, "host movement waited {frame} frames");
                return;
            }
        }
        panic!("host input did not move the avatar by the next simulation tick");
    }

    #[test]
    fn joined_player_movement_response_is_measured_in_frames() {
        let make_app = |network| {
            let options = crate::control::Options { simulation: true, ..default() };
            let hub = crate::control::Hub::new(options.clone()).unwrap();
            let mut library = SceneLibrary::new();
            library.initial = library.index("field").unwrap();
            let mut app = crate::build_app(options, library, hub);
            let mut game = Game::default();
            game.network = Some(network);
            app.insert_resource(game);
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_secs_f64(1.0 / 60.0),
            ));
            app
        };
        let mut host = make_app(Network::Host("127.0.0.1:0".parse().unwrap()));
        host.update();
        let address = host.world().resource::<Game>().session.as_ref().unwrap().local_addr().unwrap();
        let mut client = make_app(Network::Join(address));
        let mut joined = false;
        for _ in 0..240 {
            host.update();
            client.update();
            if client.world().resource::<Game>().player().is_some() {
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
                let game = client.world().resource::<Game>();
                let index = game.player().unwrap() as usize;
                let confirmed = game.to_view(game.current.as_ref().unwrap().agents[index].position);
                assert!(crate::navigation::flat(confirmed - start).length() < 0.001,
                    "the test did not observe a visual preview before the confirmed move");
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
            assert!(crate::navigation::flat(shown - last).length() < 0.2,
                "the local avatar jumped while reconciling with the host");
            last = shown;
        }
        let game = client.world().resource::<Game>();
        let index = game.player().unwrap() as usize;
        let confirmed = game.to_view(game.current.as_ref().unwrap().agents[index].position);
        assert!(crate::navigation::flat(last - confirmed).length() < 0.1,
            "the local avatar did not settle on the host's confirmed pose");
        let goal = confirmed + Vec3::new(0.6, 0.0, 0.0);
        client.world_mut().resource_mut::<Game>().go_to(goal, false);
        client.update();
        host.update();
        let route_preview = player_position(&mut client);
        assert!(crate::navigation::flat(route_preview - last).length() > 0.001,
            "click-to-move gave no immediate visual response");
        let game = client.world().resource::<Game>();
        let index = game.player().unwrap() as usize;
        let still_confirmed = game.to_view(game.current.as_ref().unwrap().agents[index].position);
        assert!(crate::navigation::flat(still_confirmed - confirmed).length() < 0.001,
            "the click-to-move test did not observe prediction before confirmation");
        let mut followed_route = false;
        let mut previous_shown = route_preview;
        for _ in 0..90 {
            client.update();
            host.update();
            let shown = player_position(&mut client);
            assert!(crate::navigation::flat(shown - previous_shown).length() < 0.2,
                "click-to-move preview jumped while reconciling with the host");
            previous_shown = shown;
            let game = client.world().resource::<Game>();
            let index = game.player().unwrap() as usize;
            let confirmed_route = game.to_view(game.current.as_ref().unwrap().agents[index].position);
            if crate::navigation::flat(confirmed_route - confirmed).length() > 0.05 {
                followed_route = true;
            }
        }
        assert!(followed_route, "the joined avatar stopped following confirmed click-to-move motion");
        let game = client.world().resource::<Game>();
        let index = game.player().unwrap() as usize;
        let confirmed_route = game.to_view(game.current.as_ref().unwrap().agents[index].position);
        assert!(crate::navigation::flat(previous_shown - confirmed_route).length() < 0.1,
            "click-to-move preview did not settle on the confirmed route");
    }
}

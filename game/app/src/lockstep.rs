//! The game's simulation: every scene runs on the deterministic lockstep
//! simulation (`game/sim`) through a session (`game/session`). This module
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
use crate::gameplay::{Controls, Obstacle, Player};

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
            applies_at: 0,
            record: None,
            replay: None,
            start_save: None,
            warming_up: false,
            network: None,
            status: String::new(),
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

    /// The local player's avatar ID, as views name agents.
    pub fn player_id(&self) -> Option<String> {
        Some(self.settlement()?.agents[self.player()? as usize].id.clone())
    }

    /// Sends a command; it applies on the next tick in single-player, and
    /// after the input delay in a networked game.
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
        }),
        memory_trial: library.has_feature(voxel_assets::SceneFeature::MemoryTrial),
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
    game.applies_at = 0;
    game.events.clear();
}

/// Runs every tick due by this frame's time and keeps the last two for interpolation.
pub fn step(time: Res<Time>, hub: Option<Res<crate::control::Hub>>, mut game: ResMut<Game>) {
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
    session.update(clock, |world| {
        ticks.push((world.tick(), world.settlement().cloned()));
    });
    for (tick, settlement) in ticks {
        if let Some(s) = &settlement {
            game.events.extend(s.events.iter().cloned());
        }
        game.previous = std::mem::replace(&mut game.current, settlement);
        game.tick = tick.saturating_sub(1);
    }
    game.alpha = session.alpha(clock);
    for event in session.events.drain(..) {
        match event {
            net::NetEvent::Desync { .. } | net::NetEvent::HostLost | net::NetEvent::Rejected { .. } => warn!("{event:?}"),
            _ => info!("{event:?}"),
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
}

/// Tells this machine apart from others when joining. A random ID per run:
/// a rejoining player gets a new avatar.
fn stable_id() -> net::StableId {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    nanos ^ (u64::from(std::process::id()) << 32)
}

/// Places the player where the simulation has the avatar, between ticks.
pub fn follow_avatar(
    game: Res<Game>,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    sound: Res<crate::sound::Sound>,
) {
    let (Some(current), Some(index)) = (&game.current, game.player()) else { return };
    let a = &current.agents[index as usize];
    let before = game.previous.as_ref().and_then(|p| p.agents.get(index as usize)).unwrap_or(a);
    let alpha = game.alpha;
    let from = game.to_view(before.position);
    let to = game.to_view(a.position);
    // A teleport (respawn, deploying a recording) snaps instead of sliding.
    let alpha = if from.distance(to) > 1.5 { 1.0 } else { alpha };
    let mut position = from.lerp(to, alpha);
    position.y = before.avatar.height.to_f32() + (a.avatar.height - before.avatar.height).to_f32() * alpha;
    let previous = player.translation;
    let teleport = previous.distance(position) > 1.5;
    player.translation = position;
    let heading = before.heading.to_f32() + angle_between(before.heading.to_f32(), a.heading.to_f32()) * alpha;
    player.rotation = Quat::from_rotation_y(heading);
    let dt = time.delta_secs().max(1e-4);
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

/// Keyboard movement: camera-relative walking, the jetpack, and respawning.
pub fn keyboard_input(keys: Res<ButtonInput<KeyCode>>, mut controls: ResMut<Controls>, mut game: ResMut<Game>) {
    let Some(agent) = game.player() else { return };
    if keys.just_pressed(KeyCode::KeyR) {
        game.command(CommandKind::Respawn { agent });
        let attack_serial = controls.attack_serial;
        *controls = Controls { attack_serial, ..default() };
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        game.command(CommandKind::Jetpack { agent, enabled: true });
    }
    if keys.just_released(KeyCode::Space) {
        game.command(CommandKind::Jetpack { agent, enabled: false });
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
        if let Some(point) = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &library) {
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
    player: Single<&Transform, With<Player>>,
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
    if let Some(point) = crate::jetpack::cursor_surface(camera.0, camera.1, &window, &library) {
        strike(&mut game, &mut controls, &library, point - player.translation);
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
    game.command(CommandKind::Attack {
        agent,
        weapon,
        aim: FxVec2::new(Fx::quantize(flat.x), Fx::quantize(flat.z)),
        muzzle: muzzle.map(Fx::quantize),
    });
    controls.strike(aim);
}

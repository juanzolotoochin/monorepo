//! The game binary.
//!
//! ```text
//! play [--scenario small|large|empty | --load SAVE] [--host ADDR:PORT]
//! play --join ADDR:PORT
//!     [--input-delay TICKS] [--threads N] [--resync] [--exit-after SECONDS [--screenshot PATH]]
//! ```
//!
//! Without `--host` or `--join` the game is single-player: a host with no
//! clients. `--load` hosts from a save, which is how a game continues after
//! the host is lost (every peer writes `autosave.gsav` on the same ticks and
//! `hostlost.gsav` when the host goes away).
//!
//! Controls: left click spawns movers, right click pushes the nearest mover
//! toward the cursor, WASD pans, the wheel zooms, F5 saves to `quicksave.gsav`.

mod session;

use std::net::SocketAddr;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use content::Content;
use fx::{Fx, FxVec2};
use net::{DesyncPolicy, NetConfig, NetEvent};
use sim::scenario::Scenario;
use sim::{CommandKind, World, CHUNK_TILES};
use view::{cursor_ground, DrawSystems, MainCamera, ShowMarker, SimView, ViewConfig, ViewPlugin};

use session::{local_stable_id, Mode, Session};

const QUICKSAVE: &str = "quicksave.gsav";

#[derive(Default)]
struct Options {
    scenario: Option<String>,
    load: Option<String>,
    host: Option<SocketAddr>,
    join: Option<SocketAddr>,
    input_delay: Option<u64>,
    threads: usize,
    resync: bool,
    exit_after: Option<f32>,
    screenshot: Option<String>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--resync" {
            options.resync = true;
            continue;
        }
        let value = iter.next().ok_or(format!("{arg} needs a value"))?;
        let number = || value.parse::<f64>().map_err(|_| format!("{arg}: invalid number {value}"));
        let addr = || value.parse::<SocketAddr>().map_err(|_| format!("{arg}: expected ADDRESS:PORT, got {value}"));
        match arg.as_str() {
            "--scenario" => options.scenario = Some(value.clone()),
            "--load" => options.load = Some(value.clone()),
            "--host" => options.host = Some(addr()?),
            "--join" => options.join = Some(addr()?),
            "--input-delay" => options.input_delay = Some(number()? as u64),
            "--threads" => options.threads = number()? as usize,
            "--exit-after" => options.exit_after = Some(number()? as f32),
            "--screenshot" => options.screenshot = Some(value.clone()),
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    if options.join.is_some() && (options.host.is_some() || options.load.is_some() || options.scenario.is_some()) {
        return Err("--join can't be combined with --host, --load or --scenario".into());
    }
    Ok(options)
}

/// The world to host and the commands that set it up.
fn starting_world(options: &Options, content: &Content) -> Result<(World, Vec<CommandKind>), String> {
    if let Some(path) = &options.load {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let world = save::read(&bytes, content).map_err(|e| format!("{path}: {e}"))?;
        return Ok((world, Vec::new()));
    }
    let scenario = match options.scenario.as_deref().unwrap_or("small") {
        "empty" => return Ok((World::new(Scenario::small().config, content.rules.clone()), Vec::new())),
        "small" => Scenario::small(),
        "large" => Scenario::large(),
        other => return Err(format!("unknown scenario {other}")),
    };
    // The scenario's opening spawns go through the turn path like any input.
    let opening = scenario.turn(&scenario.start(1)).into_iter().map(|c| c.kind).collect();
    Ok((World::new(scenario.config.clone(), content.rules.clone()), opening))
}

fn main() {
    let options = parse_options().unwrap_or_else(|e| fail(&e));
    let content = Content::builtin();
    let threads = if options.threads == 0 { std::thread::available_parallelism().map_or(4, |n| n.get()) } else { options.threads };
    let defaults = NetConfig::default();
    let config = NetConfig {
        threads,
        input_delay: options.input_delay.unwrap_or(if options.host.is_some() { defaults.input_delay } else { 2 }),
        desync: if options.resync { DesyncPolicy::Resync } else { DesyncPolicy::Halt },
        ..defaults
    };
    let mode_and_opening = match options.join {
        Some(host) => Ok((Mode::Join { host, stable_id: local_stable_id() }, Vec::new())),
        None => starting_world(&options, &content).map(|(world, opening)| (Mode::Host { world, listen: options.host }, opening)),
    };
    let (mode, opening) = mode_and_opening.unwrap_or_else(|e| fail(&e));
    let mut session = Session::start(mode, content.clone(), config).unwrap_or_else(|e| fail(&format!("network: {e}")));
    for kind in opening {
        session.queue(kind);
    }

    let view_config = ViewConfig {
        mover_colors: content.views.movers.iter().map(|v| v.color).collect(),
        chunk_tiles: CHUNK_TILES as f32,
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Lockstep — left click: spawn · right click: push · F5: save".into(),
                resolution: (1440, 960).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(ViewPlugin(view_config))
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
        .insert_resource(session)
        .insert_resource(RunStats { exit_after: options.exit_after, screenshot: options.screenshot, ..default() })
        .add_systems(Startup, spawn_hud)
        .add_systems(Update, (player_input, step_simulation, handle_events).chain().before(DrawSystems))
        .add_systems(Update, (update_hud, quicksave, exit_after).after(DrawSystems))
        .add_systems(Last, say_goodbye)
        .run();
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

#[derive(Resource, Default)]
struct RunStats {
    exit_after: Option<f32>,
    screenshot: Option<String>,
    /// Frames left before exiting once the screenshot was requested.
    exit_countdown: Option<u32>,
    elapsed: f32,
    frames: u64,
    ticks_at_start: Option<u64>,
    message: Option<String>,
}

/// Runs the network and the simulation, and hands the view the new extract.
fn step_simulation(mut session: ResMut<Session>, mut view: ResMut<SimView>) {
    if let Some(extract) = session.update() {
        view.interpolator.push(extract);
    }
    view.alpha = session.alpha();
}

fn handle_events(mut session: ResMut<Session>, mut stats: ResMut<RunStats>) {
    for event in std::mem::take(&mut session.events) {
        info!("{event:?}");
        match event {
            NetEvent::Desync { tick, .. } => {
                session.dump_desync(tick);
                stats.message = Some(format!("DESYNC at tick {tick}: wrote desync-{tick}-*.gsav and .hashes"));
            }
            NetEvent::HostLost => {
                stats.message = Some("Lost the host. Saved hostlost.gsav; re-host with --load hostlost.gsav --host ADDR:PORT".into());
            }
            NetEvent::Rejected { reason } => stats.message = Some(format!("Rejected: {reason}")),
            NetEvent::PlayerJoined { player } => stats.message = Some(format!("{player:?} joined")),
            NetEvent::PlayerLeft { player } => stats.message = Some(format!("{player:?} left")),
            NetEvent::Loaded { tick } => stats.message = Some(format!("Joined at tick {tick}")),
        }
    }
}

fn player_input(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    view: Res<SimView>,
    mut session: ResMut<Session>,
    mut markers: MessageWriter<ShowMarker>,
) {
    let (camera, camera_transform) = *camera;
    let Some(ground) = cursor_ground(&window, camera, camera_transform) else { return };
    let Some(world) = session.world() else { return };
    let size = world.config().size();
    if ground.x < 0.0 || ground.y < 0.0 || ground.x >= size.x.to_f32() || ground.y >= size.y.to_f32() {
        return;
    }
    // Input is quantized to Fx here, before it becomes a command.
    let at = FxVec2::new(quantize(ground.x), quantize(ground.y));
    if buttons.just_pressed(MouseButton::Left) {
        session.queue(CommandKind::SpawnMovers { center: at, count: 40 });
        markers.write(ShowMarker { at: ground, radius: 8.0 });
    }
    if buttons.just_pressed(MouseButton::Right) {
        let nearest = view.interpolator.current().movers.iter().min_by(|a, b| {
            let da = Vec2::new(a.pos[0], a.pos[1]).distance_squared(ground);
            let db = Vec2::new(b.pos[0], b.pos[1]).distance_squared(ground);
            da.total_cmp(&db)
        });
        if let Some(mover) = nearest {
            let direction = (ground - Vec2::new(mover.pos[0], mover.pos[1])).normalize_or_zero();
            let velocity = FxVec2::new(quantize(direction.x), quantize(direction.y));
            session.queue(CommandKind::Impulse { mover: sim::Id::from_bits(mover.id), velocity });
            markers.write(ShowMarker { at: Vec2::new(mover.pos[0], mover.pos[1]), radius: mover.radius * 2.0 });
        }
    }
}

/// Rounds view-space input to 1/1024 of a tile, so commands are compact and exact.
fn quantize(v: f32) -> Fx {
    Fx::from_ratio((v * 1024.0).round() as i64, 1024)
}

#[derive(Component)]
struct Hud;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::srgb(0.85, 0.95, 0.9)),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(10.0), ..default() },
    ));
}

/// The HUD, including the net graph numbers.
fn update_hud(session: Res<Session>, stats: Res<RunStats>, diagnostics: Res<DiagnosticsStore>, mut text: Single<&mut Text, With<Hud>>) {
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let net = session.stats();
    let mut lines = vec![session.status()];
    if let Some(world) = session.world() {
        lines.push(format!(
            "tick {}  movers {}  update {:.2} ms  {:.0} fps",
            world.tick(),
            world.mover_count(),
            session.last_update_ms(),
            fps
        ));
    }
    let rtt = net.rtt.map_or("-".to_string(), |rtt| format!("{} ms", rtt.as_millis()));
    lines.push(format!(
        "net: rtt {rtt}  input delay {}  buffered {}  behind host {}  late inputs {}  out {} KB  in {} KB",
        session.input_delay(),
        net.turns_buffered,
        net.ticks_behind_host,
        net.late_inputs,
        net.bytes_sent / 1024,
        net.bytes_received / 1024
    ));
    if let Some(message) = &stats.message {
        lines.push(message.clone());
    }
    text.0 = lines.join("\n");
}

fn quicksave(keys: Res<ButtonInput<KeyCode>>, session: Res<Session>) {
    if keys.just_pressed(KeyCode::F5) {
        if let Some(bytes) = session.save() {
            match std::fs::write(QUICKSAVE, bytes) {
                Ok(()) => info!("saved {QUICKSAVE}"),
                Err(e) => error!("could not save {QUICKSAVE}: {e}"),
            }
        }
    }
}

fn say_goodbye(mut exits: MessageReader<AppExit>, mut session: ResMut<Session>) {
    if exits.read().next().is_some() {
        session.shutdown();
    }
}

/// With `--exit-after`, quits after that many seconds and prints frame and
/// tick rates, taking a screenshot first if asked.
fn exit_after(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    mut stats: ResMut<RunStats>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let Some(limit) = stats.exit_after else { return };
    if let Some(frames) = &mut stats.exit_countdown {
        *frames = frames.saturating_sub(1);
        if *frames == 0 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let tick = session.world().map_or(0, World::tick);
    if stats.ticks_at_start.is_none() {
        stats.ticks_at_start = Some(tick);
    }
    stats.frames += 1;
    stats.elapsed += time.delta_secs();
    if stats.elapsed >= limit {
        let ticks = tick - stats.ticks_at_start.unwrap();
        println!(
            "ran {:.1} s: {:.1} frames/s, {:.1} ticks/s, tick {tick}, {} movers, {}",
            stats.elapsed,
            stats.frames as f32 / stats.elapsed,
            ticks as f32 / stats.elapsed,
            session.world().map_or(0, World::mover_count),
            session.status()
        );
        match stats.screenshot.take() {
            Some(path) => {
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
                stats.exit_countdown = Some(30);
            }
            None => {
                exit.write(AppExit::Success);
            }
        }
    }
}

//! The game binary.
//!
//! ```text
//! play [--scenario small|large|empty] [--input-delay TICKS] [--threads N]
//!     [--exit-after SECONDS [--screenshot PATH]]
//! ```
//!
//! Controls: left click spawns movers, right click pushes the nearest mover
//! toward the cursor, WASD pans, the wheel zooms, F5 quicksaves, F9 quickloads.

mod session;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use content::Content;
use fx::{Fx, FxVec2};
use sim::scenario::Scenario;
use sim::{CommandKind, CHUNK_TILES};
use view::{cursor_ground, DrawSystems, MainCamera, ShowMarker, SimView, ViewConfig, ViewPlugin};

use session::Session;

const QUICKSAVE: &str = "quicksave.gsav";

struct Options {
    scenario: String,
    input_delay: u64,
    threads: usize,
    exit_after: Option<f32>,
    screenshot: Option<String>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options { scenario: "small".into(), input_delay: 3, threads: 0, exit_after: None, screenshot: None };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let value = iter.next().ok_or(format!("{arg} needs a value"))?;
        let number = || value.parse::<f64>().map_err(|_| format!("{arg}: invalid number {value}"));
        match arg.as_str() {
            "--scenario" => options.scenario = value.clone(),
            "--input-delay" => options.input_delay = number()? as u64,
            "--threads" => options.threads = number()? as usize,
            "--exit-after" => options.exit_after = Some(number()? as f32),
            "--screenshot" => options.screenshot = Some(value.clone()),
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    Ok(options)
}

fn main() {
    let options = parse_options().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let content = Content::builtin();
    let (config, opening) = match options.scenario.as_str() {
        "empty" => (Scenario::small().config, None),
        "small" => (Scenario::small().config, Some(Scenario::small())),
        "large" => (Scenario::large().config, Some(Scenario::large())),
        other => {
            eprintln!("unknown scenario {other}");
            std::process::exit(2)
        }
    };
    let threads = if options.threads == 0 { headless_threads() } else { options.threads };
    let mut session = Session::new_game(config.clone(), &content, threads, options.input_delay);
    if let Some(scenario) = opening {
        // The scenario's opening spawns go through the turn path like any input.
        let probe = scenario.start(1);
        for command in scenario.turn(&probe) {
            session.queue(command.kind);
        }
    }

    let view_config = ViewConfig {
        mover_colors: content.views.movers.iter().map(|v| v.color).collect(),
        world_size: Vec2::new(config.size().x.to_f32(), config.size().y.to_f32()),
        chunk_tiles: CHUNK_TILES as f32,
    };
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Lockstep — left click: spawn · right click: push · F5/F9: save/load".into(),
            resolution: (1440, 960).into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(FrameTimeDiagnosticsPlugin::default())
    .add_plugins(ViewPlugin(view_config))
    .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
    .insert_resource(session)
    .insert_resource(ContentRes(content))
    .insert_resource(RunStats { exit_after: options.exit_after, screenshot: options.screenshot, ..default() })
    .add_systems(Startup, spawn_hud)
    .add_systems(Update, (player_input, step_simulation).chain().before(DrawSystems))
    .add_systems(Update, (update_hud, quicksave, exit_after).after(DrawSystems))
    .run();
}

fn headless_threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

#[derive(Resource)]
struct ContentRes(Content);

#[derive(Resource, Default)]
struct RunStats {
    exit_after: Option<f32>,
    screenshot: Option<String>,
    /// Frames left before exiting once the screenshot was requested.
    exit_countdown: Option<u32>,
    elapsed: f32,
    frames: u64,
    ticks_at_start: u64,
}

/// Steps the simulation at 30 Hz in fixed steps and hands the view a new extract.
fn step_simulation(time: Res<Time>, mut session: ResMut<Session>, mut view: ResMut<SimView>) {
    for extract in session.advance(time.delta()) {
        view.interpolator.push(extract);
    }
    view.alpha = session.alpha();
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
    let size = session.world().config().size();
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

fn update_hud(
    session: Res<Session>,
    diagnostics: Res<DiagnosticsStore>,
    mut text: Single<&mut Text, With<Hud>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let world = session.world();
    text.0 = format!(
        "tick {}  movers {}  sim {:.2} ms/tick  input delay {} ticks  {:.0} fps",
        world.tick(),
        world.mover_count(),
        session.last_step_ms(),
        session.input_delay(),
        fps
    );
}

fn quicksave(keys: Res<ButtonInput<KeyCode>>, mut session: ResMut<Session>, content: Res<ContentRes>, mut view: ResMut<SimView>) {
    if keys.just_pressed(KeyCode::F5) {
        match std::fs::write(QUICKSAVE, session.save(&content.0)) {
            Ok(()) => info!("saved {QUICKSAVE} at tick {}", session.world().tick()),
            Err(e) => error!("could not save {QUICKSAVE}: {e}"),
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        let loaded = std::fs::read(QUICKSAVE).map_err(|e| e.to_string()).and_then(|bytes| {
            session.load(&bytes, &content.0).map_err(|e| e.to_string())
        });
        match loaded {
            Ok(extract) => {
                // A load is a teleport: show it without interpolating.
                view.interpolator.push(extract.clone());
                view.interpolator.push(extract);
                info!("loaded {QUICKSAVE} at tick {}", session.world().tick());
            }
            Err(e) => error!("could not load {QUICKSAVE}: {e}"),
        }
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
    if stats.frames == 0 {
        stats.ticks_at_start = session.world().tick();
    }
    stats.frames += 1;
    stats.elapsed += time.delta_secs();
    if stats.elapsed >= limit {
        let ticks = session.world().tick() - stats.ticks_at_start;
        println!(
            "ran {:.1} s: {:.1} frames/s, {:.1} ticks/s, {} movers",
            stats.elapsed,
            stats.frames as f32 / stats.elapsed,
            ticks as f32 / stats.elapsed,
            session.world().mover_count()
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

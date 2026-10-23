//! One command path for live clients, deterministic scripts and renderer-free tests.
mod protocol;
#[cfg(test)]
mod tests;
mod transport;
mod video;
use crate::{
    assets::SceneLibrary,
    gameplay::{Controls, Obstacle, Player},
    impact::ImpactTarget,
    navigation,
    piles::{begin_job_with_options, Action, PileLab},
};
use bevy::{app::AppExit, prelude::*};
use protocol::{point, Command, Request, Script, VERSION};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{mpsc, Mutex},
};
use transport::{Envelope, Server};
pub use video::{capture_frame, setup_render, Video};

#[derive(Resource, Clone, Default)]
pub struct Options {
    pub simulation: bool,
    pub script: Option<Script>,
    pub frames_dir: Option<String>,
    pub report: Option<String>,
    pub trace: Option<String>,
    pub audio: Option<String>,
    pub port: Option<u16>,
    pub address_file: Option<String>,
}
impl Options {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let value = |key: &str| -> Option<&str> {
            args.windows(2).find(|a| a[0] == key).map(|a| a[1].as_str())
        };
        let script = if let Some(path) = value("--demo-script") {
            let script: Script =
                serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)
                    .map_err(|e| e.to_string())?;
            script.validate()?;
            Some(script)
        } else {
            None
        };
        let options = Self {
            simulation: args.iter().any(|a| a == "--simulation"),
            script,
            frames_dir: value("--frames-dir").map(str::to_string),
            report: value("--report").map(str::to_string),
            trace: value("--trace").map(str::to_string),
            audio: value("--audio-wav").map(str::to_string),
            port: value("--control-port")
                .map(str::parse)
                .transpose()
                .map_err(|e| format!("Invalid port: {e}"))?,
            address_file: value("--control-address-file").map(str::to_string),
        };
        if options.frames_dir.is_some() && (options.script.is_none() || options.simulation) {
            return Err("Frame recording requires a demo script and GPU rendering".into());
        }
        if options.simulation && options.script.is_none() && options.port.is_none() {
            return Err("Simulation mode needs a script or control port".into());
        }
        if options.audio.is_some() && options.frames_dir.is_none() {
            return Err("Audio export requires frame recording".into());
        }
        Ok(options)
    }
    pub fn fps(&self) -> u32 {
        self.script.as_ref().map_or(60, |s| s.fps)
    }
    pub fn initial_scene(&self, scenes: &SceneLibrary) -> Result<Option<usize>, String> {
        self.script.as_ref().map(|s| scenes.index(&s.scene)).transpose()
    }
}
pub fn rendered(options: Res<Options>) -> bool {
    !options.simulation
}
pub fn interactive(options: Res<Options>) -> bool {
    !options.simulation && options.frames_dir.is_none()
}

enum Pending {
    Instant,
    SettlementTransfer { scene: usize, expected: u32 },
    Walk(Vec3),
    Haul { expected_cargo: u32, serial: u64 },
    Scene(usize),
    Wait(u64),
    Strike,
    Failed(String),
}
struct Active {
    envelope: Envelope,
    pending: Pending,
    started: u64,
}
#[derive(Resource)]
pub struct Hub {
    options: Options,
    rx: Mutex<mpsc::Receiver<Envelope>>,
    _server: Option<Server>,
    queue: VecDeque<Envelope>,
    inspect: Vec<Envelope>,
    active: Option<Active>,
    pub tick: u64,
    pub finished: Option<u64>,
    pub failed: Option<String>,
    pub latest: Value,
    pub receipts: Vec<Value>,
    pub audio_events: Vec<Value>,
    warmup: u64,
    quit: Option<u64>,
    trace: Option<std::fs::File>,
}
impl Hub {
    pub fn warming_up(&self) -> bool {
        self.tick <= self.warmup
    }
    pub fn new(options: Options) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(64);
        let server = options
            .port
            .map(|port| Server::start(port, tx, options.address_file.as_deref()))
            .transpose()?;
        let queue = options
            .script
            .as_ref()
            .map(|s| {
                s.steps
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Envelope {
                        request: Request {
                            id: format!("step_{i:04}"),
                            command: c.clone(),
                        },
                        reply: None,
                        scripted: true,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let warmup = if options.frames_dir.is_some() { 30 } else { 0 };
        let trace = if options.frames_dir.is_none() {
            options
                .trace
                .as_ref()
                .map(std::fs::File::create)
                .transpose()
                .map_err(|error| format!("Cannot open world trace: {error}"))?
        } else {
            None
        };
        Ok(Self {
            trace,
            options,
            rx: Mutex::new(rx),
            _server: server,
            queue,
            inspect: vec![],
            active: None,
            tick: 0,
            finished: None,
            failed: None,
            latest: Value::Null,
            receipts: vec![],
            audio_events: vec![],
            warmup,
            quit: None,
        })
    }
    pub fn report(&self, frames: usize) -> Value {
        json!({"version":VERSION,"status":if self.failed.is_some(){"failed"}else{"passed"},"error":self.failed,"fps":self.options.fps(),"frames":frames,"tick":self.tick,"receipts":self.receipts,"state":self.latest,"audio":{"sample_rate":48000,"events":self.audio_events}})
    }
    pub fn save_report(&self, frames: usize) {
        if let Some(path) = &self.options.report {
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&self.report(frames)).unwrap(),
            )
            .expect("write demo report");
        }
    }
}
fn player_position(world: &mut World) -> Vec3 {
    world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .expect("player")
        .translation
}
fn discs(world: &mut World) -> Vec<navigation::Disc> {
    world
        .query::<&Obstacle>()
        .iter(world)
        .map(|o| o.0)
        .collect()
}
fn queue_haul(
    world: &mut World,
    action: Action,
    pace: f32,
    approach: Option<[f32; 2]>,
) -> Result<(), String> {
    let position = player_position(world);
    let obstacles = discs(world);
    let stand = approach.map(point).transpose()?;
    world.resource_scope(|world, mut lab: Mut<PileLab>| {
        begin_job_with_options(
            &mut lab,
            &mut world.resource_mut::<Controls>(),
            position,
            action,
            &obstacles,
            pace,
            stand,
        )
        .map_err(str::to_string)
    })
}
fn start(world: &mut World, command: &Command, tick: u64) -> Result<Pending, String> {
    match command {
        Command::Inspect => Ok(Pending::Instant),
        Command::InspectBuilding { building } => {
            crate::settlement::inspect_building(world, building.as_deref())?;
            Ok(Pending::Instant)
        }
        Command::SettlementTransfer {
            depot,
            item,
            units,
            deposit,
        } => {
            let expected =
                crate::settlement::player_transfer(world, depot, item, *units, *deposit)?;
            Ok(Pending::SettlementTransfer {
                scene: world.resource::<SceneLibrary>().current,
                expected,
            })
        }
        Command::SettlementCancel => {
            crate::settlement::model(world)?.cancel_transfer("player")?;
            Ok(Pending::Instant)
        }
        Command::SettlementPause { paused } => {
            crate::settlement::model(world)?.paused = *paused;
            Ok(Pending::Instant)
        }
        Command::SettlementRate { rate } => {
            crate::settlement::model(world)?.set_rate(*rate)?;
            Ok(Pending::Instant)
        }
        Command::SettlementLink { id, enabled } => {
            crate::settlement::model(world)?.set_link(id, *enabled)?;
            Ok(Pending::Instant)
        }
        Command::SettlementMachine { id, enabled } => {
            crate::settlement::model(world)?.set_machine(id, *enabled)?;
            Ok(Pending::Instant)
        }
        Command::SettlementDefender { id, enabled } => {
            crate::settlement::model(world)?.set_defender(id, *enabled)?;
            Ok(Pending::Instant)
        }
        Command::SettlementStock { depot, item, units } => {
            crate::settlement::model(world)?.add_stock(depot, item, *units)?;
            Ok(Pending::Instant)
        }
        Command::AssertSettlement {
            delivered,
            cycles,
            equipped,
            kills,
        } => {
            let mut state = world.resource_mut::<crate::settlement::Settlement>();
            let sim = state.simulation.as_mut().ok_or("No settlement loaded")?;
            sim.validate_state()?;
            if sim.metrics.delivered < *delivered
                || sim.metrics.cycles < *cycles
                || sim.metrics.equipped < *equipped
                || sim.metrics.kills < *kills
            {
                return Err(format!(
                    "Supply chain milestones not reached: {:?}",
                    sim.metrics
                ));
            }
            Ok(Pending::Instant)
        }
        Command::MemoryRecord => {
            crate::palimpsest::record(world)?;
            Ok(Pending::Instant)
        }
        Command::MemoryDeploy => {
            crate::palimpsest::deploy(world)?;
            Ok(Pending::Instant)
        }
        Command::MemoryClear => {
            crate::palimpsest::clear(world);
            Ok(Pending::Instant)
        }
        Command::AssertTrial {
            unlocked,
            collected,
        } => {
            let memory = world.resource::<crate::palimpsest::Memory>();
            if memory.unlocked != *unlocked || memory.collected != *collected {
                return Err("Field trial assertion failed".into());
            }
            Ok(Pending::Instant)
        }
        Command::Jetpack { enabled } => {
            world.resource_mut::<crate::jetpack::Flight>().requested = *enabled;
            Ok(Pending::Instant)
        }
        Command::AssertFlight {
            airborne,
            altitude,
            tolerance,
        } => {
            if !altitude.is_finite() || !tolerance.is_finite() || !(0.0..=1.0).contains(tolerance) {
                return Err(
                    "Flight assertion requires a finite altitude and tolerance in 0..1".into(),
                );
            }
            let actual = player_position(world).y;
            let flying = world.resource::<crate::jetpack::Flight>().airborne;
            if flying != *airborne || (actual - altitude).abs() > *tolerance {
                return Err(format!(
                    "Flight assertion failed: airborne={flying}, altitude={actual:.3}"
                ));
            }
            Ok(Pending::Instant)
        }
        Command::Scene { scene } => {
            let index = world.resource::<SceneLibrary>().index(scene)?;
            world.resource_mut::<SceneLibrary>().requested = Some(index);
            Ok(Pending::Scene(index))
        }
        Command::Walk { position } => {
            let goal = point(*position)?;
            let start = player_position(world);
            let obstacles = discs(world);
            let route = crate::jetpack::route(
                world.resource::<SceneLibrary>(),
                world.resource::<crate::jetpack::Flight>(),
                start,
                goal,
                &obstacles,
            );
            if route.is_empty() {
                return Err("Destination is blocked or unreachable".into());
            }
            world.resource_mut::<PileLab>().job = None;
            let mut controls = world.resource_mut::<Controls>();
            controls.route = route;
            controls.attack = None;
            controls.queued = None;
            Ok(Pending::Walk(goal))
        }
        Command::Add { dispenser, units } => {
            let mut lab = world.resource_mut::<PileLab>();
            let stock = lab.stock.as_mut().ok_or("No logistics scene loaded")?;
            let pile = stock
                .piles
                .get(dispenser)
                .filter(|p| p.source)
                .ok_or("Unknown dispenser")?;
            if *units == 0 || *units > voxel_assets::PILE_CAPACITY - pile.units {
                return Err("Requested amount does not fit the output pile".into());
            }
            stock.add(*dispenser, *units).map_err(str::to_string)?;
            lab.notice = format!("Program added {units} units.");
            world
                .resource::<crate::sound::Sound>()
                .play(crate::sound::Cue::Dispense);
            Ok(Pending::Instant)
        }
        Command::Grab {
            pile,
            units,
            material,
            seconds_per_unit,
            approach,
        } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            let source = stock.piles.get(pile).ok_or("Unknown pile")?;
            if material.is_some_and(|m| m != source.material)
                || stock.cargo.material.is_some_and(|m| m != source.material)
            {
                return Err("Material mismatch".into());
            }
            if *units == 0
                || *units > source.units
                || *units > stock.cargo.capacity - stock.cargo.units
            {
                return Err(
                    "Exact pickup quantity unavailable or exceeds carrying capacity".into(),
                );
            }
            let expected = stock.cargo.units + units;
            queue_haul(
                world,
                Action::PickupAmount(*pile, *units),
                *seconds_per_unit,
                *approach,
            )?;
            Ok(Pending::Haul {
                expected_cargo: expected,
                serial: world.resource::<PileLab>().job_serial,
            })
        }
        Command::DropAll {
            position,
            seconds_per_unit,
            approach,
        } => {
            if world
                .resource::<PileLab>()
                .stock
                .as_ref()
                .ok_or("No logistics scene loaded")?
                .cargo
                .units
                == 0
            {
                return Err("Carrier is empty".into());
            }
            queue_haul(
                world,
                Action::Drop(point(*position)?),
                *seconds_per_unit,
                *approach,
            )?;
            Ok(Pending::Haul {
                expected_cargo: 0,
                serial: world.resource::<PileLab>().job_serial,
            })
        }
        Command::DepositAll {
            pile,
            seconds_per_unit,
            approach,
        } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            let target = stock.piles.get(pile).ok_or("Unknown pile")?;
            if stock.cargo.units == 0
                || stock.cargo.material != Some(target.material)
                || stock.cargo.units > voxel_assets::PILE_CAPACITY - target.units
            {
                return Err("Cargo is empty, incompatible, or does not fit destination".into());
            }
            queue_haul(world, Action::Deposit(*pile), *seconds_per_unit, *approach)?;
            Ok(Pending::Haul {
                expected_cargo: 0,
                serial: world.resource::<PileLab>().job_serial,
            })
        }
        Command::Strike { target } => {
            let aim = point(*target)? - player_position(world);
            world.resource_mut::<PileLab>().job = None;
            world.resource_mut::<Controls>().strike(aim);
            Ok(Pending::Strike)
        }
        Command::EquipWeapon { weapon } => {
            let selected = match weapon.as_str() {
                "sword" => gameplay_core::Weapon::Sword,
                "spear" => gameplay_core::Weapon::Spear,
                "bow" => gameplay_core::Weapon::Bow,
                "arc_coil" => gameplay_core::Weapon::ArcCoil,
                _ => return Err("Unknown weapon".into()),
            };
            let available = world
                .resource::<crate::settlement::Settlement>()
                .simulation
                .as_ref()
                .is_some_and(|sim| sim.config().weapons.contains(&selected));
            if !available {
                return Err("Weapon is unavailable in this scenario".into());
            }
            world.resource_mut::<Controls>().weapon = selected;
            Ok(Pending::Instant)
        }
        Command::Wait { frames } => {
            if *frames > 7200 {
                return Err("Wait exceeds frame budget".into());
            }
            Ok(Pending::Wait(tick + u64::from(*frames)))
        }
        Command::Assert {
            cargo_units,
            total_units,
            piles,
        } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            if cargo_units.is_some_and(|n| n != stock.cargo.units) {
                return Err(format!(
                    "Cargo assertion failed: actual {}",
                    stock.cargo.units
                ));
            }
            if total_units.is_some_and(|n| n != stock.total()) {
                return Err(format!("Total assertion failed: actual {}", stock.total()));
            }
            for p in piles {
                if stock.piles.get(&p.id).map(|p| p.units) != Some(p.units) {
                    return Err(format!("Pile {} quantity assertion failed", p.id));
                }
            }
            if stock.total() != stock.created {
                return Err("Quantity conservation failed".into());
            }
            Ok(Pending::Instant)
        }
        Command::Quit => Ok(Pending::Instant),
    }
}
fn poll(world: &mut World, pending: &Pending, tick: u64) -> Option<Result<(), String>> {
    match pending {
        Pending::Instant => Some(Ok(())),
        Pending::SettlementTransfer { scene, expected } => {
            if world.resource::<SceneLibrary>().current != *scene {
                return Some(Err("Scene changed during transfer".into()));
            }
            let state = world.resource::<crate::settlement::Settlement>();
            let sim = state.simulation.as_ref()?;
            if sim.manual_jobs.contains_key("player")
                || sim.transfers.values().any(|t| {
                    matches!(&t.from,gameplay_core::Endpoint::Cargo(id) if id=="player")
                        || matches!(&t.to,gameplay_core::Endpoint::Cargo(id) if id=="player")
                })
            {
                None
            } else {
                Some(if sim.agents["player"].cargo.total() == *expected {
                    Ok(())
                } else {
                    Err("Transfer canceled before exact quantity completed".into())
                })
            }
        }
        Pending::Failed(e) => Some(Err(e.clone())),
        Pending::Wait(until) => (tick >= *until).then_some(Ok(())),
        Pending::Scene(index) => {
            (world.resource::<SceneLibrary>().current == *index).then_some(Ok(()))
        }
        Pending::Strike => world
            .resource::<Controls>()
            .attack
            .is_none()
            .then_some(Ok(())),
        Pending::Walk(goal) => {
            if !world.resource::<Controls>().route.is_empty() {
                None
            } else {
                Some(
                    if navigation::flat(player_position(world) - *goal).length() < 0.15 {
                        Ok(())
                    } else {
                        Err("Walk was interrupted".into())
                    },
                )
            }
        }
        Pending::Haul {
            expected_cargo,
            serial,
        } => {
            let lab = world.resource::<PileLab>();
            if lab.job_serial != *serial {
                return Some(Err("Haul was superseded or its scene changed".into()));
            }
            if lab.job.is_some() {
                None
            } else if let Some(e) = &lab.last_error {
                Some(Err(e.clone()))
            } else {
                Some(match &lab.stock {
                    Some(s) if s.cargo.units == *expected_cargo => Ok(()),
                    _ => Err("Haul interrupted or exact quantity not transferred".into()),
                })
            }
        }
    }
}

pub fn snapshot(world: &mut World, hub: &Hub) -> Value {
    let started = std::time::Instant::now();
    let telemetry = crate::telemetry::snapshot(world);
    let settlement = crate::settlement::snapshot(world);
    let style = crate::style::snapshot(world);
    let memory = crate::palimpsest::snapshot(world);
    let position = player_position(world);
    let flight = world.resource::<crate::jetpack::Flight>();
    let jetpack = json!({"requested":flight.requested,"airborne":flight.airborne,"support_height":flight.support,"hover_height":crate::jetpack::HOVER,"clearance":position.y-flight.support});
    let terrain = &world.resource::<SceneLibrary>().scenes
        [world.resource::<SceneLibrary>().current]
        .scene
        .terrain;
    let terrain = json!(terrain);
    let controls = world.resource::<Controls>();
    let movement = json!({"route":controls.route.iter().map(|p|p.to_array()).collect::<Vec<_>>(),"velocity":controls.velocity.to_array(),"attacking":controls.attack.is_some(),"weapon":controls.weapon,"impacts":controls.impacts});
    let scene = world.resource::<SceneLibrary>().title().to_string();
    let lab = world.resource::<PileLab>();
    let airborne = lab.in_flight;
    let (piles, cargo, totals) = if let Some(stock) = &lab.stock {
        (stock.piles.iter().map(|(&id,p)|json!({"id":id,"material":p.material,"position":[p.position[0],0.0,p.position[1]],"units":p.units,"source":p.source,"capacity":voxel_assets::PILE_CAPACITY})).collect::<Vec<_>>(),json!({"material":stock.cargo.material,"units":stock.cargo.units,"capacity":stock.cargo.capacity}),json!({"created":stock.created,"ground":stock.total()-u64::from(stock.cargo.units),"carried":stock.cargo.units,"conserved":stock.total()==stock.created}))
    } else {
        (vec![], Value::Null, Value::Null)
    };
    let mut targets:Vec<_>=world.query::<(&Transform,&ImpactTarget)>().iter(world).map(|(t,target)|json!({"position":t.translation.to_array(),"materials":target.slots.iter().map(|s|json!({"response":s.kind,"flash":s.flash})).collect::<Vec<_>>()})).collect();
    targets.sort_by_key(Value::to_string);
    let obstacles: Vec<_> = world
        .query::<&Obstacle>()
        .iter(world)
        .map(|o| json!({"position":o.0.center.to_array(),"radius":o.0.radius}))
        .collect();
    let flying: Vec<_> = world
        .query::<(&Transform, &crate::piles::Transit)>()
        .iter(world)
        .map(|(transform, transit)| {
            let destination = match transit.destination {
                crate::piles::Destination::Cargo(slot) => json!({"cargo_slot":slot}),
                crate::piles::Destination::Pile(id, slot) => json!({"pile":id,"slot":slot}),
            };
            json!({"position":transform.translation.to_array(),"destination":destination})
        })
        .collect();
    let visible_units = world
        .query::<&crate::piles::MineralUnit>()
        .iter(world)
        .count();
    let result = json!({"telemetry":telemetry,"settlement":settlement,"version":VERSION,"tick":hub.tick,"scene":scene,"player":{"position":position.to_array(),"movement":movement,"cargo":cargo,"jetpack":jetpack},"terrain":terrain,"memory":memory,"style":style,"piles":piles,"totals":totals,"visible_mineral_units":visible_units,"airborne_units":airborne,"airborne":flying,"targets":targets,"obstacles":obstacles,"active_command":hub.active.as_ref().map(|a|&a.envelope.request)});
    crate::telemetry::elapsed(world, "inspection_snapshot", started);
    world
        .resource_mut::<crate::telemetry::Telemetry>()
        .count("snapshot_calls", 1);
    result
}

pub fn begin(world: &mut World) {
    let mut hub = world.remove_resource::<Hub>().expect("control hub");
    hub.tick += 1;
    world
        .resource::<crate::sound::Sound>()
        .set_recording_tick(hub.tick);
    if hub.options.script.is_none() && hub._server.is_none() {
        world.insert_resource(hub);
        return;
    }
    let incoming: Vec<_> = hub.rx.lock().unwrap().try_iter().take(64).collect();
    for envelope in incoming {
        if matches!(envelope.request.command, Command::Inspect) {
            hub.inspect.push(envelope);
        } else if hub.queue.len() < 1024 {
            hub.queue.push_back(envelope);
        } else if let Some(reply) = envelope.reply {
            let _ = reply
                .send(json!({"id":envelope.request.id,"status":"failed","error":"Queue is full"}));
        }
    }
    if hub.tick > hub.warmup && hub.active.is_none() && hub.failed.is_none() {
        if let Some(envelope) = hub.queue.pop_front() {
            let pending =
                start(world, &envelope.request.command, hub.tick).unwrap_or_else(Pending::Failed);
            hub.active = Some(Active {
                envelope,
                pending,
                started: hub.tick,
            });
        }
    }
    world.insert_resource(hub);
}
pub fn end(world: &mut World) {
    let mut hub = world.remove_resource::<Hub>().expect("control hub");
    if hub.options.script.is_none() && hub._server.is_none() {
        world.insert_resource(hub);
        return;
    }
    if let Some(active) = hub.active.take() {
        let result = if hub.tick - active.started > 7200 {
            Some(Err("Command timed out".into()))
        } else {
            poll(world, &active.pending, hub.tick)
        };
        if let Some(result) = result {
            let state = snapshot(world, &hub);
            let receipt = json!({"version":VERSION,"id":active.envelope.request.id,"command":active.envelope.request.command,"status":if result.is_ok(){"completed"}else{"failed"},"error":result.as_ref().err(),"started_tick":active.started,"completed_tick":hub.tick,"state":state});
            if let Some(reply) = active.envelope.reply {
                let _ = reply.send(receipt.clone());
            }
            if active.envelope.scripted && result.is_err() {
                hub.failed = result.err();
                hub.queue.clear();
            }
            if matches!(active.envelope.request.command, Command::Quit) {
                hub.quit = Some(hub.tick + 3);
            }
            hub.receipts.push(receipt);
        } else {
            hub.active = Some(active);
        }
    }
    // Full inspection includes serialized inventories, world geometry, and
    // telemetry. Build it only for a trace, an inspection reply, or a report.
    let finishing = hub.failed.is_some()
        || hub.quit.is_some_and(|tick| hub.tick >= tick)
        || (hub.options.script.is_some()
            && hub.tick > hub.warmup
            && hub.queue.is_empty()
            && hub.active.is_none());
    if hub.trace.is_some() || !hub.inspect.is_empty() || finishing {
        hub.latest = snapshot(world, &hub);
    }
    if hub.trace.is_some() {
        use std::io::Write;
        let record = json!({"frame":hub.tick,"time":hub.tick as f64 / f64::from(hub.options.fps()),"state":hub.latest});
        // Unbuffered newline records can also be consumed through a named pipe.
        writeln!(hub.trace.as_mut().unwrap(), "{record}").expect("write world trace");
    }
    for envelope in hub.inspect.drain(..) {
        if let Some(reply) = envelope.reply {
            let _=reply.send(json!({"version":VERSION,"id":envelope.request.id,"status":"completed","state":hub.latest}));
        }
    }
    if hub.options.script.is_some()
        && hub.tick > hub.warmup
        && hub.queue.is_empty()
        && hub.active.is_none()
        && hub.finished.is_none()
    {
        hub.finished = Some(hub.tick);
    }
    if let Some(script) = &hub.options.script {
        if hub.tick > u64::from(script.max_frames) && hub.finished.is_none() {
            hub.failed = Some("Script exceeded max_frames".into());
            hub.finished = Some(hub.tick);
        }
    }
    let quit = hub.quit.is_some_and(|tick| hub.tick >= tick);
    if hub.failed.is_some() || quit || (hub.finished.is_some() && hub.options.frames_dir.is_none())
    {
        hub.save_report(0);
        world
            .resource_mut::<Messages<AppExit>>()
            .write(if hub.failed.is_some() {
                AppExit::Error(std::num::NonZeroU8::new(1).unwrap())
            } else {
                AppExit::Success
            });
    }
    world.insert_resource(hub);
}

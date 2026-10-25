//! One command path for live clients, deterministic scripts and renderer-free tests.
mod protocol;
#[cfg(test)]
mod tests;
mod transport;
mod video;
use crate::{
    assets::SceneLibrary,
    character::Player, combat::Obstacle, gameplay::Controls,
    impact::ImpactTarget,
    lockstep::Game,
    navigation,
    piles::PileLab,
};
use sim::settlement::piles::PileAction;
use sim::CommandKind;
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
    /// Show the launch menu before starting an interactive session.
    pub menu: bool,
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
            menu: false,
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
    /// Done once the command has been simulated.
    Applied,
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
/// The simulated settlement, or an error for scenes without one.
fn settlement(world: &World) -> Result<&sim::settlement::Settlement, String> {
    if world.resource::<crate::settlement::Settlement>().config.is_none() {
        return Err("No settlement loaded".into());
    }
    world.resource::<Game>().settlement().ok_or_else(|| "No settlement loaded".to_string())
}
fn send(world: &mut World, kind: CommandKind) {
    world.resource_mut::<Game>().command(kind);
}
/// Starts a haul toward a pile. The job serial lets the poll tell this job
/// from a later one.
fn queue_haul(world: &mut World, action: PileAction, pace: f32, approach: Option<[f32; 2]>) -> Result<u64, String> {
    if !(0.0..=5.0).contains(&pace) {
        return Err("Invalid transfer interval".into());
    }
    let stand = approach.map(point).transpose()?;
    let serial = world.resource::<PileLab>().job_serial + 1;
    crate::piles::haul(&mut world.resource_mut::<Game>(), action, pace, stand);
    Ok(serial)
}
fn start(world: &mut World, command: &Command, tick: u64) -> Result<Pending, String> {
    match command {
        Command::Inspect => Ok(Pending::Instant),
        Command::InspectBuilding { building } => {
            crate::settlement::inspect_building(world, building.as_deref())?;
            Ok(Pending::Instant)
        }
        Command::SettlementTransfer { depot, item, units, deposit } => {
            let expected = crate::settlement::player_transfer(world, depot, item, *units, *deposit)?;
            Ok(Pending::SettlementTransfer { scene: world.resource::<SceneLibrary>().current, expected })
        }
        Command::SettlementCancel => {
            settlement(world)?;
            let agent = world.resource::<Game>().player().ok_or("No controlled actor")?;
            send(world, CommandKind::CancelTransfer { agent });
            Ok(Pending::Instant)
        }
        Command::SettlementPause { paused } => {
            settlement(world)?;
            if world.resource::<Game>().network.is_some() {
                return Err("Networked games can't pause".into());
            }
            world.resource_mut::<Game>().paused = *paused;
            Ok(Pending::Instant)
        }
        Command::SettlementRate { rate } => {
            settlement(world)?;
            if world.resource::<Game>().network.is_some() {
                return Err("Networked games run at one speed".into());
            }
            if !rate.is_finite() || !(0.25..=4.0).contains(rate) {
                return Err("simulation rate must be 0.25..4".into());
            }
            world.resource_mut::<Game>().rate = *rate;
            Ok(Pending::Instant)
        }
        Command::SettlementLink { id, enabled } => {
            let link = settlement(world)?.link_index(id).ok_or("unknown haul link")?;
            send(world, CommandKind::SetLink { link, enabled: *enabled });
            Ok(Pending::Instant)
        }
        Command::SettlementMachine { id, enabled } => {
            let producer = settlement(world)?.producer_index(id).ok_or("unknown producer")?;
            send(world, CommandKind::SetMachine { producer, enabled: *enabled });
            Ok(Pending::Instant)
        }
        Command::SettlementDefender { id, enabled } => {
            let s = settlement(world)?;
            let defender = s.defenders.iter().position(|d| d.id == *id).ok_or("unknown defender")? as u16;
            send(world, CommandKind::SetDefender { defender, enabled: *enabled });
            Ok(Pending::Instant)
        }
        Command::SettlementStock { depot, item, units } => {
            let s = settlement(world)?;
            let (Some(d), Some(i)) = (s.depot_index(depot), s.item_index(item)) else {
                return Err("unknown depot/item or zero amount".into());
            };
            if *units == 0 {
                return Err("unknown depot/item or zero amount".into());
            }
            if *units > s.depots[d as usize].free() {
                return Err("stock does not fit unreserved destination capacity".into());
            }
            send(world, CommandKind::AddStock { depot: d, item: i, units: *units });
            Ok(Pending::Applied)
        }
        Command::AssertSettlement { delivered, cycles, equipped, kills } => {
            let state = world.resource::<crate::settlement::Settlement>();
            let sim = state.simulation.as_ref().ok_or("No settlement loaded")?;
            sim.validate_state()?;
            if sim.metrics.delivered < *delivered || sim.metrics.cycles < *cycles || sim.metrics.equipped < *equipped || sim.metrics.kills < *kills {
                return Err(format!("Supply chain milestones not reached: {:?}", sim.metrics));
            }
            Ok(Pending::Instant)
        }
        Command::MemoryRecord => {
            crate::palimpsest::record(world)?;
            Ok(Pending::Applied)
        }
        Command::MemoryDeploy => {
            crate::palimpsest::deploy(world)?;
            Ok(Pending::Applied)
        }
        Command::MemoryClear => {
            crate::palimpsest::clear(world);
            Ok(Pending::Applied)
        }
        Command::AssertTrial { unlocked, collected } => {
            let memory = world.resource::<crate::palimpsest::Memory>();
            if memory.unlocked != *unlocked || memory.collected != *collected {
                return Err("Field trial assertion failed".into());
            }
            Ok(Pending::Instant)
        }
        Command::Jetpack { enabled } => {
            let agent = world.resource::<Game>().player().ok_or("No player")?;
            send(world, CommandKind::Jetpack { agent, enabled: *enabled });
            Ok(Pending::Applied)
        }
        Command::Deliver { seconds_per_unit } => {
            let carried = world.resource::<PileLab>().stock.as_ref().ok_or("No logistics scene loaded")?.cargo.units;
            if carried == 0 {
                return Err("Carrier is empty".into());
            }
            let serial = queue_haul(world, PileAction::Deliver, *seconds_per_unit, None)?;
            Ok(Pending::Haul { expected_cargo: 0, serial })
        }
        Command::Dig => {
            let game = world.resource::<Game>();
            if !game.world().is_some_and(|w| w.terrain().diggable) {
                return Err("This scene has no diggable ground".into());
            }
            world.resource_mut::<Game>().dig();
            Ok(Pending::Applied)
        }
        Command::AssertFlight { airborne, altitude, tolerance } => {
            if !altitude.is_finite() || !tolerance.is_finite() || !(0.0..=1.0).contains(tolerance) {
                return Err("Flight assertion requires a finite altitude and tolerance in 0..1".into());
            }
            let game = world.resource::<Game>();
            let a = game.settlement().zip(game.player()).map(|(s, p)| &s.agents[p as usize]).ok_or("No player")?;
            let (flying, actual) = (a.avatar.flight.airborne, a.avatar.height.to_f32());
            if flying != *airborne || (actual - altitude).abs() > *tolerance {
                return Err(format!("Flight assertion failed: airborne={flying}, altitude={actual:.3}"));
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
            let mut controls = world.resource_mut::<Controls>();
            controls.attack = None;
            controls.queued = None;
            world.resource_mut::<Game>().go_to(goal, false);
            Ok(Pending::Walk(goal))
        }
        Command::Add { dispenser, units } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            let pile = stock.piles.get(dispenser).filter(|p| p.source).ok_or("Unknown dispenser")?;
            if *units == 0 || *units > voxel_assets::PILE_CAPACITY - pile.units {
                return Err("Requested amount does not fit the output pile".into());
            }
            send(world, CommandKind::Dispense { dispenser: *dispenser, units: *units });
            world.resource_mut::<PileLab>().notice = format!("Program added {units} units.");
            world.resource::<crate::sound::Sound>().play(crate::sound::Cue::Dispense);
            Ok(Pending::Applied)
        }
        Command::Grab { pile, units, material, seconds_per_unit, approach } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            let source = stock.piles.get(pile).ok_or("Unknown pile")?;
            if material.is_some_and(|m| m != source.material) || stock.cargo.material.is_some_and(|m| m != source.material) {
                return Err("Material mismatch".into());
            }
            if *units == 0 || *units > source.units || *units > stock.cargo.capacity - stock.cargo.units {
                return Err("Exact pickup quantity unavailable or exceeds carrying capacity".into());
            }
            let expected = stock.cargo.units + units;
            let serial = queue_haul(world, PileAction::PickupAmount(*pile, *units), *seconds_per_unit, *approach)?;
            Ok(Pending::Haul { expected_cargo: expected, serial })
        }
        Command::DropAll { position, seconds_per_unit, approach } => {
            let carried = world.resource::<PileLab>().stock.as_ref().ok_or("No logistics scene loaded")?.cargo.units;
            if carried == 0 {
                return Err("Carrier is empty".into());
            }
            let at = world.resource::<Game>().to_sim(point(*position)?);
            let serial = queue_haul(world, PileAction::Drop(at), *seconds_per_unit, *approach)?;
            Ok(Pending::Haul { expected_cargo: 0, serial })
        }
        Command::DepositAll { pile, seconds_per_unit, approach } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            let target = stock.piles.get(pile).ok_or("Unknown pile")?;
            if stock.cargo.units == 0 || stock.cargo.material != Some(target.material) || stock.cargo.units > voxel_assets::PILE_CAPACITY - target.units {
                return Err("Cargo is empty, incompatible, or does not fit destination".into());
            }
            let serial = queue_haul(world, PileAction::Deposit(*pile), *seconds_per_unit, *approach)?;
            Ok(Pending::Haul { expected_cargo: 0, serial })
        }
        Command::Strike { target } => {
            let aim = point(*target)? - player_position(world);
            world.resource_scope(|world, mut game: Mut<Game>| {
                world.resource_scope(|world, mut controls: Mut<Controls>| {
                    crate::lockstep::strike(&mut game, &mut controls, world.resource::<SceneLibrary>(), aim);
                });
            });
            Ok(Pending::Strike)
        }
        Command::EquipWeapon { weapon } => {
            use content::scenario::Weapon;
            let selected = match weapon.as_str() {
                "sword" => Weapon::Sword,
                "spear" => Weapon::Spear,
                "bow" => Weapon::Bow,
                "arc_coil" => Weapon::ArcCoil,
                _ => return Err("Unknown weapon".into()),
            };
            let available = world.resource::<Game>().settlement().is_some_and(|s| s.weapons.contains(&selected));
            if !available {
                return Err("Weapon is unavailable in this scenario".into());
            }
            world.resource_mut::<Controls>().weapon = selected;
            world.resource_mut::<Game>().equip(selected);
            Ok(Pending::Instant)
        }
        Command::Wait { frames } => {
            if *frames > 7200 {
                return Err("Wait exceeds frame budget".into());
            }
            Ok(Pending::Wait(tick + u64::from(*frames)))
        }
        Command::Assert { cargo_units, total_units, piles } => {
            let lab = world.resource::<PileLab>();
            let stock = lab.stock.as_ref().ok_or("No logistics scene loaded")?;
            if cargo_units.is_some_and(|n| n != stock.cargo.units) {
                return Err(format!("Cargo assertion failed: actual {}", stock.cargo.units));
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
    // Commands apply on the next simulated tick; until then there is nothing to check.
    let applied = world.resource::<Game>().caught_up();
    match pending {
        Pending::Instant => Some(Ok(())),
        Pending::Applied => applied.then_some(Ok(())),
        Pending::SettlementTransfer { scene, expected } => {
            if world.resource::<SceneLibrary>().current != *scene {
                return Some(Err("Scene changed during transfer".into()));
            }
            if !applied {
                return None;
            }
            let game = world.resource::<Game>();
            let a = &game.settlement()?.agents[game.player()? as usize];
            if a.avatar.job.is_some() || a.pending_in > 0 || a.pending_out > 0 {
                None
            } else {
                Some(if a.cargo.iter().sum::<u32>() == *expected {
                    Ok(())
                } else {
                    Err("Transfer canceled before exact quantity completed".into())
                })
            }
        }
        Pending::Failed(e) => Some(Err(e.clone())),
        Pending::Wait(until) => (tick >= *until).then_some(Ok(())),
        Pending::Scene(index) => (world.resource::<SceneLibrary>().current == *index && applied_scene(world, *index)).then_some(Ok(())),
        Pending::Strike => (applied && world.resource::<Controls>().attack.is_none()).then_some(Ok(())),
        Pending::Walk(goal) => {
            if !applied {
                return None;
            }
            let game = world.resource::<Game>();
            let a = &game.settlement()?.agents[game.player()? as usize];
            if !a.route.is_empty() {
                None
            } else if navigation::flat(game.to_view(a.position) - *goal).length() < 0.15 {
                Some(Ok(()))
            } else {
                Some(Err("Destination is blocked or unreachable, or the walk was interrupted".into()))
            }
        }
        Pending::Haul { expected_cargo, serial } => {
            if !applied {
                return None;
            }
            let lab = world.resource::<PileLab>();
            if lab.job_serial != *serial {
                return Some(Err(lab.last_error.clone().unwrap_or_else(|| "Haul was superseded or its scene changed".into())));
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
/// Whether the scene's world exists and has run a tick.
fn applied_scene(world: &World, index: usize) -> bool {
    let _ = index;
    world.resource::<Game>().world().is_some_and(|w| w.tick() > 0)
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
    let digging = crate::digging::snapshot(world.resource::<Game>());
    let controls = world.resource::<Controls>();
    let movement = json!({"route":controls.route.iter().map(|p|p.to_array()).collect::<Vec<_>>(),"velocity":controls.velocity.to_array(),"attacking":controls.attack.is_some(),"weapon":controls.weapon,"impacts":controls.impacts});
    let scene = world.resource::<SceneLibrary>().title().to_string();
    let lab = world.resource::<PileLab>();
    let airborne = lab.in_flight;
    let (piles, cargo, totals) = if let Some(stock) = &lab.stock {
        (stock.piles.iter().map(|(&id,p)|json!({"id":id,"material":p.material,"position":[p.position[0],0.0,p.position[1]],"units":p.units,"source":p.source,"capacity":voxel_assets::PILE_CAPACITY})).collect::<Vec<_>>(),json!({"material":stock.cargo.material,"units":stock.cargo.units,"capacity":stock.cargo.capacity}),json!({"created":stock.created,"ground":stock.ground(),"carried":stock.cargo.units,"conserved":stock.total()==stock.created}))
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
                crate::piles::Destination::Cargo(agent, slot) => json!({"agent":agent,"cargo_slot":slot}),
                crate::piles::Destination::Pile(id, slot) => json!({"pile":id,"slot":slot}),
                crate::piles::Destination::Collector(_) => json!({"collector":true}),
            };
            json!({"position":transform.translation.to_array(),"destination":destination})
        })
        .collect();
    let visible_units = world
        .query::<&crate::piles::MineralUnit>()
        .iter(world)
        .count();
    let sim_tick = world.resource::<Game>().world().map(|w| w.tick());
    let network = world.resource::<Game>().network.is_some().then(|| world.resource::<Game>().status.clone());
    let peers = crate::peers::snapshot(world);
    let result = json!({"telemetry":telemetry,"settlement":settlement,"version":VERSION,"tick":hub.tick,"sim_tick":sim_tick,"network":network,"peers":peers,"scene":scene,"player":{"position":position.to_array(),"movement":movement,"cargo":cargo,"jetpack":jetpack},"terrain":terrain,"digging":digging,"memory":memory,"style":style,"piles":piles,"totals":totals,"visible_mineral_units":visible_units,"airborne_units":airborne,"airborne":flying,"targets":targets,"obstacles":obstacles,"active_command":hub.active.as_ref().map(|a|&a.envelope.request)});
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
    if hub.options.trace.is_some() || !hub.inspect.is_empty() || finishing {
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

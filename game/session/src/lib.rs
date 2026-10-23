//! A game session: this machine's peer in the lockstep protocol, or the
//! replay of an input log. Single-player is a host with no clients, so every
//! game runs through the same turn path.
//!
//! Callers pass the time, so a scripted run on a fixed clock is as
//! deterministic as the simulation.

use std::net::SocketAddr;
use std::time::Duration;

use content::Content;
use net::transport::Transport;
use net::{Client, ClientPhase, Host, NetConfig, NetEvent, NetStats, NullTransport, StableId, UdpTransport};
use save::InputLog;
use sim::{Command, CommandKind, PlayerId, Simulation, World};

type Socket = Box<dyn Transport<Addr = SocketAddr> + Send + Sync>;

/// Every peer autosaves on the same ticks, so after losing the host anyone
/// can re-host from a state everyone shares.
pub const AUTOSAVE_TICKS: u64 = 30 * 60 * 5;

enum Peer {
    Host(Box<Host<Socket>>),
    Client(Box<Client<Socket>>),
    /// Plays an input log back without a network.
    Replay { sim: Box<Simulation>, log: InputLog, pacer: Duration, next: Duration },
}

pub struct Session {
    peer: Peer,
    content: Content,
    /// Things for the app to tell the player about or act on.
    pub events: Vec<NetEvent>,
    /// Autosaves: write them where the app keeps saves.
    pub autosave: Option<Vec<u8>>,
}

pub enum Mode {
    /// Host a game. `listen` is `None` for single-player.
    Host { world: World, listen: Option<SocketAddr> },
    Join { host: SocketAddr, stable_id: StableId },
    /// Play back `log` from the save it starts at.
    Replay { world: World, log: InputLog },
}

impl Session {
    pub fn start(mode: Mode, content: Content, config: NetConfig) -> std::io::Result<Session> {
        let peer = match mode {
            Mode::Host { world, listen } => {
                let socket: Socket = match listen {
                    Some(addr) => Box::new(UdpTransport::bind(addr)?),
                    None => Box::new(NullTransport),
                };
                // A new epoch per hosting session, so stray packets from an
                // earlier session are ignored.
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_secs() as u32 | 1);
                Peer::Host(Box::new(Host::new(socket, world, content.clone(), config, epoch, Duration::ZERO)))
            }
            Mode::Join { host, stable_id } => {
                let any: SocketAddr = if host.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse().unwrap();
                let socket: Socket = Box::new(UdpTransport::bind(any)?);
                Peer::Client(Box::new(Client::new(socket, host, content.clone(), config, stable_id, Duration::ZERO)))
            }
            Mode::Replay { world, log } => Peer::Replay {
                sim: Box::new(Simulation::from_world(world, config.threads)),
                log,
                pacer: net::TICK,
                next: Duration::ZERO,
            },
        };
        Ok(Session { peer, content, events: Vec::new(), autosave: None })
    }

    /// The world, once there is one (a joining client has none until loaded).
    pub fn world(&self) -> Option<&World> {
        match &self.peer {
            Peer::Host(host) => Some(host.world()),
            Peer::Client(client) => client.world(),
            Peer::Replay { sim, .. } => Some(sim.world()),
        }
    }

    /// The local player's ID in the turn path.
    pub fn player(&self) -> Option<PlayerId> {
        match &self.peer {
            Peer::Host(_) => Some(net::HOST_PLAYER),
            Peer::Client(client) => client.player(),
            Peer::Replay { .. } => None,
        }
    }

    /// Queues a command from the local player. Replays ignore new input.
    pub fn queue(&mut self, kind: CommandKind) {
        match &mut self.peer {
            Peer::Host(host) => host.queue(kind),
            Peer::Client(client) => client.queue(kind),
            Peer::Replay { .. } => {}
        }
    }

    /// Runs the network and every tick due by `now`, calling `on_step` after
    /// each tick. Returns how many ticks ran.
    pub fn update(&mut self, now: Duration, mut on_step: impl FnMut(&World)) -> u64 {
        let mut stepped = 0;
        let mut autosave = None;
        let content = &self.content;
        let mut step = |world: &World| {
            stepped += 1;
            if world.tick() % AUTOSAVE_TICKS == 0 {
                autosave = Some(save::write(world, content));
            }
            on_step(world);
        };
        match &mut self.peer {
            Peer::Host(host) => {
                host.update(now, &mut step);
                self.events.extend(host.take_events());
            }
            Peer::Client(client) => {
                client.update(now, &mut step);
                self.events.extend(client.take_events());
            }
            Peer::Replay { sim, log, pacer, next } => {
                while now >= *next && sim.world().tick() < log.end_tick() {
                    let tick = sim.world().tick();
                    sim.step(log.turn(tick));
                    step(sim.world());
                    *next += *pacer;
                }
            }
        }
        if autosave.is_some() {
            self.autosave = autosave;
        }
        stepped
    }

    /// How far render time is between the last two ticks.
    pub fn alpha(&self, now: Duration) -> f32 {
        match &self.peer {
            Peer::Host(host) => host.alpha(now),
            Peer::Client(client) => client.alpha(now),
            Peer::Replay { pacer, next, .. } => {
                let since = now.saturating_sub(next.saturating_sub(*pacer));
                (since.as_secs_f32() / pacer.as_secs_f32()).min(1.0)
            }
        }
    }

    pub fn input_delay(&self) -> u64 {
        match &self.peer {
            Peer::Host(host) => host.input_delay(),
            Peer::Client(client) => client.input_delay(),
            Peer::Replay { .. } => 0,
        }
    }

    pub fn stats(&self) -> NetStats {
        match &self.peer {
            Peer::Host(host) => host.stats().clone(),
            Peer::Client(client) => client.stats(),
            Peer::Replay { .. } => NetStats::default(),
        }
    }

    /// Whether a replay has run out of turns.
    pub fn finished(&self) -> bool {
        matches!(&self.peer, Peer::Replay { sim, log, .. } if sim.world().tick() >= log.end_tick())
    }

    /// One line for the HUD about the connection.
    pub fn status(&self) -> String {
        match &self.peer {
            Peer::Host(host) => {
                let players = host.players().len();
                match host.halted() {
                    Some(tick) => format!("HALTED: desync at tick {tick}"),
                    None if players > 1 => format!("hosting, {players} players"),
                    None => "single player".to_string(),
                }
            }
            Peer::Client(client) => match client.phase() {
                ClientPhase::Connecting => "connecting…".to_string(),
                ClientPhase::Loading { received, total } => format!("loading save {received}/{total}"),
                ClientPhase::Playing => format!("playing as {:?}", client.player().unwrap()),
                ClientPhase::Stopped { reason } => format!("stopped: {reason}"),
            },
            Peer::Replay { sim, log, .. } => format!("replay, tick {} of {}", sim.world().tick(), log.end_tick()),
        }
    }

    pub fn save(&self) -> Option<Vec<u8>> {
        self.world().map(|world| save::write(world, &self.content))
    }

    /// The host's turns so far, as an input log from the session's first tick.
    pub fn input_log(&self) -> Option<InputLog> {
        match &self.peer {
            Peer::Host(host) => {
                let (start, turns) = host.turns();
                Some(InputLog { start_tick: start, turns: turns.to_vec() })
            }
            _ => None,
        }
    }

    /// This peer's state and hash log, as files to keep after a desync.
    pub fn desync_evidence(&self) -> (Option<Vec<u8>>, String) {
        let log = match &self.peer {
            Peer::Host(host) => host.history().clone(),
            Peer::Client(client) => client.history().clone(),
            Peer::Replay { .. } => return (None, String::new()),
        };
        let text = (log.start()..log.end()).map(|t| format!("{t} {:016x}\n", log.get(t).unwrap())).collect();
        (self.save(), text)
    }

    pub fn is_host(&self) -> bool {
        matches!(self.peer, Peer::Host(_))
    }

    pub fn shutdown(&mut self) {
        match &mut self.peer {
            Peer::Host(host) => host.shutdown(),
            Peer::Client(client) => client.shutdown(),
            Peer::Replay { .. } => {}
        }
    }
}

/// Commands that came from a turn, for callers that record or inspect them.
pub fn commands(log: &InputLog) -> impl Iterator<Item = (u64, &Command)> {
    log.turns.iter().enumerate().flat_map(move |(i, turn)| turn.iter().map(move |c| (log.start_tick + i as u64, c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx::{Fx, FxVec2};

    fn single_player() -> Session {
        let content = Content::builtin();
        let world = World::new(sim::scenario::Scenario::small().config, content.rules.clone());
        Session::start(Mode::Host { world, listen: None }, content, NetConfig { threads: 2, ..NetConfig::default() }).unwrap()
    }

    #[test]
    fn single_player_runs_through_the_turn_path() {
        let mut session = single_player();
        session.queue(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(40), Fx::from_int(40)), count: 5 });
        let mut now = Duration::ZERO;
        while session.world().unwrap().mover_count() == 0 && now < Duration::from_secs(5) {
            now += Duration::from_millis(5);
            session.update(now, |_| {});
        }
        let world = session.world().unwrap();
        assert_eq!(world.mover_count(), 5);
        // The command waited for its tick.
        assert!(world.tick() > session.input_delay());
    }

    /// A recorded session replays to the same state.
    #[test]
    fn input_logs_replay_exactly() {
        let content = Content::builtin();
        let mut session = single_player();
        let start = save::write(session.world().unwrap(), &content);
        session.queue(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(40), Fx::from_int(40)), count: 30 });
        let mut now = Duration::ZERO;
        while now < Duration::from_secs(3) {
            now += Duration::from_millis(10);
            session.update(now, |_| {});
        }
        let log = session.input_log().unwrap();
        let original = save::write(session.world().unwrap(), &content);

        let world = save::read(&start, &content).unwrap();
        let mut replay = Session::start(Mode::Replay { world, log }, content.clone(), NetConfig { threads: 2, ..NetConfig::default() }).unwrap();
        replay.update(Duration::from_secs(60), |_| {});
        assert!(replay.finished());
        assert_eq!(save::write(replay.world().unwrap(), &content), original);
    }
}

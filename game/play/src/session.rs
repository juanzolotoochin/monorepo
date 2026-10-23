//! The game session: this machine's peer in the lockstep protocol.
//! Single-player is a host with no clients, so every game runs through the
//! same turn path.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bevy::prelude::Resource;
use content::Content;
use net::transport::Transport;
use net::{Client, ClientPhase, Host, NetConfig, NetEvent, NetStats, NullTransport, StableId, UdpTransport};
use sim::{CommandKind, World};
use view::Extract;

type Socket = Box<dyn Transport<Addr = SocketAddr> + Send + Sync>;

/// Every peer autosaves on the same ticks, so after losing the host anyone
/// can re-host from a state everyone shares.
pub const AUTOSAVE_TICKS: u64 = 30 * 60 * 5;

enum Peer {
    Host(Host<Socket>),
    Client(Client<Socket>),
}

#[derive(Resource)]
pub struct Session {
    peer: Peer,
    content: Content,
    start: Instant,
    last_update_ms: f64,
    /// Things for the app to tell the player about or act on.
    pub events: Vec<NetEvent>,
}

pub enum Mode {
    /// Host a game. `listen` is `None` for single-player.
    Host { world: World, listen: Option<SocketAddr> },
    Join { host: SocketAddr, stable_id: StableId },
}

impl Session {
    pub fn start(mode: Mode, content: Content, config: NetConfig) -> std::io::Result<Session> {
        let start = Instant::now();
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
                Peer::Host(Host::new(socket, world, content.clone(), config, epoch, Duration::ZERO))
            }
            Mode::Join { host, stable_id } => {
                let any: SocketAddr = if host.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse().unwrap();
                let socket: Socket = Box::new(UdpTransport::bind(any)?);
                Peer::Client(Client::new(socket, host, content.clone(), config, stable_id, Duration::ZERO))
            }
        };
        Ok(Session { peer, content, start, last_update_ms: 0.0, events: Vec::new() })
    }

    /// The world, once there is one (a joining client has none until loaded).
    pub fn world(&self) -> Option<&World> {
        match &self.peer {
            Peer::Host(host) => Some(host.world()),
            Peer::Client(client) => client.world(),
        }
    }

    pub fn queue(&mut self, kind: CommandKind) {
        match &mut self.peer {
            Peer::Host(host) => host.queue(kind),
            Peer::Client(client) => client.queue(kind),
        }
    }

    /// Runs the network and every tick due. Returns the extract of the last
    /// tick run, if any.
    pub fn update(&mut self) -> Option<Extract> {
        let now = self.start.elapsed();
        let started = Instant::now();
        let mut stepped = false;
        let mut autosave = None;
        let content = &self.content;
        let mut on_step = |world: &World| {
            stepped = true;
            if world.tick() % AUTOSAVE_TICKS == 0 {
                autosave = Some(save::write(world, content));
            }
        };
        match &mut self.peer {
            Peer::Host(host) => {
                host.update(now, &mut on_step);
                self.events.extend(host.take_events());
            }
            Peer::Client(client) => {
                client.update(now, &mut on_step);
                self.events.extend(client.take_events());
            }
        }
        if let Some(bytes) = autosave {
            write_file("autosave.gsav", &bytes);
        }
        if stepped {
            self.last_update_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        let lost = self.events.iter().any(|e| matches!(e, NetEvent::HostLost));
        if lost {
            if let Some(world) = self.world() {
                write_file("hostlost.gsav", &save::write(world, &self.content));
            }
        }
        if stepped {
            self.world().map(Extract::from_world)
        } else {
            None
        }
    }

    pub fn alpha(&self) -> f32 {
        let now = self.start.elapsed();
        match &self.peer {
            Peer::Host(host) => host.alpha(now),
            Peer::Client(client) => client.alpha(now),
        }
    }

    pub fn input_delay(&self) -> u64 {
        match &self.peer {
            Peer::Host(host) => host.input_delay(),
            Peer::Client(client) => client.input_delay(),
        }
    }

    pub fn last_update_ms(&self) -> f64 {
        self.last_update_ms
    }

    pub fn stats(&self) -> NetStats {
        match &self.peer {
            Peer::Host(host) => host.stats().clone(),
            Peer::Client(client) => client.stats(),
        }
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
        }
    }

    pub fn save(&self) -> Option<Vec<u8>> {
        self.world().map(|world| save::write(world, &self.content))
    }

    /// Writes the evidence for a desync: this peer's state and hash log.
    pub fn dump_desync(&self, tick: u64) {
        let role = match &self.peer {
            Peer::Host(_) => "host",
            Peer::Client(_) => "client",
        };
        if let Some(bytes) = self.save() {
            write_file(&format!("desync-{tick}-{role}.gsav"), &bytes);
        }
        let log = match &self.peer {
            Peer::Host(host) => host.history().clone(),
            Peer::Client(client) => client.history().clone(),
        };
        let text: String = (log.start()..log.end()).map(|t| format!("{t} {:016x}\n", log.get(t).unwrap())).collect();
        write_file(&format!("desync-{tick}-{role}.hashes"), text.as_bytes());
    }

    pub fn shutdown(&mut self) {
        match &mut self.peer {
            Peer::Host(host) => host.shutdown(),
            Peer::Client(client) => client.shutdown(),
        }
    }
}

fn write_file(path: &str, bytes: &[u8]) {
    match std::fs::write(path, bytes) {
        Ok(()) => bevy::log::info!("wrote {path}"),
        Err(e) => bevy::log::error!("could not write {path}: {e}"),
    }
}

/// This machine's stable player ID, kept in `profile.id` next to the game.
pub fn local_stable_id() -> StableId {
    const PATH: &str = "profile.id";
    if let Some(id) = std::fs::read_to_string(PATH).ok().and_then(|s| s.trim().parse().ok()) {
        return id;
    }
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
        ^ (std::process::id() as u64) << 32;
    let id = sim::Rng::new(seed, 0, 0, 0).next_u64();
    let _ = std::fs::write(PATH, id.to_string());
    id
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
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.world().unwrap().mover_count() == 0 && Instant::now() < deadline {
            session.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        let world = session.world().unwrap();
        assert_eq!(world.mover_count(), 5);
        // The command waited for its tick.
        assert!(world.tick() > session.input_delay());
    }
}

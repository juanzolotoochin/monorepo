//! A game session: this machine's peer in the lockstep protocol, or the
//! replay of an input log. Single-player is a host with no clients, so every
//! game runs through the same turn path.
//!
//! Callers pass the time, so a scripted run on a fixed clock is as
//! deterministic as the simulation.
//!
//! A client that finishes loading asks for an avatar (`CommandKind::Join`);
//! the host's avatar comes with the scene.

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
pub const AUTOSAVE_TICKS: u64 = content::TICKS_PER_SECOND as u64 * 60 * 5;

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
    /// The address a networked host listens on.
    local_addr: Option<SocketAddr>,
}

pub enum Mode {
    /// Host a game. `listen` is `None` for single-player. `label` is the
    /// app's name for the world (its scene), passed on to clients.
    Host { world: World, label: String, listen: Option<SocketAddr> },
    Join { host: SocketAddr, stable_id: StableId },
    /// Play back `log` from the save it starts at.
    Replay { world: World, log: InputLog },
}

impl Session {
    pub fn start(mode: Mode, content: Content, config: NetConfig) -> std::io::Result<Session> {
        let mut local_addr = None;
        let peer = match mode {
            Mode::Host { world, label, listen } => {
                let socket: Socket = match listen {
                    Some(addr) => {
                        let udp = UdpTransport::bind(addr)?;
                        local_addr = Some(udp.local_addr()?);
                        Box::new(udp)
                    }
                    None => Box::new(NullTransport),
                };
                // A new epoch per hosting session, so stray packets from an
                // earlier session are ignored.
                let epoch = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_secs() as u32 | 1);
                Peer::Host(Box::new(Host::new(socket, world, label, content.clone(), config, epoch, Duration::ZERO)))
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
        Ok(Session { peer, content, events: Vec::new(), autosave: None, local_addr })
    }

    /// The world, once there is one (a joining client has none until loaded).
    pub fn world(&self) -> Option<&World> {
        match &self.peer {
            Peer::Host(host) => Some(host.world()),
            Peer::Client(client) => client.world(),
            Peer::Replay { sim, .. } => Some(sim.world()),
        }
    }

    /// Replaces the host's world with a new one (a new map, at tick 0) that
    /// the app calls `label`. Clients load it from a save, and it starts once
    /// they all have it. Returns false, doing nothing, on a client or replay.
    pub fn load_world(&mut self, world: World, label: String, now: Duration) -> bool {
        match &mut self.peer {
            Peer::Host(host) => {
                host.load_world(world, label, now);
                true
            }
            _ => false,
        }
    }

    /// Counts the worlds this session has gone through, from 1; 0 for a
    /// client before its first. It changes when the world is replaced by a
    /// new one, not when a client resyncs.
    pub fn world_number(&self) -> u32 {
        match &self.peer {
            Peer::Host(host) => host.world_number(),
            Peer::Client(client) => client.world_number(),
            Peer::Replay { .. } => 1,
        }
    }

    /// The host app's name for the current world, once known.
    pub fn world_label(&self) -> Option<&str> {
        match &self.peer {
            Peer::Host(host) => Some(host.label()),
            Peer::Client(client) if client.world_number() > 0 => Some(client.label()),
            _ => None,
        }
    }

    /// Whether the world is still loading: a client fetching the host's save,
    /// or a host waiting for its clients to load a new world.
    pub fn world_loading(&self) -> bool {
        match &self.peer {
            Peer::Host(host) => host.starting(),
            Peer::Client(client) => matches!(client.phase(), ClientPhase::Connecting | ClientPhase::Loading { .. }),
            Peer::Replay { .. } => false,
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

    pub fn is_replay(&self) -> bool {
        matches!(self.peer, Peer::Replay { .. })
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
                for event in client.take_events() {
                    if matches!(event, NetEvent::Loaded { .. }) {
                        client.queue(CommandKind::Join);
                    }
                    self.events.push(event);
                }
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

    /// Commands this peer queued that haven't been simulated yet.
    pub fn pending_inputs(&self) -> u64 {
        match &self.peer {
            Peer::Host(host) => host.pending_inputs(),
            Peer::Client(client) => client.pending_inputs(),
            Peer::Replay { .. } => 0,
        }
    }

    /// A client's commands that haven't been simulated yet, oldest first,
    /// each with the tick it asked for. A host's apply on its next tick and
    /// a replay takes no input, so they have none.
    pub fn pending_commands(&self) -> impl Iterator<Item = (u64, &CommandKind)> + '_ {
        let client = match &self.peer {
            Peer::Client(client) => Some(client.pending_commands()),
            _ => None,
        };
        client.into_iter().flatten()
    }

    /// The tick a client's command queued now asks for; `None` on a host or
    /// replay.
    pub fn input_horizon(&self) -> Option<u64> {
        match &self.peer {
            Peer::Client(client) => Some(client.input_horizon()),
            _ => None,
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

    /// Networking observations on this peer's caller-supplied clock.
    pub fn diagnostics(&self, now: Duration) -> net::NetDiagnostics {
        match &self.peer {
            Peer::Host(host) => host.diagnostics(now),
            Peer::Client(client) => client.diagnostics(now),
            Peer::Replay { .. } => net::NetDiagnostics::default(),
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

    /// The host's turns in the current world, as an input log from its first tick.
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

    /// The address a networked host listens on.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
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
        Session::start(Mode::Host { world, label: "small".into(), listen: None }, content, NetConfig { threads: 2, ..NetConfig::default() }).unwrap()
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

    /// The networking MVP: a client joins a host over UDP on localhost, gets
    /// its own avatar, and both players walk; the two machines end up with
    /// the same world.
    #[test]
    fn two_players_walk_around_together() {
        use sim::settlement::avatar::Intent;
        use sim::settlement::SceneSetup;
        let content = Content::builtin();
        let setup = SceneSetup { player: "exosuit".into(), extent: Fx::from_int(8), spawn: [Fx::ZERO, Fx::ZERO, Fx::from_int(2)], ..SceneSetup::default() };
        let world = World::new_scene(1, &content, &setup).unwrap();
        let config = NetConfig { threads: 2, ..NetConfig::default() };
        let listen = Some("127.0.0.1:0".parse().unwrap());
        let mut host = Session::start(Mode::Host { world, label: "first".into(), listen }, content.clone(), config.clone()).unwrap();
        let addr = host.local_addr().unwrap();
        let mut client = Session::start(Mode::Join { host: addr, stable_id: 42 }, content.clone(), config).unwrap();

        let avatar = |session: &Session| session.world()?.settlement()?.avatar_of(session.player()?);
        let position = |session: &Session, agent: u16| session.world().unwrap().settlement().unwrap().agents[agent as usize].position;
        let walk = |agent: u16, x: i32| CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::from_int(x), Fx::ZERO), run: false } };
        let start = std::time::Instant::now();
        let (mut walking, mut now) = (false, Duration::ZERO);
        while start.elapsed() < Duration::from_secs(4) {
            now = start.elapsed();
            host.update(now, |_| {});
            client.update(now, |_| {});
            if !walking {
                if let (Some(mine), Some(theirs)) = (avatar(&client), avatar(&host)) {
                    host.queue(walk(theirs, -1));
                    client.queue(walk(mine, 1));
                    walking = true;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(walking, "the client never got an avatar: {}", client.status());
        assert_eq!(host.status(), "hosting, 2 players");

        // Stop the host and let the client run (a tick at most per update)
        // until it reaches the host's tick.
        let target = host.world().unwrap().tick();
        let wait = std::time::Instant::now();
        while client.world().unwrap().tick() < target && wait.elapsed() < Duration::from_secs(2) {
            now += Duration::from_millis(2);
            client.update(now, |_| {});
            std::thread::sleep(Duration::from_millis(1));
        }
        let (mine, theirs) = (avatar(&client).unwrap(), avatar(&host).unwrap());
        assert_ne!(mine, theirs);
        let spawn = host.world().unwrap().settlement().unwrap().spawn;
        assert!(position(&host, mine).x > spawn.x + Fx::from_int(2), "the guest walked right");
        assert!(position(&host, theirs).x < spawn.x - Fx::from_int(2), "the host walked left");
        assert_eq!(host.world().unwrap().tick(), client.world().unwrap().tick());
        assert_eq!(host.save(), client.save(), "both machines have the same world");
    }

    /// One single-player session through two worlds: the second starts at
    /// tick 0, and the input log covers only the current world.
    #[test]
    fn a_session_goes_through_two_worlds() {
        let content = Content::builtin();
        let mut session = single_player();
        let mut now = Duration::ZERO;
        while now < Duration::from_secs(1) {
            now += Duration::from_millis(10);
            session.update(now, |_| {});
        }
        assert!(session.world().unwrap().tick() > 30);
        let world = World::new(sim::scenario::Scenario::small().config, content.rules.clone());
        assert!(session.load_world(world, "second".into(), now));
        assert_eq!((session.world_number(), session.world_label()), (2, Some("second")));
        assert_eq!(session.world().unwrap().tick(), 0);
        // Nobody to wait for.
        assert!(!session.world_loading());
        let end = now + Duration::from_millis(500);
        while now < end {
            now += Duration::from_millis(10);
            session.update(now, |_| {});
        }
        let tick = session.world().unwrap().tick();
        assert!((25..=35).contains(&tick), "the second world ran {tick} ticks in half a second");
        let log = session.input_log().unwrap();
        assert_eq!((log.start_tick, log.turns.len() as u64), (0, tick));
    }

    /// A map change over UDP: the client loads the host's new world, learns
    /// its label, gets an avatar in it, and ends with the host's state.
    #[test]
    fn a_client_follows_the_host_to_a_new_world() {
        use sim::settlement::SceneSetup;
        let content = Content::builtin();
        let scene = |x: i32| {
            let setup = SceneSetup { player: "exosuit".into(), extent: Fx::from_int(8), spawn: [Fx::from_int(x), Fx::ZERO, Fx::from_int(2)], ..SceneSetup::default() };
            World::new_scene(1, &content, &setup).unwrap()
        };
        let config = NetConfig { threads: 2, ..NetConfig::default() };
        let listen = Some("127.0.0.1:0".parse().unwrap());
        let mut host = Session::start(Mode::Host { world: scene(0), label: "first".into(), listen }, content.clone(), config.clone()).unwrap();
        let addr = host.local_addr().unwrap();
        let mut client = Session::start(Mode::Join { host: addr, stable_id: 42 }, content.clone(), config).unwrap();
        let avatar = |session: &Session| session.world()?.settlement()?.avatar_of(session.player()?);

        let start = std::time::Instant::now();
        let run_until = |host: &mut Session, client: &mut Session, done: &dyn Fn(&Session, &Session) -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !done(host, client) && std::time::Instant::now() < deadline {
                let now = start.elapsed();
                host.update(now, |_| {});
                client.update(now, |_| {});
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        run_until(&mut host, &mut client, &|_, c| avatar(c).is_some());
        assert_eq!(client.world_label(), Some("first"));

        assert!(host.load_world(scene(3), "second".into(), start.elapsed()));
        assert!(host.world_loading(), "the host waits for the client");
        run_until(&mut host, &mut client, &|h, c| {
            avatar(c).is_some() && c.world_number() == 2 && c.world().unwrap().tick() >= 120 && h.world().unwrap().tick() >= 120
        });
        assert_eq!((client.world_number(), client.world_label()), (2, Some("second")));
        assert!(avatar(&client).is_some(), "the client has an avatar in the new world: {}", client.status());

        // Every tick the client ran in the second world has the host's hash.
        let (_, host_hashes) = host.desync_evidence();
        let (_, client_hashes) = client.desync_evidence();
        assert!(client_hashes.lines().count() >= 120);
        assert!(client_hashes.starts_with("0 "), "the client's log starts at the new world's first tick");
        for line in client_hashes.lines() {
            assert!(host_hashes.lines().any(|h| h == line), "client tick and hash {line} differ from the host's");
        }
    }

    /// A bot for one player: walks at the nearest ghoul from its `lanes`
    /// (wave indices) and strikes it, deciding from this peer's own world
    /// every few ticks.
    fn fight(session: &mut Session, weapon: content::scenario::Weapon, lanes: &[u16], last: &mut u64) {
        use sim::settlement::avatar::Intent;
        let Some(world) = session.world() else { return };
        let tick = world.tick();
        let Some(s) = world.settlement() else { return };
        let Some(agent) = session.player().and_then(|p| s.avatar_of(p)) else { return };
        if tick < *last + 6 {
            return;
        }
        *last = tick;
        let a = &s.agents[agent as usize];
        let mine = s.threats.iter().filter(|t| lanes.contains(&t.wave));
        let Some(t) = mine.min_by_key(|t| (t.position - a.position).length()) else { return };
        let to = t.position - a.position;
        let kind = if to.length() > Fx::ONE {
            CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: to.normalize_or_zero(), run: true } }
        } else {
            CommandKind::Attack { agent, weapon, aim: to, muzzle: [Fx::ZERO, Fx::from_ratio(3, 5), Fx::from_ratio(1, 4)] }
        };
        session.queue(kind);
    }

    /// The ghoul defense over the network: a client joins the Red Clearing
    /// over UDP, both players fight the waves with their own weapons, and
    /// the two machines end with the same world.
    #[test]
    fn two_players_fight_ghouls_over_udp() {
        use content::scenario::{Scenario, Weapon};
        use sim::settlement::Event;
        let content = Content::builtin();
        let scenario = Scenario::parse(include_str!("../../app/assets/levels/combat_arena_supply.ron")).unwrap();
        let world = World::new_settlement(1, &content, &scenario, &[]).unwrap();
        let config = NetConfig { threads: 2, ..NetConfig::default() };
        let listen = Some("127.0.0.1:0".parse().unwrap());
        let mut host = Session::start(Mode::Host { world, label: "first".into(), listen }, content.clone(), config.clone()).unwrap();
        let addr = host.local_addr().unwrap();
        let mut client = Session::start(Mode::Join { host: addr, stable_id: 7 }, content.clone(), config).unwrap();

        // Hits by each avatar, counted on the host.
        let mut hits = std::collections::BTreeMap::<u16, u32>::new();
        let (mut host_last, mut client_last) = (0, 0);
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(9) {
            let now = start.elapsed();
            host.update(now, |world| {
                for event in &world.settlement().unwrap().events {
                    if let Event::Hit { agent: Some(agent), .. } = event {
                        *hits.entry(*agent).or_default() += 1;
                    }
                }
            });
            client.update(now, |_| {});
            // The host holds the center lane, the guest the flanks.
            fight(&mut host, Weapon::Sword, &[0], &mut host_last);
            fight(&mut client, Weapon::Spear, &[1, 2], &mut client_last);
            std::thread::sleep(Duration::from_millis(2));
        }
        let s = host.world().unwrap().settlement().unwrap();
        let (mine, theirs) = (s.avatar_of(net::HOST_PLAYER).unwrap(), s.avatar_of(client.player().unwrap()).unwrap());
        assert!(hits.get(&mine).is_some_and(|&n| n > 0) && hits.get(&theirs).is_some_and(|&n| n > 0), "both players hit ghouls: {hits:?}");
        assert!(s.combat.spear_hits > 0 && s.combat.melee_hits > 0, "{:?}", s.combat);

        // Every tick both machines ran hashed the same.
        let hashes = |session: &Session| -> std::collections::BTreeMap<u64, String> {
            session.desync_evidence().1.lines().map(|line| {
                let (tick, hash) = line.split_once(' ').unwrap();
                (tick.parse().unwrap(), hash.to_string())
            }).collect()
        };
        let (on_host, on_client) = (hashes(&host), hashes(&client));
        let shared: Vec<_> = on_client.iter().filter(|(tick, _)| on_host.contains_key(tick)).collect();
        assert!(shared.len() > 100, "only {} ticks in common", shared.len());
        for (tick, hash) in shared {
            assert_eq!(&on_host[tick], hash, "the machines diverged at tick {tick}");
        }
    }
}

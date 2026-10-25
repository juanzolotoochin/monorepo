use std::cell::RefCell;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::{Duration, Instant};

use content::Content;
use fx::{Fx, FxVec2};
use sim::scenario::Scenario;
use sim::{CommandKind, World};

use crate::netsim::{LinkConditions, NetSim, SimAddr, SimSocket};
use crate::{Client, ClientPhase, DesyncPolicy, Host, NetConfig, NetEvent, UdpTransport, TICK};

const HOST: SimAddr = 0;
/// Frames advance the virtual clock in 5 ms steps (200 Hz).
const FRAME: Duration = Duration::from_millis(5);

struct Harness {
    net: Rc<RefCell<NetSim>>,
    host: Host<SimSocket>,
    clients: Vec<Client<SimSocket>>,
    host_events: Vec<NetEvent>,
    client_events: Vec<Vec<NetEvent>>,
    now: Duration,
    rng: u64,
    config: NetConfig,
    host_paused: bool,
}

impl Harness {
    fn new(link: LinkConditions, config: NetConfig, populated: bool) -> Harness {
        let content = Content::builtin();
        let scenario = Scenario::small();
        let net = NetSim::new(99, link);
        let world = World::new(scenario.config.clone(), content.rules.clone());
        let mut host = Host::new(SimSocket::new(&net, HOST), world, "small".into(), content, config.clone(), 1, Duration::ZERO);
        if populated {
            let probe = scenario.start(1);
            for command in scenario.turn(&probe) {
                host.queue(command.kind);
            }
        }
        Harness {
            net,
            host,
            clients: Vec::new(),
            host_events: Vec::new(),
            client_events: Vec::new(),
            now: Duration::ZERO,
            rng: 1,
            config,
            host_paused: false,
        }
    }

    fn add_client(&mut self) -> usize {
        self.add_client_with(Content::builtin())
    }

    fn add_client_with(&mut self, content: Content) -> usize {
        let index = self.clients.len();
        let addr = index as SimAddr + 1;
        let socket = SimSocket::new(&self.net, addr);
        self.clients.push(Client::new(socket, HOST, content, self.config.clone(), 1000 + index as u64, self.now));
        self.client_events.push(Vec::new());
        index
    }

    fn random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Runs for `duration`; each frame, each player makes an input with
    /// probability `input_permille / 1000`.
    fn run_for(&mut self, duration: Duration, input_permille: u64) {
        let end = self.now + duration;
        while self.now < end {
            self.now += FRAME;
            self.net.borrow_mut().advance_to(self.now);
            if !self.host_paused {
                self.host.update(self.now, |_| {});
                self.host_events.extend(self.host.take_events());
            }
            for i in 0..self.clients.len() {
                self.clients[i].update(self.now, |_| {});
                let events = self.clients[i].take_events();
                self.client_events[i].extend(events);
            }
            for player in 0..=self.clients.len() {
                if self.random() % 1000 < input_permille {
                    let kind = self.random_input();
                    match player {
                        0 if !self.host_paused => self.host.queue(kind),
                        0 => {}
                        p => self.clients[p - 1].queue(kind),
                    }
                }
            }
        }
    }

    fn random_input(&mut self) -> CommandKind {
        let x = Fx::from_int((self.random() % 250) as i32 + 3);
        let y = Fx::from_int((self.random() % 250) as i32 + 3);
        if self.random() % 3 == 0 {
            CommandKind::SpawnMovers { center: FxVec2::new(x, y), count: 6 }
        } else {
            let id = self.host.world().chunks().iter().find_map(|c| c.movers.id.first().copied());
            match id {
                Some(mover) => CommandKind::Impulse { mover, velocity: FxVec2::new(Fx::from_ratio(1, 6), Fx::ZERO) },
                None => CommandKind::SpawnMovers { center: FxVec2::new(x, y), count: 2 },
            }
        }
    }

    /// Every tick a client ran has the same hash as the host's.
    fn assert_in_sync(&self) {
        for (i, client) in self.clients.iter().enumerate() {
            let log = client.history();
            assert!(log.end() > log.start(), "client {i} ran no ticks");
            for tick in log.start()..log.end() {
                assert_eq!(client.history().get(tick), self.host.history().get(tick), "client {i} at tick {tick}");
            }
        }
    }

    fn desyncs(&self) -> usize {
        let is_desync = |e: &&NetEvent| matches!(e, NetEvent::Desync { .. });
        self.host_events.iter().filter(is_desync).count()
            + self.client_events.iter().flatten().filter(is_desync).count()
    }
}

fn config() -> NetConfig {
    NetConfig { threads: 2, ..NetConfig::default() }
}

#[test]
fn lockstep_over_a_bad_network() {
    let mut h = Harness::new(LinkConditions::internet(), config(), true);
    h.add_client();
    h.add_client();
    h.run_for(Duration::from_secs(40), 20);

    assert_eq!(h.desyncs(), 0);
    h.assert_in_sync();
    // The host never waits for clients.
    let host_tick = h.host.world().tick();
    assert!(host_tick >= 1190, "host only reached tick {host_tick}");
    for client in &h.clients {
        assert_eq!(client.phase(), ClientPhase::Playing);
        let behind = host_tick - client.world().unwrap().tick();
        // One way is about 4 ticks, plus jitter and the buffer.
        assert!(behind <= 15, "client is {behind} ticks behind");
        let rtt = client.stats().rtt.unwrap();
        assert!(rtt >= Duration::from_millis(180) && rtt <= Duration::from_millis(320), "rtt {rtt:?}");
    }
}

#[test]
fn client_inputs_apply_everywhere() {
    let mut h = Harness::new(LinkConditions::internet(), config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    h.clients[0].queue(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(50), Fx::from_int(50)), count: 7 });
    assert_eq!(h.clients[0].pending_inputs(), 1);
    h.run_for(Duration::from_secs(2), 0);
    assert_eq!(h.clients[0].pending_inputs(), 0);
    assert_eq!(h.host.world().mover_count(), 7);
    assert_eq!(h.clients[0].world().unwrap().mover_count(), 7);
    h.assert_in_sync();
}

#[test]
fn joining_mid_game() {
    let mut h = Harness::new(LinkConditions::internet(), config(), true);
    h.add_client();
    h.run_for(Duration::from_secs(10), 20);
    let late = h.add_client();
    h.run_for(Duration::from_secs(10), 20);

    assert_eq!(h.desyncs(), 0);
    let loaded = h.client_events[late].iter().find_map(|e| match e {
        NetEvent::Loaded { tick } => Some(*tick),
        _ => None,
    });
    assert!(loaded.is_some_and(|tick| tick >= 290), "late client loaded at {loaded:?}");
    h.assert_in_sync();
}

#[test]
fn late_inputs_move_to_the_next_open_turn() {
    let mut h = Harness::new(LinkConditions::internet(), config(), true);
    h.add_client();
    // A one-second spike on the client's uplink.
    let spike = (Duration::from_secs(5), Duration::from_secs(6), Duration::from_secs(1));
    h.net.borrow_mut().set_link(1, HOST, LinkConditions { spikes: vec![spike], ..LinkConditions::internet() });
    h.run_for(Duration::from_secs(12), 60);
    assert!(h.host.stats().late_inputs > 0);
    assert!(h.host.world().tick() >= 355, "host waited: tick {}", h.host.world().tick());
    assert_eq!(h.desyncs(), 0);
    h.assert_in_sync();
}

#[test]
fn desync_halts_everyone_in_dev() {
    let mut h = Harness::new(LinkConditions::perfect(), config(), true);
    h.add_client();
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    h.clients[0].diverge_for_test(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(9), Fx::from_int(9)), count: 1 });
    h.run_for(Duration::from_secs(2), 0);

    assert!(h.host.halted().is_some());
    assert!(h.host_events.iter().any(|e| matches!(e, NetEvent::Desync { player: Some(_), .. })));
    for client in &h.clients {
        assert!(matches!(client.phase(), ClientPhase::Stopped { .. }), "{:?}", client.phase());
    }
}

#[test]
fn desync_resyncs_the_client_in_release() {
    let mut h = Harness::new(LinkConditions::internet(), NetConfig { desync: DesyncPolicy::Resync, ..config() }, true);
    h.add_client();
    h.run_for(Duration::from_secs(4), 10);
    h.clients[0].diverge_for_test(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(9), Fx::from_int(9)), count: 1 });
    h.run_for(Duration::from_secs(8), 10);

    assert!(h.host.halted().is_none());
    let loads = h.client_events[0].iter().filter(|e| matches!(e, NetEvent::Loaded { .. })).count();
    assert_eq!(loads, 2, "{:?}", h.client_events[0]);
    // The history restarted at the resync save, and matches since.
    h.assert_in_sync();
}

#[test]
fn different_rules_are_rejected() {
    let mut h = Harness::new(LinkConditions::perfect(), config(), false);
    let mut sources = content::builtin_sources();
    let large = sources.iter_mut().find(|(path, _)| path == "mover/large.ron").expect("the large mover");
    large.1 = large.1.replace("max_hp: 40", "max_hp: 41");
    h.add_client_with(Content::from_sources(sources).unwrap());
    h.run_for(Duration::from_secs(1), 0);
    assert!(matches!(h.client_events[0][..], [NetEvent::Rejected { .. }]));
}

#[test]
fn host_loss_is_detected() {
    let mut h = Harness::new(LinkConditions::perfect(), config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(2), 0);
    h.host_paused = true;
    h.run_for(Duration::from_secs(6), 0);
    assert!(h.client_events[0].contains(&NetEvent::HostLost));
}

#[test]
fn players_keep_their_slot_when_they_return() {
    let mut h = Harness::new(LinkConditions::perfect(), config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(1), 0);
    let first = h.clients[0].player();
    h.clients[0].shutdown();
    h.run_for(Duration::from_millis(100), 0);
    // Same stable ID from a new address.
    let socket = SimSocket::new(&h.net, 9);
    h.clients[0] = Client::new(socket, HOST, Content::builtin(), config(), 1000, h.now);
    h.run_for(Duration::from_secs(1), 0);
    assert_eq!(h.clients[0].player(), first);
}

/// The same protocol over real UDP sockets on localhost.
#[test]
fn udp_smoke() {
    let content = Content::builtin();
    // Few movers: this checks the protocol over real sockets in real time, so
    // the ticks must stay cheap even when other tests load the machine.
    let scenario = Scenario { per_cluster: 20, ..Scenario::small() };
    let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let host_socket = UdpTransport::bind(any).unwrap();
    let host_addr = host_socket.local_addr().unwrap();
    let world = World::new(scenario.config.clone(), content.rules.clone());
    let start = Instant::now();
    let mut host = Host::new(host_socket, world, "small".into(), content.clone(), config(), 7, Duration::ZERO);
    for command in scenario.turn(&scenario.start(1)) {
        host.queue(command.kind);
    }
    let mut client = Client::new(UdpTransport::bind(any).unwrap(), host_addr, content, config(), 5, Duration::ZERO);
    while start.elapsed() < Duration::from_secs(3) {
        let now = start.elapsed();
        host.update(now, |_| {});
        client.update(now, |_| {});
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(client.phase(), ClientPhase::Playing);
    let log = client.history();
    assert!(log.end() - log.start() > 30, "client ran {} ticks", log.end() - log.start());
    for tick in log.start()..log.end() {
        assert_eq!(log.get(tick), host.history().get(tick));
    }
    assert!(host.world().tick() as f64 >= 0.8 * 3.0 / TICK.as_secs_f64(), "host ran {} ticks", host.world().tick());
}

/// Runs the harness's host through a map change: a new world at tick 0.
fn change_map(h: &mut Harness, label: &str) {
    let content = Content::builtin();
    let world = World::new(Scenario::small().config, content.rules.clone());
    h.host.load_world(world, label.into(), h.now);
}

#[test]
fn a_map_change_moves_everyone_to_the_new_world() {
    let mut h = Harness::new(LinkConditions::internet(), config(), true);
    h.add_client();
    h.add_client();
    h.run_for(Duration::from_secs(5), 20);
    let old_tick = h.host.world().tick();
    assert!(old_tick > 250);

    change_map(&mut h, "second");
    h.run_for(FRAME, 0);
    // The new world waits at tick 0 until both clients have it.
    assert!(h.host.starting());
    assert_eq!(h.host.world().tick(), 0);
    h.run_for(Duration::from_secs(8), 20);

    assert!(!h.host.starting());
    assert_eq!(h.desyncs(), 0);
    assert_eq!(h.host.world_number(), 2);
    assert_eq!(h.host.history().start(), 0);
    for (i, client) in h.clients.iter().enumerate() {
        assert_eq!(client.phase(), ClientPhase::Playing);
        assert_eq!((client.world_number(), client.label()), (2, "second"));
        assert_eq!(client.history().start(), 0, "client {i} started world 2 at its first tick");
        let loads = h.client_events[i].iter().filter(|e| matches!(e, NetEvent::Loaded { .. })).count();
        assert_eq!(loads, 2, "client {i}: {:?}", h.client_events[i]);
    }
    h.assert_in_sync();
}

#[test]
fn packets_from_an_earlier_world_are_ignored() {
    use crate::protocol::{encode, Packet};
    use crate::transport::Transport;
    let mut h = Harness::new(LinkConditions::perfect(), config(), true);
    h.add_client();
    h.run_for(Duration::from_secs(2), 0);
    change_map(&mut h, "second");
    h.run_for(Duration::from_secs(2), 0);
    assert_eq!(h.clients[0].world_number(), 2);

    // Stale world-1 packets "from the host": a welcome back to world 1, and
    // turns for upcoming ticks that would spawn movers if they were taken.
    let mut stale = SimSocket::new(&h.net, HOST);
    let tick = h.clients[0].world().unwrap().tick();
    let player = h.clients[0].player().unwrap();
    let spawn = sim::Command { player, kind: CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(9), Fx::from_int(9)), count: 3 } };
    let packets = [
        Packet::Welcome { epoch: 1, world: 1, label: "small".into(), player, input_delay: 0, save_tick: 0, save_len: 10 },
        Packet::Turns { epoch: 1, world: 1, first: tick, turns: vec![vec![spawn]; 30], host_tick: tick + 30, input_ack: 0, pong: 0, pong_held_ms: 0 },
    ];
    for packet in &packets {
        stale.send(&1, &encode(packet));
    }
    h.run_for(Duration::from_secs(2), 0);

    assert_eq!(h.clients[0].phase(), ClientPhase::Playing);
    assert_eq!((h.clients[0].world_number(), h.clients[0].label()), (2, "second"));
    assert_eq!(h.desyncs(), 0);
    h.assert_in_sync();
}

/// Release is new in protocol 3: it round-trips, and a peer speaking 2
/// (which cannot decode it) is ignored rather than misread.
#[test]
fn release_needs_protocol_three() {
    use crate::protocol::{decode, encode, Input, Packet, PROTOCOL_VERSION};
    let inputs = vec![Input { seq: 1, tick: 4, kind: sim::CommandKind::ReleaseDirectControl { agent: 0 } }];
    let packet = Packet::Inputs { epoch: 1, world: 1, turn_ack: 0, inputs, hashes: vec![], ping: 0 };
    let mut bytes = encode(&packet);
    assert_eq!(PROTOCOL_VERSION, 3);
    assert_eq!(decode(&bytes), Some(packet));
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(decode(&bytes), None);
}

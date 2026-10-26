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
fn diagnostics_distinguish_receipt_from_local_application() {
    let mut h = Harness::new(LinkConditions::perfect(), NetConfig { input_margin: Some(60), ..config() }, false);
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    h.clients[0].queue(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(50), Fx::from_int(50)), count: 7 });
    h.run_for(Duration::from_millis(100), 0);
    let d = h.clients[0].diagnostics(h.now);
    assert_eq!(d.unacked_inputs, 0, "the host has received the command");
    assert_eq!(h.clients[0].pending_inputs(), 1, "receipt is not local application");
    assert!(d.inputs_queued > d.inputs_applied);
    let host = h.host.diagnostics(h.now);
    assert_eq!(host.peers[0].last_requested_tick, host.peers[0].last_assigned_tick);
    assert_eq!(host.peers[0].last_requested_tick, d.last_requested_tick);
    assert_eq!(Some(host.peers[0].input_ack), d.last_input_ack);
    h.run_for(Duration::from_secs(2), 0);
    assert_eq!(h.clients[0].pending_inputs(), 0);
    h.assert_in_sync();
}

#[test]
fn pending_commands_stay_until_simulated_and_go_with_the_world() {
    let mut h = Harness::new(LinkConditions::perfect(), NetConfig { input_margin: Some(60), ..config() }, false);
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    let spawn = |count| CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(50), Fx::from_int(50)), count };
    let horizon = h.clients[0].input_horizon();
    h.clients[0].queue(spawn(1));
    h.run_for(Duration::from_millis(50), 0);
    h.clients[0].queue(spawn(2));
    let requested = h.clients[0].diagnostics(h.now).last_requested_tick.unwrap();
    let pending: Vec<_> = h.clients[0].pending_commands().map(|(tick, kind)| (tick, kind.clone())).collect();
    assert_eq!(pending, vec![(horizon, spawn(1)), (requested, spawn(2))]);
    assert!(requested > horizon);

    // Received by the host, still not simulated here.
    h.run_for(Duration::from_millis(100), 0);
    assert_eq!(h.clients[0].diagnostics(h.now).unacked_inputs, 0);
    assert_eq!(h.clients[0].pending_commands().count(), 2);
    // Each leaves once its turn runs here.
    while h.clients[0].world().unwrap().tick() <= horizon {
        h.run_for(FRAME, 0);
    }
    let left: Vec<_> = h.clients[0].pending_commands().map(|(tick, _)| tick).collect();
    assert_eq!(left, vec![requested]);
    h.run_for(Duration::from_secs(2), 0);
    assert_eq!(h.clients[0].pending_commands().count(), 0);
    assert_eq!(h.clients[0].world().unwrap().mover_count(), 3);

    // A new world drops what the old one never ran.
    h.clients[0].queue(spawn(1));
    change_map(&mut h, "second");
    h.run_for(Duration::from_millis(50), 0);
    assert_eq!(h.clients[0].pending_commands().count(), 0);
    assert_eq!(h.clients[0].pending_inputs(), 0);
}

#[test]
fn diagnostics_observe_starvation_and_catch_up_without_changing_turns() {
    let mut h = Harness::new(LinkConditions::perfect(), config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    h.net.borrow_mut().set_link(HOST, 1, LinkConditions { loss_permille: 1000, ..LinkConditions::perfect() });
    h.run_for(Duration::from_secs(1), 0);
    let waiting = h.clients[0].diagnostics(h.now);
    assert!(waiting.starved_updates > 0);
    assert!(waiting.starved_time > Duration::from_millis(500));
    assert!(waiting.last_packet_age.unwrap() > Duration::from_millis(900));
    h.net.borrow_mut().set_link(HOST, 1, LinkConditions::perfect());
    h.run_for(Duration::from_secs(2), 0);
    assert!(h.clients[0].diagnostics(h.now).catch_up_ticks > waiting.catch_up_ticks);
    assert_eq!(h.desyncs(), 0);
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
    let peers = h.host.diagnostics(h.now).peers;
    assert!(peers[0].late_inputs > 0);
    assert!(peers[0].shifted_ticks >= peers[0].late_inputs);
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

/// A steady link with no jitter or loss, `ms` each way.
fn steady(ms: u64) -> LinkConditions {
    LinkConditions { latency: Duration::from_millis(ms), ..LinkConditions::perfect() }
}

/// Runs `frames` frames, the client queueing a jump every `every` frames
/// (numbered by agent, so the host's turns show their order). Returns the
/// tick each asked for and how often it asked for more than the margin
/// alone would have, so as not to go back before the previous input.
fn queue_jumps(h: &mut Harness, frames: u32, every: u32, next: &mut u16) -> (Vec<u64>, usize) {
    let (mut requested, mut clamped) = (Vec::new(), 0);
    for frame in 0..frames {
        if frame % every == 0 {
            let client = &mut h.clients[0];
            let unclamped = client.host_present() + client.input_delay();
            client.queue(CommandKind::Jump { agent: *next });
            *next += 1;
            let tick = client.diagnostics(h.now).last_requested_tick.unwrap();
            clamped += usize::from(tick > unclamped);
            requested.push(tick);
        }
        h.run_for(FRAME, 0);
    }
    (requested, clamped)
}

/// The client's jumps in the order the host applied them.
fn applied_jumps(h: &Harness) -> Vec<u16> {
    let player = h.clients[0].player().unwrap();
    let (_, turns) = h.host.turns();
    turns.iter().flatten().filter(|c| c.player == player).filter_map(|c| match c.kind {
        CommandKind::Jump { agent } => Some(agent),
        _ => None,
    }).collect()
}

#[test]
fn the_margin_adapts_to_the_measured_network() {
    // A LAN: one way rounds up to a tick, plus a tick of safety.
    let mut h = Harness::new(LinkConditions::perfect(), config(), false);
    h.add_client();
    assert_eq!(h.clients[0].input_delay(), config().input_delay, "the host's margin until measured");
    h.run_for(Duration::from_secs(3), 0);
    assert!(h.clients[0].input_delay() <= 2, "LAN margin {}", h.clients[0].input_delay());

    // 100 ms each way, up to 40 ms of jitter: more than one way and a tick.
    let mut h = Harness::new(LinkConditions { loss_permille: 0, ..LinkConditions::internet() }, config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(5), 0);
    let rtt = h.clients[0].stats().rtt.unwrap();
    let one_way = (rtt / 2).as_nanos().div_ceil(TICK.as_nanos()) as u64;
    let margin = h.clients[0].input_delay();
    assert!(margin > one_way + 1 && margin < one_way + 6, "margin {margin} ticks, one way {one_way}");
    let d = h.clients[0].diagnostics(h.now);
    assert_eq!((d.input_delay, d.input_margin_pinned), (margin, false));
}

#[test]
fn a_pinned_margin_is_kept() {
    let mut h = Harness::new(LinkConditions::internet(), NetConfig { input_margin: Some(3), ..config() }, false);
    h.add_client();
    h.run_for(Duration::from_secs(5), 0);
    let mut next = 0;
    queue_jumps(&mut h, 2000, 4, &mut next);
    h.run_for(Duration::from_secs(2), 0);
    let d = h.clients[0].diagnostics(h.now);
    assert!(d.late_inputs > 0, "3 ticks is too short for 100 ms each way");
    assert_eq!((d.input_delay, d.input_margin_pinned), (3, true));
}

#[test]
fn a_client_sees_its_late_inputs_grows_its_margin_and_shrinks_it_when_quiet() {
    let mut h = Harness::new(steady(50), config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    let settled = h.clients[0].input_delay();
    // A quarter second more on the uplink for a second.
    let spike = (h.now, h.now + Duration::from_secs(1), Duration::from_millis(250));
    h.net.borrow_mut().set_link(1, HOST, LinkConditions { spikes: vec![spike], ..steady(50) });
    let mut next = 0;
    queue_jumps(&mut h, 400, 4, &mut next);
    h.run_for(Duration::from_millis(1500), 0);
    assert_eq!(h.clients[0].pending_inputs(), 0);

    // The client counts what the host counted, from the turns alone.
    let d = h.clients[0].diagnostics(h.now);
    let peer = h.host.diagnostics(h.now).peers[0].clone();
    assert!(peer.late_inputs > 0);
    assert_eq!((d.late_inputs, d.shifted_ticks), (peer.late_inputs, peer.shifted_ticks));
    // Grown at once by about the spike.
    let grown = h.clients[0].input_delay();
    assert!(grown >= settled + 12, "margin {settled} → {grown} ticks");

    // Then back down a tick at a time, once quiet: the spike is over.
    let quiet = crate::margin::QUIET;
    h.run_for(quiet - Duration::from_millis(500), 0);
    assert!(h.clients[0].input_delay() >= grown - 1);
    h.run_for(quiet * (grown - settled) as u32 + quiet, 0);
    assert_eq!(h.clients[0].input_delay(), settled);
    assert_eq!(h.desyncs(), 0);
    h.assert_in_sync();
}

#[test]
fn requested_ticks_never_go_backwards_while_the_margin_shrinks() {
    // A lag spike grows the margin (its round trips are measured, no input
    // is sent); after it the margin shrinks back a tick at a time while the
    // client keeps sending. Shrinks land as new turns arrive, which also move
    // the host's tick on; turns reordered on the way back (jitter over a
    // tick) lower the round trip without that, so the margin alone would
    // ask for an earlier tick than the last input did.
    let mut h = Harness::new(LinkConditions { jitter: Duration::from_millis(40), ..steady(50) }, config(), false);
    h.add_client();
    h.run_for(Duration::from_secs(3), 0);
    let settled = h.clients[0].input_delay();
    let spike = (h.now, h.now + Duration::from_secs(1), Duration::from_millis(250));
    h.net.borrow_mut().set_link(1, HOST, LinkConditions { spikes: vec![spike], ..steady(50) });
    h.run_for(Duration::from_secs(2), 0);
    let grown = h.clients[0].input_delay();
    assert!(grown >= settled + 5, "margin {settled} → {grown} ticks");
    let mut next = 0;
    let per_tick = (crate::margin::QUIET.as_millis() / FRAME.as_millis()) as u32;
    let (requested, clamped) = queue_jumps(&mut h, per_tick * (grown - settled + 1) as u32, 1, &mut next);
    h.run_for(Duration::from_secs(1), 0);

    let after = h.clients[0].input_delay();
    assert!(after <= settled, "the margin went {settled} → {grown} → {after} ticks");
    assert!(requested.windows(2).all(|w| w[0] <= w[1]), "a requested tick went backwards");
    assert!(clamped > 0, "the margin never shrank under a sent input");
    // So the host applied them in the order they were sent, none late.
    assert_eq!(applied_jumps(&h), (0..next).collect::<Vec<_>>());
    assert_eq!(h.host.diagnostics(h.now).peers[0].late_inputs, 0);
    assert_eq!(h.clients[0].diagnostics(h.now).late_inputs, 0);
    h.assert_in_sync();
}

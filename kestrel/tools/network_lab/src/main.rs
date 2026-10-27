//! A localhost UDP relay that delays each datagram, for feeling Kestrel over
//! a worse network on one machine. Started by ../network-lab.ps1; see there.
//!
//!   clients <-> 127.0.0.1:<listen> (this relay) <-> 127.0.0.1:<host>
//!
//! Each client sends to the listen socket and hears replies from it, so its
//! host-address check holds. The relay forwards each client to the host from
//! a socket of its own, so the host sees every client as its own peer and
//! replies to that socket. Up to --max-clients addresses are served at once;
//! anything past them is dropped and counted. A client silent for --idle-ms
//! (it closed) is forgotten, so one that rejoins from a new port gets its slot.
//!
//! Each datagram is held for max(0, delay + uniform(-jitter, +jitter)) ms, one
//! seeded random stream per client and direction (`bookkeeping.rs`). Datagrams may
//! overtake one another.
//!
//! Windows timers wake in ~15.6 ms steps, which alone would add ~7 ms of
//! delay. The relay sleeps on a timer until a delivery is close, then polls
//! until it is due (about one CPU core while packets are in flight), and
//! reports how late deliveries actually were. `--spin-ms 0` turns polling off.
//!
//! Status lives in the run directory: `ready.json` once the listen socket is
//! bound, `stats.json` every second and on exit. Creating `stop` in the run
//! directory, the parent process exiting, or Ctrl+C stops it.

mod bookkeeping;

use crate::bookkeeping::{Admit, Config, Relay, Stats, Way};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
/// How often socket threads look up from a quiet socket to see if they're done.
const RECV_TIMEOUT: Duration = Duration::from_millis(200);

/// Everything the command line says.
#[derive(Debug, PartialEq)]
struct Args {
    listen: u16,
    host: u16,
    config: Config,
    run_dir: PathBuf,
    parent_pid: Option<u32>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    const NAMES: [&str; 12] = [
        "listen", "host", "delay-ms", "jitter-ms", "seed", "spin-ms", "max-packets", "max-bytes", "max-clients",
        "idle-ms", "run-dir", "parent-pid",
    ];
    let mut values: HashMap<&str, String> = HashMap::new();
    let mut rest = argv.iter();
    while let Some(arg) = rest.next() {
        let Some(option) = arg.strip_prefix("--") else { return Err(format!("unexpected argument {arg}")) };
        let (name, inline) = match option.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (option, None),
        };
        let Some(&name) = NAMES.iter().find(|&&n| n == name) else { return Err(format!("unknown option --{name}")) };
        let value = match inline {
            Some(value) => value,
            None => rest.next().ok_or_else(|| format!("--{name} needs a value"))?.clone(),
        };
        values.insert(name, value);
    }
    let integer = |name: &str, default: Option<u64>, min: u64, max: u64| -> Result<u64, String> {
        let text = values.get(name).cloned().or(default.map(|d| d.to_string()));
        match text.as_deref().filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())).map(str::parse::<u64>) {
            Some(Ok(n)) if (min..=max).contains(&n) => Ok(n),
            _ => Err(format!(
                "--{name} must be an integer from {min} to {max}, got {}",
                text.as_deref().unwrap_or("nothing")
            )),
        }
    };
    let listen = integer("listen", None, 1, 65535)? as u16;
    let host = integer("host", None, 1, 65535)? as u16;
    let defaults = Config::default();
    let config = Config {
        delay_ms: integer("delay-ms", None, 0, 10_000)? as u32,
        jitter_ms: integer("jitter-ms", None, 0, 10_000)? as u32,
        seed: integer("seed", None, 0, 0xffff_ffff)? as u32,
        spin_ms: integer("spin-ms", Some(defaults.spin_ms.into()), 0, 100)? as u32,
        max_packets: integer("max-packets", Some(defaults.max_packets as u64), 1, 1_000_000)? as usize,
        max_bytes: integer("max-bytes", Some(defaults.max_bytes as u64), 1, 1 << 30)? as usize,
        max_clients: integer("max-clients", Some(defaults.max_clients as u64), 1, 64)? as usize,
        idle_ms: integer("idle-ms", Some(defaults.idle_ms.into()), 0, 600_000)? as u32,
    };
    let parent_pid = match values.contains_key("parent-pid") {
        true => Some(integer("parent-pid", None, 1, 0xffff_ffff)? as u32),
        false => None,
    };
    let run_dir = PathBuf::from(values.get("run-dir").ok_or("--run-dir is required")?);
    if listen == host {
        return Err("--listen and --host must differ".into());
    }
    Ok(Args { listen, host, config, run_dir, parent_pid })
}

/// A client's socket toward the host.
struct Upstream {
    socket: UdpSocket,
    closed: AtomicBool,
}

struct State {
    relay: Relay,
    upstreams: HashMap<u32, Arc<Upstream>>,
}

/// The running relay: the bookkeeping behind one lock, the sockets and the
/// threads that move datagrams.
struct Running {
    state: Mutex<State>,
    /// Woken when a datagram is queued, so the sender can reschedule.
    queued: Condvar,
    listen: UdpSocket,
    host: SocketAddr,
    started: Instant,
    /// A socket error that should stop the relay.
    failure: Mutex<Option<String>>,
}

impl Running {
    /// Starts relaying between `listen` (bound) and the host on `host_port`.
    fn start(listen: UdpSocket, host_port: u16, config: Config) -> io::Result<Arc<Running>> {
        listen.set_read_timeout(Some(RECV_TIMEOUT))?;
        let running = Arc::new(Running {
            state: Mutex::new(State { relay: Relay::new(config), upstreams: HashMap::new() }),
            queued: Condvar::new(),
            listen,
            host: SocketAddr::V4(SocketAddrV4::new(LOOPBACK, host_port)),
            started: Instant::now(),
            failure: Mutex::new(None),
        });
        let r = running.clone();
        std::thread::Builder::new().name("listen".into()).spawn(move || r.receive_from_clients())?;
        let r = running.clone();
        std::thread::Builder::new().name("deliver".into()).spawn(move || r.deliver())?;
        Ok(running)
    }

    /// Milliseconds since the relay started.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fail(&self, message: String) {
        self.failure.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(message);
    }

    fn failure(&self) -> Option<String> {
        self.failure.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn receive_from_clients(self: Arc<Self>) {
        let mut buffer = vec![0u8; 65536];
        loop {
            let (len, from) = match self.listen.recv_from(&mut buffer) {
                Ok(received) => received,
                Err(e) if quiet(&e) => continue,
                Err(e) => return self.fail(format!("socket error: {e}")),
            };
            let mut state = self.lock();
            let now = self.now();
            if let Admit::New(id) = state.relay.from_client(from, &buffer[..len], now) {
                match self.open_upstream(id) {
                    Ok(upstream) => {
                        state.upstreams.insert(id, upstream);
                    }
                    Err(e) => return self.fail(format!("socket error: {e}")),
                }
            }
            drop(state);
            self.queued.notify_one();
        }
    }

    fn open_upstream(self: &Arc<Self>, id: u32) -> io::Result<Arc<Upstream>> {
        let socket = UdpSocket::bind((LOOPBACK, 0))?;
        socket.set_read_timeout(Some(RECV_TIMEOUT))?;
        let upstream = Arc::new(Upstream { socket, closed: AtomicBool::new(false) });
        let (r, u) = (self.clone(), upstream.clone());
        std::thread::Builder::new().name(format!("upstream{id}")).spawn(move || r.receive_from_host(id, &u))?;
        Ok(upstream)
    }

    fn receive_from_host(&self, id: u32, upstream: &Upstream) {
        let mut buffer = vec![0u8; 65536];
        while !upstream.closed.load(Ordering::Relaxed) {
            let (len, from) = match upstream.socket.recv_from(&mut buffer) {
                Ok(received) => received,
                Err(e) if quiet(&e) => continue,
                Err(e) => return self.fail(format!("socket error: {e}")),
            };
            let mut state = self.lock();
            if from != self.host {
                state.relay.stranger();
                continue;
            }
            let now = self.now();
            state.relay.from_host(id, &buffer[..len], now);
            drop(state);
            self.queued.notify_one();
        }
    }

    /// Sends each datagram when it's due: waits on a timer until --spin-ms
    /// before, then polls.
    fn deliver(self: Arc<Self>) {
        let spin_ms = f64::from(self.lock().relay.config.spin_ms);
        let mut state = self.lock();
        loop {
            let Some(due) = state.relay.next_due() else {
                state = self.queued.wait(state).unwrap_or_else(|e| e.into_inner());
                continue;
            };
            let wait = due - self.now();
            if wait > spin_ms {
                let timeout = Duration::from_secs_f64((wait - spin_ms) / 1000.0);
                state = self.queued.wait_timeout(state, timeout).unwrap_or_else(|e| e.into_inner()).0;
            } else if wait > 0.0 {
                drop(state);
                std::thread::yield_now();
                state = self.lock();
            } else {
                let now = self.now();
                for d in state.relay.take_due(now) {
                    let sent = match d.way {
                        Way::ToHost => match state.upstreams.get(&d.client) {
                            Some(upstream) => upstream.socket.send_to(&d.bytes, self.host),
                            None => Err(ErrorKind::NotConnected.into()),
                        },
                        Way::ToClient => self.listen.send_to(&d.bytes, d.addr),
                    };
                    state.relay.sent(d.client, d.way, d.bytes.len(), sent.is_ok());
                }
            }
        }
    }

    /// Forgets silent clients and closes their sockets toward the host.
    fn retire_idle(&self) {
        let mut state = self.lock();
        let now = self.now();
        for id in state.relay.retire_idle(now, &iso_time(SystemTime::now())) {
            if let Some(upstream) = state.upstreams.remove(&id) {
                upstream.closed.store(true, Ordering::Relaxed);
            }
        }
    }

    fn stats(&self, state: &str, started_at: &str) -> Stats {
        let now = self.now();
        self.lock().relay.stats(state, started_at, now)
    }
}

/// Errors a socket thread shrugs off. Windows reports an earlier send's ICMP
/// "port unreachable" (the host not up yet, a client gone) as a reset on the
/// next receive; it is packet loss, not a reason to stop.
fn quiet(e: &io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset | ErrorKind::Interrupted)
}

/// `ready.json`, written once the listen socket is bound.
#[derive(Serialize)]
struct Ready {
    pid: u32,
    listen: String,
    host: String,
    max_clients: usize,
    idle_ms: u32,
    delay_ms: u32,
    jitter_ms: u32,
    seed: u32,
    spin_ms: u32,
    max_packets: usize,
    max_bytes: usize,
}

/// Writes `value` to `dir/file` through a temporary file, so a reader never
/// sees half of it.
fn write_json(dir: &Path, file: &str, value: &impl Serialize) -> io::Result<()> {
    let target = dir.join(file);
    let temp = dir.join(format!("{file}.tmp"));
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &target)
}

/// `time` as UTC ISO 8601 with milliseconds, like JavaScript's toISOString.
fn iso_time(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        since.subsec_millis()
    )
}

/// Set by Ctrl+C (or SIGINT/SIGTERM), checked by the main loop.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
mod platform {
    use super::INTERRUPTED;
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    }

    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x102;

    /// The parent process, opened once so a reused PID can't fool it.
    pub struct Parent(*mut c_void);

    impl Parent {
        pub fn open(pid: u32) -> Parent {
            Parent(unsafe { OpenProcess(SYNCHRONIZE, 0, pid) })
        }

        pub fn alive(&self) -> bool {
            !self.0.is_null() && unsafe { WaitForSingleObject(self.0, 0) } == WAIT_TIMEOUT
        }
    }

    unsafe extern "system" fn on_ctrl(_event: u32) -> i32 {
        INTERRUPTED.store(true, Ordering::SeqCst);
        // Give the main loop time to write the final stats before a console
        // close ends the process.
        std::thread::sleep(std::time::Duration::from_millis(500));
        1
    }

    pub fn catch_interrupts() {
        unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
    }
}

#[cfg(unix)]
mod platform {
    use super::INTERRUPTED;
    use std::sync::atomic::Ordering;

    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn signal(signal: i32, handler: usize) -> usize;
    }

    pub struct Parent(i32);

    impl Parent {
        pub fn open(pid: u32) -> Parent {
            Parent(pid as i32)
        }

        pub fn alive(&self) -> bool {
            unsafe { kill(self.0, 0) == 0 }
        }
    }

    extern "C" fn on_signal(_signal: i32) {
        INTERRUPTED.store(true, Ordering::SeqCst);
    }

    pub fn catch_interrupts() {
        const SIGINT: i32 = 2;
        const SIGTERM: i32 = 15;
        unsafe {
            signal(SIGINT, on_signal as *const () as usize);
            signal(SIGTERM, on_signal as *const () as usize);
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&argv).unwrap_or_else(|message| {
        eprintln!("relay: {message}");
        std::process::exit(2);
    });
    let started_at = iso_time(SystemTime::now());
    let listen = UdpSocket::bind((LOOPBACK, args.listen)).unwrap_or_else(|e| {
        eprintln!("relay: cannot bind: {e}");
        std::process::exit(2);
    });
    let config = args.config.clone();
    let running = Running::start(listen, args.host, config.clone()).unwrap_or_else(|e| {
        eprintln!("relay: cannot start: {e}");
        std::process::exit(2);
    });
    platform::catch_interrupts();
    let parent = args.parent_pid.map(platform::Parent::open);

    let ready = Ready {
        pid: std::process::id(),
        listen: format!("{LOOPBACK}:{}", args.listen),
        host: format!("{LOOPBACK}:{}", args.host),
        max_clients: config.max_clients,
        idle_ms: config.idle_ms,
        delay_ms: config.delay_ms,
        jitter_ms: config.jitter_ms,
        seed: config.seed,
        spin_ms: config.spin_ms,
        max_packets: config.max_packets,
        max_bytes: config.max_bytes,
    };
    if let Err(e) = write_json(&args.run_dir, "ready.json", &ready) {
        eprintln!("relay: cannot write ready.json: {e}");
        std::process::exit(2);
    }
    println!("relay: ready on {LOOPBACK}:{} -> {LOOPBACK}:{}", args.listen, args.host);

    let stop = |code: i32| -> ! {
        if let Err(e) = write_json(&args.run_dir, "stats.json", &running.stats("stopped", &started_at)) {
            eprintln!("relay: cannot write stats: {e}");
        }
        std::process::exit(code);
    };
    let mut next_tick = Instant::now() + Duration::from_secs(1);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if INTERRUPTED.load(Ordering::SeqCst) {
            stop(0);
        }
        if let Some(message) = running.failure() {
            eprintln!("relay: {message}");
            stop(1);
        }
        if Instant::now() < next_tick {
            continue;
        }
        next_tick += Duration::from_secs(1);
        if args.run_dir.join("stop").exists() || parent.as_ref().is_some_and(|p| !p.alive()) {
            stop(0);
        }
        running.retire_idle();
        // A reader holding the file can make this fail; the next second retries.
        if let Err(e) = write_json(&args.run_dir, "stats.json", &running.stats("running", &started_at)) {
            eprintln!("relay: cannot write stats: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_what_the_lab_passes() {
        let args = parse_args(&strings(&[
            "--listen", "7778", "--host", "7777", "--delay-ms", "75", "--jitter-ms", "20", "--seed", "4294967295",
            "--spin-ms", "0", "--max-clients", "7", "--run-dir", "C:\\runs\\x", "--parent-pid", "1234",
        ]))
        .unwrap();
        assert_eq!(args.listen, 7778);
        assert_eq!(args.host, 7777);
        assert_eq!(
            args.config,
            Config { delay_ms: 75, jitter_ms: 20, seed: u32::MAX, spin_ms: 0, max_clients: 7, ..Config::default() }
        );
        assert_eq!(args.run_dir, PathBuf::from("C:\\runs\\x"));
        assert_eq!(args.parent_pid, Some(1234));
    }

    #[test]
    fn defaults_and_inline_values() {
        let args =
            parse_args(&strings(&["--listen=1", "--host=2", "--delay-ms=0", "--jitter-ms=0", "--seed=0", "--run-dir=d"]))
                .unwrap();
        assert_eq!(args.config, Config::default());
        assert_eq!(args.parent_pid, None);
    }

    #[test]
    fn rejects_bad_arguments() {
        let base = ["--listen", "1", "--host", "2", "--delay-ms", "0", "--jitter-ms", "0", "--seed", "0", "--run-dir", "d"];
        let with = |extra: &[&str]| {
            let mut all = base.to_vec();
            all.extend_from_slice(extra);
            parse_args(&strings(&all))
        };
        assert!(with(&[]).is_ok());
        assert_eq!(with(&["--seed", "4294967296"]).unwrap_err(), "--seed must be an integer from 0 to 4294967295, got 4294967296");
        assert!(with(&["--delay-ms", "-1"]).is_err());
        assert!(with(&["--max-clients", "65"]).is_err());
        assert!(with(&["--spin-ms", "1.5"]).is_err());
        assert_eq!(with(&["--host", "1"]).unwrap_err(), "--listen and --host must differ");
        assert_eq!(with(&["--bogus", "1"]).unwrap_err(), "unknown option --bogus");
        assert_eq!(with(&["--seed"]).unwrap_err(), "--seed needs a value");
        assert_eq!(
            parse_args(&strings(&["--listen", "1", "--host", "2"])).unwrap_err(),
            "--delay-ms must be an integer from 0 to 10000, got nothing"
        );
    }

    #[test]
    fn iso_time_matches_javascript() {
        assert_eq!(iso_time(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // new Date(1793103338123).toISOString()
        let t = UNIX_EPOCH + Duration::from_millis(1_793_103_338_123);
        assert_eq!(iso_time(t), "2026-10-27T12:15:38.123Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(iso_time(leap), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn writes_json_files_whole() {
        let dir = std::env::var_os("TEST_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = dir.join(format!("relay-json-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_json(&dir, "stats.json", &serde_json::json!({"a": 1})).unwrap();
        write_json(&dir, "stats.json", &serde_json::json!({"a": 2})).unwrap();
        let text = std::fs::read_to_string(dir.join("stats.json")).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["a"], 2);
        assert!(!dir.join("stats.json.tmp").exists());
    }

    fn socket() -> UdpSocket {
        let s = UdpSocket::bind((LOOPBACK, 0)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s
    }

    fn receive(s: &UdpSocket) -> (Vec<u8>, SocketAddr) {
        let mut buffer = [0u8; 2048];
        let (len, from) = s.recv_from(&mut buffer).expect("a datagram through the relay");
        (buffer[..len].to_vec(), from)
    }

    /// Sends `bytes` from `from` to `to` and waits for them at `at`; returns
    /// where they came from and how long they took.
    fn through(from: &UdpSocket, to: SocketAddr, at: &UdpSocket, bytes: &[u8]) -> (SocketAddr, Duration) {
        let sent = Instant::now();
        from.send_to(bytes, to).unwrap();
        let (got, source) = receive(at);
        assert_eq!(got, bytes);
        (source, sent.elapsed())
    }

    #[test]
    fn relays_both_ways_after_the_delay_with_a_socket_per_client() {
        const DELAY: Duration = Duration::from_millis(60);
        let host = socket();
        let host_port = host.local_addr().unwrap().port();
        let listen = UdpSocket::bind((LOOPBACK, 0)).unwrap();
        let relay_addr = listen.local_addr().unwrap();
        let config = Config { delay_ms: 60, jitter_ms: 0, seed: 1, ..Config::default() };
        let running = Running::start(listen, host_port, config).unwrap();

        let (a, b) = (socket(), socket());
        let (a_upstream, took) = through(&a, relay_addr, &host, b"from a");
        assert!(took >= DELAY, "client to host took {took:?}");
        assert_ne!(a_upstream, a.local_addr().unwrap());
        let (source, took) = through(&host, a_upstream, &a, b"to a");
        assert_eq!(source, relay_addr, "replies come from the address the client joined");
        assert!(took >= DELAY, "host to client took {took:?}");

        let (b_upstream, _) = through(&b, relay_addr, &host, b"from b");
        assert_ne!(b_upstream, a_upstream, "each client gets its own socket toward the host");
        let (source, _) = through(&host, b_upstream, &b, b"to b");
        assert_eq!(source, relay_addr);
        // A's socket still reaches A.
        through(&host, a_upstream, &a, b"to a again");

        // Only the host may talk to a client's upstream socket.
        b.send_to(b"stray", a_upstream).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let stats = running.stats("running", "t");
        assert_eq!(stats.active_clients, 2);
        assert_eq!(stats.stranger_drops, 1);
        assert_eq!((stats.client_to_host.received, stats.client_to_host.delivered), (2, 2));
        assert_eq!((stats.host_to_client.received, stats.host_to_client.delivered), (3, 3));
        assert_eq!(stats.clients[0].host_to_client.delivered, 2);
        assert_eq!(stats.clients[0].port, a.local_addr().unwrap().port());
        assert_eq!(stats.client_to_host.lateness_ms.samples, 2);
        assert!(stats.client_to_host.lateness_ms.p99.unwrap() >= 0.0);
    }

    #[test]
    fn forgets_a_silent_client_and_serves_its_rejoin() {
        let host = socket();
        let host_port = host.local_addr().unwrap().port();
        let listen = UdpSocket::bind((LOOPBACK, 0)).unwrap();
        let relay_addr = listen.local_addr().unwrap();
        let config = Config { max_clients: 1, idle_ms: 100, ..Config::default() };
        let running = Running::start(listen, host_port, config).unwrap();

        let first = socket();
        let (first_upstream, _) = through(&first, relay_addr, &host, b"hello");
        let second = socket();
        second.send_to(b"turned away", relay_addr).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        running.retire_idle();
        let (second_upstream, _) = through(&second, relay_addr, &host, b"rejoined");
        assert_ne!(second_upstream, first_upstream);

        let stats = running.stats("running", "t");
        assert_eq!(stats.stranger_drops, 1);
        assert_eq!(stats.active_clients, 1);
        assert_eq!(stats.clients.len(), 2);
        assert!(stats.clients[0].retired_at.is_some());
    }
}

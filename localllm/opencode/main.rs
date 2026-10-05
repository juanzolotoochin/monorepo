use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, ExitCode, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        // The server and its engine have a separate process group. Never kill an
        // existing service that happens to occupy the requested port.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn runfiles() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(p) = env::var_os("RUNFILES_DIR") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(p) = env::args_os().next() {
        let mut p = p;
        p.push(".runfiles");
        candidates.push(PathBuf::from(p));
    }
    let mut p = env::current_exe()?.into_os_string();
    p.push(".runfiles");
    candidates.push(PathBuf::from(p));
    candidates
        .into_iter()
        .find(|p| p.join("_main/localllm/opencode/assets.json").is_file())
        .context("Bazel runfiles missing; start with bazel run //localllm/opencode")?
        .canonicalize()
        .context("resolve Bazel runfiles")
}

fn ready(address: SocketAddr, key: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(200)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    if write!(stream, "GET /health HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nConnection: close\r\n\r\n").is_err() { return false; }
    let mut response = String::new();
    if stream
        .take(64 * 1024)
        .read_to_string(&mut response)
        .is_err()
    {
        return false;
    }
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return false;
    };
    headers.lines().next().is_some_and(|l| l.contains(" 200 "))
        && serde_json::from_str::<Value>(body).is_ok_and(|v| v["status"] == "ok")
}

fn context_window(value: Option<&str>) -> Result<u32> {
    let context = value
        .unwrap_or("65536")
        .parse::<u32>()
        .context("LOCALLLM_CONTEXT must be 32768, 65536, or 131072")?;
    if !matches!(context, 32768 | 65536 | 131072) {
        bail!("LOCALLLM_CONTEXT must be 32768, 65536, or 131072 (tested capacities)");
    }
    Ok(context)
}

fn provider(port: u16, key: &str, context: u32, quantization: &str) -> Value {
    json!({
        "model": "strata/local", "small_model": "strata/local",
        "enabled_providers": ["strata"], "autoupdate": false,
        "share": "disabled", "plugin": [], "mcp": {}, "lsp": false, "formatter": false,
        // Explicit input limit makes this OpenCode version honor `reserved`.
        // Leave 16K output plus 4K for new tool results before compacting.
        "compaction": {"auto": true, "prune": true, "reserved": 20480,
            "preserve_recent_tokens": 4096},
        "tool_output": {"max_lines": 400, "max_bytes": 16000},
        "provider": {"strata": {
            "npm": "@ai-sdk/openai-compatible", "name": "Strata (local)",
            "options": {"baseURL": format!("http://127.0.0.1:{port}/v1"), "apiKey": key},
            "models": {"local": {"name": format!("Qwen3.8 Flash Next {quantization}"), "tool_call": true,
                "limit": {"context": context, "input": context, "output": 16384}}}
        }}
    })
}

fn run() -> Result<i32> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--launcher-help") {
        println!(
            "bazel run //localllm/opencode -- [OpenCode arguments]\n\
          --check: load the real model, check readiness, then stop\n\
          LOCALLLM_CONTEXT: context tokens, 32768 / 65536 (default) / 131072\n\
          LOCALLLM_PORT: local server port (default: choose an unused port)\n\
          LOCALLLM_STARTUP_TIMEOUT: seconds to wait for model loading (default: 600)"
        );
        return Ok(0);
    }
    let context = context_window(env::var("LOCALLLM_CONTEXT").ok().as_deref())?;
    let root = runfiles()?;
    let assets: Value =
        serde_json::from_slice(&fs::read(root.join("_main/localllm/opencode/assets.json"))?)?;
    let quantization = assets["quantization"]
        .as_str()
        .context("missing model quantization")?;
    let asset = |name: &str| -> Result<PathBuf> {
        let p = root.join(
            assets[name]
                .as_str()
                .with_context(|| format!("missing asset {name}"))?,
        );
        if !p.exists() {
            bail!("missing asset: {}", p.display());
        }
        Ok(p)
    };
    let engine = asset("engine")?;
    let server = asset("server")?;
    let opencode = asset("opencode")?;
    let pack = asset("pack")?;
    let draft = asset("draft")?;
    let profile = asset("profile")?;
    let ripgrep = asset("ripgrep")?;
    let mut shards: Vec<PathBuf> = assets["shards"]
        .as_array()
        .context("missing model shards")?
        .iter()
        .map(|p| root.join(p.as_str().unwrap()))
        .collect();
    shards.sort();
    if shards.len() != 2 || shards.iter().any(|p| !p.is_file()) {
        bail!("expected two pinned model shards");
    }
    let port: u16 = env::var("LOCALLLM_PORT")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .context("invalid LOCALLLM_PORT")?;
    let reservation =
        TcpListener::bind(("127.0.0.1", port)).context("local port is already in use")?;
    let address = reservation.local_addr()?;
    let state = tempfile::Builder::new().prefix("localllm-").tempdir()?;
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let key: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let config = state.path().join("strata.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&json!({
            "exe": engine, "cwd": state.path(), "tokenizer": pack.join("tokenizer"),
            "model_name": "local", "backend": "cuda",
            // Leave about 4K output tokens for an answer after long reasoning.
            "reasoning_budget_tokens": 12288,
            "args": ["--pack", pack, "--native", shards[0], "--ple-gguf", shards[1],
                "--expert-profile", profile, "--expert-cache", "auto",
                // Verification graphs are instantiated lazily across requests.
                // Keep room for them and desktop use instead of filling VRAM with experts.
                "--vram-reserve-mib", "3072", "--prefill", "auto",
                "--spec", "4", "--spec-min-p", "0.5", "--mtp", draft, "--max-context", context.to_string(), "--kv", "int8"],
            "log": state.path().join("engine.log")
        }))?,
    )?;
    let library_paths: Vec<_> = assets["libraries"]
        .as_array()
        .context("missing CUDA libraries")?
        .iter()
        .map(|p| root.join(p.as_str().unwrap()))
        .collect();
    let log_path = state.path().join("server.log");
    let log = fs::File::create(&log_path)?;
    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }
    drop(reservation);
    eprintln!(
        "Loading Strata {quantization} on {address}; startup logs: {}",
        log_path.display()
    );
    let mut child = Server(
        Command::new(server)
            .args(["--fit-max-tokens", "--engine", "strata", "--config"])
            .arg(config)
            .args(["--host", "127.0.0.1", "--port", &address.port().to_string()])
            .env("STRATA_API_KEY", &key)
            .env("LD_LIBRARY_PATH", env::join_paths(library_paths)?)
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONNOUSERSITE", "1")
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            .current_dir(state.path())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0)
            .spawn()?,
    );
    let timeout: u64 = env::var("LOCALLLM_STARTUP_TIMEOUT")
        .unwrap_or_else(|_| "600".into())
        .parse()?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if STOP.load(Ordering::Relaxed) {
            return Ok(130);
        }
        if let Some(status) = child.0.try_wait()? {
            bail!(
                "Strata exited with {status}:\n{}",
                fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        if ready(address, &key) {
            break;
        }
        if Instant::now() >= deadline {
            bail!(
                "Strata startup timed out:\n{}",
                fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        thread::sleep(Duration::from_millis(250));
    }
    if args.first().is_some_and(|a| a == "--check") {
        eprintln!("Strata is ready.");
        return Ok(0);
    }
    let workspace = env::var_os("BUILD_WORKING_DIRECTORY")
        .or_else(|| env::var_os("BUILD_WORKSPACE_DIRECTORY"))
        .map(PathBuf::from)
        .unwrap_or(env::current_dir()?);
    let persistent = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .context("HOME or XDG_DATA_HOME must be set for OpenCode sessions")?
        .join("localllm");
    fs::create_dir_all(&persistent)?;
    let mut command = Command::new(opencode);
    // Isolate app configuration; retain HOME and the workspace for coding tools.
    for (name, _) in env::vars_os() {
        if name.to_string_lossy().starts_with("OPENCODE_") {
            command.env_remove(name);
        }
    }
    command
        .args(&args)
        .current_dir(&workspace)
        .env("PWD", &workspace)
        .env("XDG_CONFIG_HOME", state.path().join("config"))
        .env("XDG_DATA_HOME", &persistent)
        .env("XDG_CACHE_HOME", persistent.join("cache"))
        .env("XDG_STATE_HOME", persistent.join("state"))
        .env(
            "OPENCODE_CONFIG_CONTENT",
            provider(address.port(), &key, context, quantization).to_string(),
        );
    command
        .env("OPENCODE_HERMETIC", "1")
        .env("LOCALLLM_RIPGREP", ripgrep);
    for name in [
        "OPENCODE_DISABLE_AUTOUPDATE",
        "OPENCODE_DISABLE_MODELS_FETCH",
        "OPENCODE_DISABLE_PROJECT_CONFIG",
        "OPENCODE_DISABLE_DEFAULT_PLUGINS",
        "OPENCODE_DISABLE_LSP_DOWNLOAD",
        "OPENCODE_PURE",
    ] {
        command.env(name, "1");
    }
    let mut client = command.spawn()?;
    loop {
        if let Some(status) = client.try_wait()? {
            return Ok(status.code().unwrap_or(130));
        }
        if STOP.load(Ordering::Relaxed) {
            // Let OpenCode restore terminal state before resorting to SIGKILL.
            unsafe {
                libc::kill(client.id() as i32, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if client.try_wait()?.is_some() {
                    return Ok(130);
                }
                thread::sleep(Duration::from_millis(100));
            }
            let _ = client.kill();
            let _ = client.wait();
            return Ok(130);
        }
        if let Some(status) = child.0.try_wait()? {
            let _ = client.kill();
            let _ = client.wait();
            bail!("Strata stopped unexpectedly: {status}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("localllm: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_uses_local_model_for_all_requests() {
        let config = provider(12345, "test-key", 65536, "IQ3_XXS");
        assert_eq!(config["model"], config["small_model"]);
        let output = config["provider"]["strata"]["models"]["local"]["limit"]["output"]
            .as_u64()
            .unwrap();
        let reserved = config["compaction"]["reserved"].as_u64().unwrap();
        assert!(reserved >= output + 4096);
        assert!(reserved < 32768);
        assert_eq!(
            config["provider"]["strata"]["models"]["local"]["name"],
            "Qwen3.8 Flash Next IQ3_XXS"
        );
        assert_eq!(
            config["provider"]["strata"]["options"]["baseURL"],
            "http://127.0.0.1:12345/v1"
        );
    }
    #[test]
    fn context_setting_rejects_untested_or_malformed_values() {
        assert_eq!(context_window(None).unwrap(), 65536);
        for context in [32768, 65536, 131072] {
            assert_eq!(context_window(Some(&context.to_string())).unwrap(), context);
        }
        for value in ["0", "8192", "262144", "64k", "-1"] {
            assert!(context_window(Some(value)).is_err(), "{value}");
        }
    }
    #[test]
    fn readiness_rejects_an_unrelated_http_service() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}")
                .unwrap();
        });
        assert!(!ready(address, "key"));
        worker.join().unwrap();
    }
    #[test]
    fn dropping_server_reaps_child() {
        let child = Command::new(env::current_exe().unwrap())
            .args(["--ignored", "--exact", "tests::child_process"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        drop(Server(child));
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
    #[test]
    #[ignore]
    fn child_process() {
        thread::sleep(Duration::from_secs(60));
    }
}

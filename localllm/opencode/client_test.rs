mod config;

use serde_json::{json, Value};
use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn client(workspace: &Path, config: &Value, args: &[&str]) -> String {
    let runfile = |name| env::current_dir().unwrap().join(env::var(name).unwrap());
    let binary = runfile("CLIENT_DIST").join("opencode");
    let mut command = Command::new(binary);
    for (key, _) in env::vars_os() {
        if key.to_string_lossy().starts_with("OPENCODE_") {
            command.env_remove(key);
        }
    }
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        command.env(key, workspace.join(key));
    }
    let mut child = command
        .current_dir(workspace)
        .env("PWD", workspace)
        .env("OPENCODE_HERMETIC", "1")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_CONFIG_CONTENT", config.to_string())
        .env("LOCALLLM_RIPGREP", runfile("RIPGREP"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "client timed out: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn request(stream: &mut TcpStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        headers.push(byte[0]);
    }
    let headers = String::from_utf8(headers).unwrap();
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    assert!(headers
        .to_lowercase()
        .contains("authorization: bearer test-key\r\n"));
    let size: usize = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    let mut body = vec![0; size];
    stream.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn compiled_client_uses_isolated_config_and_completes_a_tool_call() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(workspace.path().join("fixture.txt"), "LOCAL_TOOL_RESULT\n").unwrap();
    // An ordinary project config must not override the launcher's local provider.
    fs::write(
        workspace.path().join("opencode.json"),
        r#"{"model":"unwanted/remote","plugins":["missing-plugin-for-isolation-test"]}"#,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = config::provider(
        listener.local_addr().unwrap().port(),
        "test-key",
        65536,
        "IQ3_XXS",
    );
    assert_eq!(
        client(workspace.path(), &config, &["--version"]).trim(),
        "opencode v2.0.32-local"
    );
    let entries: Value =
        serde_json::from_str(&client(workspace.path(), &config, &["debug", "config"])).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    let loaded = &entries[0]["info"];
    assert_eq!(
        loaded["model"],
        json!({"providerID":"strata", "model":"local"})
    );
    assert_eq!(loaded["compaction"], config["compaction"]);
    assert_eq!(
        loaded["providers"]["strata"]["models"],
        config["providers"]["strata"]["models"]
    );
    assert_eq!(
        client(workspace.path(), &config, &["models"]).trim(),
        "strata/local"
    );

    let backend = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        for turn in 0..2 {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "client did not contact the local provider"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let body = request(&mut stream);
            assert_eq!(body["model"], "local");
            assert_eq!(
                body.get("max_tokens")
                    .or_else(|| body.get("max_completion_tokens")),
                Some(&json!(16384))
            );
            if turn == 1 {
                assert!(
                    body["messages"].as_array().unwrap().iter().any(|message| {
                        message["role"] == "tool"
                            && message["content"].to_string().contains("LOCAL_TOOL_RESULT")
                    }),
                    "tool result missing from the follow-up request"
                );
            }
            let (delta, finish) = if turn == 0 {
                (
                    json!({"role":"assistant", "tool_calls":[{"index":0,"id":"read_fixture","type":"function","function":{"name":"read","arguments":"{\"path\":\"fixture.txt\"}"}}]}),
                    "tool_calls",
                )
            } else {
                (json!({"role":"assistant", "content":"LOCAL_OK"}), "stop")
            };
            let chunk = |delta, finish| {
                json!({"id":"smoke", "object":"chat.completion.chunk", "created":0, "model":"local", "choices":[{"index":0,"delta":delta,"finish_reason":finish}]}).to_string()
            };
            let response = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                chunk(delta, Value::Null),
                chunk(json!({}), json!(finish))
            );
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        }
    });
    let output = client(
        workspace.path(),
        &config,
        &[
            "run",
            "--title",
            "Smoke",
            "--format",
            "json",
            "Read fixture.txt, then reply LOCAL_OK",
        ],
    );
    backend.join().unwrap();
    assert!(
        output
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|event| { event["type"] == "text" && event["part"]["text"] == "LOCAL_OK" }),
        "client did not return the local model's answer: {output}"
    );
}

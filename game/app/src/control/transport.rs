//! Local-only JSON-lines transport; the ECS thread remains the sole world owner.
use super::protocol::Request;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

pub struct Envelope {
    pub request: Request,
    pub reply: Option<mpsc::SyncSender<Value>>,
    pub scripted: bool,
}
pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn start(
        port: u16,
        tx: mpsc::SyncSender<Envelope>,
        address_file: Option<&str>,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        if let Some(path) = address_file {
            std::fs::write(path, address.to_string()).map_err(|e| e.to_string())?;
        }
        eprintln!("Game control listening at {address}");
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let handle = thread::spawn(move || {
            while !signal.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let tx = tx.clone();
                        thread::spawn(move || client(stream, tx));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            thread: Some(handle),
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
fn client(mut stream: TcpStream, tx: mpsc::SyncSender<Envelope>) {
    let Ok(read) = stream.try_clone() else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(read);
    loop {
        let mut line = String::new();
        match reader.by_ref().take(65537).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let response = if line.len() > 65536 {
            json!({"status":"failed","error":"Request exceeds 64 KiB"})
        } else {
            match serde_json::from_str::<Request>(&line) {
                Err(e) => json!({"status":"failed","error":e.to_string()}),
                Ok(request) => {
                    let (reply, rx) = mpsc::sync_channel(1);
                    match tx.try_send(Envelope {
                        request,
                        reply: Some(reply),
                        scripted: false,
                    }) {
                        Err(_) => {
                            json!({"status":"failed","error":"Command queue unavailable or full"})
                        }
                        Ok(()) => rx.recv_timeout(Duration::from_secs(120)).unwrap_or_else(
                            |_| json!({"status":"failed","error":"Command response timed out"}),
                        ),
                    }
                }
            }
        };
        if writeln!(stream, "{response}").is_err() {
            return;
        }
    }
}

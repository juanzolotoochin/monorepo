//! The Bevy side of the session: wall-clock time, autosaves, desync dumps.

use std::time::Instant;

use bevy::prelude::Resource;
use content::Content;
use net::{NetConfig, NetStats, StableId};
use sim::{CommandKind, World};
use view::Extract;

pub use session::Mode;

#[derive(Resource)]
pub struct Session {
    inner: session::Session,
    start: Instant,
    last_update_ms: f64,
    /// Things for the app to tell the player about or act on.
    pub events: Vec<net::NetEvent>,
}

impl Session {
    pub fn start(mode: Mode, content: Content, config: NetConfig) -> std::io::Result<Session> {
        let inner = session::Session::start(mode, content, config)?;
        Ok(Session { inner, start: Instant::now(), last_update_ms: 0.0, events: Vec::new() })
    }

    pub fn world(&self) -> Option<&World> {
        self.inner.world()
    }

    pub fn queue(&mut self, kind: CommandKind) {
        self.inner.queue(kind);
    }

    /// Runs the network and every tick due. Returns the extract of the last
    /// tick run, if any.
    pub fn update(&mut self) -> Option<Extract> {
        let started = Instant::now();
        let stepped = self.inner.update(self.start.elapsed(), |_| {});
        self.events.append(&mut self.inner.events);
        if let Some(bytes) = self.inner.autosave.take() {
            write_file("autosave.gsav", &bytes);
        }
        if self.events.iter().any(|e| matches!(e, net::NetEvent::HostLost)) {
            if let Some(bytes) = self.inner.save() {
                write_file("hostlost.gsav", &bytes);
            }
        }
        if stepped == 0 {
            return None;
        }
        self.last_update_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.world().map(Extract::from_world)
    }

    pub fn alpha(&self) -> f32 {
        self.inner.alpha(self.start.elapsed())
    }

    pub fn input_delay(&self) -> u64 {
        self.inner.input_delay()
    }

    pub fn last_update_ms(&self) -> f64 {
        self.last_update_ms
    }

    pub fn stats(&self) -> NetStats {
        self.inner.stats()
    }

    pub fn status(&self) -> String {
        self.inner.status()
    }

    pub fn save(&self) -> Option<Vec<u8>> {
        self.inner.save()
    }

    /// Writes the evidence for a desync: this peer's state and hash log.
    pub fn dump_desync(&self, tick: u64) {
        let role = if self.inner.is_host() { "host" } else { "client" };
        let (save, hashes) = self.inner.desync_evidence();
        if let Some(bytes) = save {
            write_file(&format!("desync-{tick}-{role}.gsav"), &bytes);
        }
        write_file(&format!("desync-{tick}-{role}.hashes"), hashes.as_bytes());
    }

    pub fn shutdown(&mut self) {
        self.inner.shutdown();
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

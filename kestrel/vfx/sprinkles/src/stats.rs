//! Kestrel: per-frame cost counters for profiling (VFX spike step 4, see
//! `UPSTREAM.md`). The fork's hot systems add their CPU time and the GPU
//! objects they create; a profiler reads and resets them with [`take`] once
//! per frame. Relaxed atomics: a few nanoseconds per system, nothing per particle.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
// std::time panics on wasm32-unknown-unknown.
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// One counter.
pub struct Counter(AtomicU64);

impl Counter {
    const fn new() -> Self {
        Counter(AtomicU64::new(0))
    }

    pub(crate) fn add(&self, value: u64) {
        self.0.fetch_add(value, Ordering::Relaxed);
    }

    /// Starts timing; the elapsed nanoseconds are added when the guard drops.
    pub(crate) fn time(&'static self) -> Timer {
        Timer(self, Instant::now())
    }
}

pub(crate) struct Timer(&'static Counter, Instant);

impl Drop for Timer {
    fn drop(&mut self) {
        self.0.add(self.1.elapsed().as_nanos() as u64);
    }
}

/// Returns a counter's value and resets it.
pub fn take(counter: &Counter) -> u64 {
    counter.0.swap(0, Ordering::Relaxed)
}

/// Main world: `setup_particle_systems` (both passes), ns.
pub static SETUP_NS: Counter = Counter::new();
/// Main world: the per-frame time step, uniform writes and syncs, ns.
pub static UPDATE_NS: Counter = Counter::new();
/// Extraction of particle systems and colliders, ns.
pub static EXTRACT_NS: Counter = Counter::new();
/// Render world: `prepare_particle_compute_bind_groups`, ns.
pub static PREPARE_COMPUTE_NS: Counter = Counter::new();
/// Render world: `prepare_particle_sort_bind_groups`, ns.
pub static PREPARE_SORT_NS: Counter = Counter::new();
/// Render world: GPU buffers created by the two prepare systems.
pub static BUFFERS_CREATED: Counter = Counter::new();
/// Render world: bind groups created by the two prepare systems.
pub static BIND_GROUPS_CREATED: Counter = Counter::new();

/// Whether the prepare systems keep their uniform buffers and bind groups
/// from frame to frame (the default). `KESTREL_SPRINKLES_NO_REUSE=1` turns
/// it off, for comparing against upstream's per-frame allocation.
pub fn reuse_gpu_resources() -> bool {
    static CHECKED: AtomicBool = AtomicBool::new(false);
    static REUSE: AtomicBool = AtomicBool::new(true);
    if !CHECKED.swap(true, Ordering::Relaxed) {
        let off = std::env::var("KESTREL_SPRINKLES_NO_REUSE").is_ok_and(|v| v == "1");
        REUSE.store(!off, Ordering::Relaxed);
    }
    REUSE.load(Ordering::Relaxed)
}

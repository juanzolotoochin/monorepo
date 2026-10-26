//! A read-only network view. Samples survive the Session layer's teardown.
use super::{egui, json, Duration, Instant, Value, VecDeque};
use bevy::prelude::{Color, Gizmos, Isometry3d, Transform, Vec3};

const HISTORY: Duration = Duration::from_secs(10);
const MAX_SAMPLES: usize = 600;
/// How long a correction stays drawn in the world after it ends.
const CORRECTION_SHOWN: Duration = Duration::from_secs(2);
/// Episodes that pull the avatar less than this (units) are not kept.
const MIN_CORRECTION: f32 = 0.05;
/// A frame pulling less than this does not count, so float noise at the
/// leash's edge does not hold an episode open.
const MIN_FRAME_CORRECTION: f32 = 0.001;
const MAX_CORRECTIONS: usize = 256;

pub struct NetworkFrame {
    pub at: Instant,
    pub role: &'static str,
    pub status: String,
    pub world: u32,
    pub tick: Option<u64>,
    pub pending: u64,
    pub stats: net::NetStats,
    pub diagnostics: net::NetDiagnostics,
    pub update_ms: f64,
    pub tick_ms: Option<f64>,
    pub frame_ms: Option<f64>,
}

#[derive(Default)]
pub struct NetworkDiagnostics {
    frames: VecDeque<NetworkFrame>,
    preview: Option<PreviewSample>,
    preview_clamps: u64,
    preview_snaps: u64,
    /// Finished correction episodes this session, oldest first.
    corrections: VecDeque<PreviewCorrection>,
    /// The episode the latest preview frames belong to.
    open_correction: Option<PreviewCorrection>,
    /// Episodes dropped for being under `MIN_CORRECTION`: count and distance.
    small_corrections: (u32, f32),
    last_shown: Option<Vec3>,
    /// Draw recent corrections in the world. Kept across sessions.
    pub show_corrections: bool,
}

/// One frame of the client's local avatar preview, in view space.
pub struct PreviewUpdate {
    /// Where the avatar is drawn this frame.
    pub shown: Vec3,
    /// Where the confirmed simulation has it.
    pub confirmed: Vec3,
    /// Distance the preview eased toward the confirmed position.
    pub eased: f32,
    /// Distance the leash held the preview back from where it would have gone.
    pub clamped: f32,
    /// The preview was too far off and reset onto the confirmed position.
    pub snapped: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorrectionKind {
    Ease,
    Clamp,
    Snap,
}

impl CorrectionKind {
    fn name(self) -> &'static str {
        match self {
            Self::Ease => "ease",
            Self::Clamp => "clamp",
            Self::Snap => "snap",
        }
    }

    fn color(self, alpha: f32) -> Color {
        match self {
            Self::Ease => Color::srgba(1.0, 0.85, 0.2, alpha),
            Self::Clamp => Color::srgba(1.0, 0.35, 0.15, alpha),
            Self::Snap => Color::srgba(1.0, 0.2, 0.9, alpha),
        }
    }
}

/// Consecutive preview frames pulled back the same way: one movement preview
/// reconciliation as the player feels it.
#[derive(Clone, Debug)]
pub struct PreviewCorrection {
    pub kind: CorrectionKind,
    pub started: Instant,
    pub ended: Instant,
    /// Where the avatar was drawn just before the episode.
    pub from: Vec3,
    /// Where it was drawn on the episode's last frame.
    pub to: Vec3,
    /// Total distance pulled, summed over frames.
    pub distance: f32,
    pub frames: u32,
    pub tick: Option<u64>,
    pub pending: Option<u64>,
    pub rtt: Option<Duration>,
}

impl PreviewCorrection {
    fn kept(&self) -> bool {
        self.kind == CorrectionKind::Snap || self.distance >= MIN_CORRECTION
    }
}

struct PreviewSample {
    at: Instant,
    offset: f32,
    correction: f32,
}

impl NetworkDiagnostics {
    pub fn begin(&mut self) {
        *self = Self {
            show_corrections: self.show_corrections,
            ..Self::default()
        };
    }

    pub fn record(&mut self, frame: NetworkFrame) {
        // A map change restarts tick counters and input diagnostics. Never graph
        // those across a world boundary as though they were continuous.
        if self
            .frames
            .back()
            .is_some_and(|last| last.world != frame.world)
        {
            self.begin();
        }
        while self
            .frames
            .front()
            .is_some_and(|first| frame.at.duration_since(first.at) > HISTORY)
            || self.frames.len() >= MAX_SAMPLES
        {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// The latest sample while a session is recording them. History kept
    /// after the session ended is not current.
    pub fn current(&self, now: Instant) -> Option<&NetworkFrame> {
        self.frames
            .back()
            .filter(|frame| now.saturating_duration_since(frame.at) <= Duration::from_secs(1))
    }

    pub fn preview(&mut self, now: Instant, update: PreviewUpdate) {
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).length();
        self.preview = Some(PreviewSample {
            at: now,
            offset: flat(update.shown - update.confirmed),
            correction: update.eased,
        });
        self.preview_clamps += u64::from(update.clamped > 0.0);
        self.preview_snaps += u64::from(update.snapped);

        let (kind, distance) = if update.snapped {
            let from = self.last_shown.unwrap_or(update.shown);
            (Some(CorrectionKind::Snap), flat(update.shown - from))
        } else if update.clamped > MIN_FRAME_CORRECTION {
            (Some(CorrectionKind::Clamp), update.clamped)
        } else if update.eased > MIN_FRAME_CORRECTION {
            (Some(CorrectionKind::Ease), update.eased)
        } else {
            (None, 0.0)
        };
        if self.open_correction.as_ref().is_some_and(|open| Some(open.kind) != kind) {
            self.close_correction();
        }
        if let Some(kind) = kind {
            let from = self.last_shown.unwrap_or(update.shown);
            let latest = self.frames.back();
            let open = self.open_correction.get_or_insert_with(|| PreviewCorrection {
                kind,
                started: now,
                ended: now,
                from,
                to: update.shown,
                distance: 0.0,
                frames: 0,
                tick: latest.and_then(|f| f.tick),
                pending: latest.map(|f| f.pending),
                rtt: latest.and_then(|f| f.stats.rtt),
            });
            open.ended = now;
            open.to = update.shown;
            open.distance += distance;
            open.frames += 1;
            if kind == CorrectionKind::Snap {
                self.close_correction();
            }
        }
        self.last_shown = Some(update.shown);
    }

    fn close_correction(&mut self) {
        let Some(correction) = self.open_correction.take() else { return };
        if !correction.kept() {
            self.small_corrections.0 += 1;
            self.small_corrections.1 += correction.distance;
            return;
        }
        if self.corrections.len() >= MAX_CORRECTIONS {
            self.corrections.pop_front();
        }
        self.corrections.push_back(correction);
    }

    /// Kept corrections, including the one still in progress, oldest first.
    pub fn corrections(&self) -> impl Iterator<Item = &PreviewCorrection> {
        self.corrections
            .iter()
            .chain(self.open_correction.iter().filter(|c| c.kept()))
    }

    /// Episodes too small to keep, as a count and their total distance.
    #[cfg(test)]
    pub fn small_corrections(&self) -> (u32, f32) {
        self.small_corrections
    }

    /// Forgets every correction so far, kept or not.
    #[cfg(test)]
    pub fn clear_corrections(&mut self) {
        self.corrections.clear();
        self.open_correction = None;
        self.small_corrections = (0, 0.0);
    }

    /// Boxes where the local avatar was drawn when each recent correction
    /// started, with a line to where it was pulled, fading over two seconds.
    pub fn draw_corrections(&self, now: Instant, gizmos: &mut Gizmos) {
        if !self.show_corrections {
            return;
        }
        for c in self.corrections() {
            let age = now.saturating_duration_since(c.ended);
            if age > CORRECTION_SHOWN {
                continue;
            }
            let color = c.kind.color(1.0 - 0.7 * age.as_secs_f32() / CORRECTION_SHOWN.as_secs_f32());
            let lift = Vec3::Y * 0.05;
            gizmos.cube(
                Transform::from_translation(c.from + Vec3::Y * 0.9).with_scale(Vec3::new(0.7, 1.8, 0.7)),
                color,
            );
            gizmos.line(c.from + lift, c.to + lift, color);
            gizmos.sphere(Isometry3d::from_translation(c.to + lift), 0.12, color);
        }
    }

    fn correction_json(c: &PreviewCorrection, end: Instant) -> Value {
        json!({
            "kind": c.kind.name(),
            "age_ms": end.saturating_duration_since(c.started).as_secs_f64() * 1000.,
            "duration_ms": c.ended.saturating_duration_since(c.started).as_secs_f64() * 1000.,
            "frames": c.frames,
            "distance": c.distance,
            "from": [c.from.x, c.from.y, c.from.z],
            "to": [c.to.x, c.to.y, c.to.z],
            "local_next_tick": c.tick,
            "pending_application": c.pending,
            "rtt_ms": c.rtt.map(|v| v.as_secs_f64() * 1000.),
        })
    }

    /// Rates over up to one second; the first sample has no rate.
    fn rates(&self) -> Option<(f64, f64, f64, f64)> {
        let last = self.frames.back()?;
        let first = self
            .frames
            .iter()
            .rev()
            .take_while(|frame| last.at.duration_since(frame.at) <= Duration::from_secs(1))
            .last()?;
        let seconds = last.at.duration_since(first.at).as_secs_f64();
        if seconds <= 0.0 {
            return None;
        }
        let rate = |a: u64, b: u64| a.saturating_sub(b) as f64 / seconds;
        Some((
            rate(last.stats.packets_sent, first.stats.packets_sent),
            rate(last.stats.packets_received, first.stats.packets_received),
            rate(last.stats.bytes_sent, first.stats.bytes_sent),
            rate(last.stats.bytes_received, first.stats.bytes_received),
        ))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, active: bool) {
        let Some(frame) = self.frames.back() else {
            ui.label("Host or join a network game to collect diagnostics.");
            return;
        };
        let d = &frame.diagnostics;
        let client = frame.role == "Client";
        ui.label(format!(
            "{} · world {} · {}",
            frame.role, frame.world, frame.status
        ));
        if !active {
            ui.label("Session ended. Showing the last recorded values and history.");
        }
        ui.label(format!(
            "Latest sample {:.1} s ago · {} samples · up to 10 s / 600 frames",
            frame.at.elapsed().as_secs_f64(),
            self.frames.len()
        ));
        if ui.button("Copy diagnostics and history").clicked() {
            let report = json!({
                "latest": self.snapshot(),
                "history": self.frames.iter().map(|f| json!({
                    "age_ms": frame.at.duration_since(f.at).as_secs_f64() * 1000.,
                    "rtt_ms": f.stats.rtt.map(|v| v.as_secs_f64() * 1000.),
                    "ready_turns": f.stats.turns_buffered,
                    "ticks_behind_last_host": f.stats.ticks_behind_host,
                    "pending_application": f.pending,
                    "starved_updates": f.diagnostics.starved_updates,
                    "extra_catch_up_ticks": f.diagnostics.catch_up_ticks,
                    "frame_ms": f.frame_ms, "session_update_ms": f.update_ms, "slowest_tick_ms": f.tick_ms,
                    "packets_sent": f.stats.packets_sent, "packets_received": f.stats.packets_received,
                    "bytes_sent": f.stats.bytes_sent, "bytes_received": f.stats.bytes_received
                })).collect::<Vec<_>>(),
                "preview_corrections": self.corrections().map(|c| Self::correction_json(c, frame.at)).collect::<Vec<_>>()
            });
            ui.ctx()
                .copy_text(serde_json::to_string_pretty(&report).unwrap());
        }
        egui::Grid::new("network_summary")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                row(ui, "Local next tick", optional(frame.tick));
                row(ui, "Host next tick (last observed)", optional(d.host_tick));
                if client {
                    row(
                        ui,
                        "Behind last observed host",
                        ticks(frame.stats.ticks_behind_host),
                    );
                    row(
                        ui,
                        "Consecutive turns ready",
                        ticks(frame.stats.turns_buffered),
                    );
                    row(ui, "Input scheduling margin", ticks(d.input_delay));
                    row(
                        ui,
                        "Jitter target (catch-up threshold)",
                        ticks(d.jitter_buffer),
                    );
                    row(
                        ui,
                        "RTT (smoothed / latest sample)",
                        format!("{} / {}", ms(frame.stats.rtt), ms(d.rtt_sample)),
                    );
                    row(ui, "Last host packet age", ms(d.last_packet_age));
                    row(ui, "Last receive interval", ms(d.last_receive_interval));
                    row(
                        ui,
                        "Unacknowledged / pending application",
                        format!("{} / {} inputs", d.unacked_inputs, frame.pending),
                    );
                    row(
                        ui,
                        "Queued / applied locally",
                        format!("{} / {} commands", d.inputs_queued, d.inputs_applied),
                    );
                    row(
                        ui,
                        "Last receipt ack / requested tick",
                        format!(
                            "{} / {}",
                            optional(d.last_input_ack),
                            optional(d.last_requested_tick)
                        ),
                    );
                    row(
                        ui,
                        "Host assigned tick",
                        "Unknown on client (not in protocol)",
                    );
                    row(
                        ui,
                        "Missing-turn waits",
                        format!(
                            "{} updates · {} observed",
                            d.starved_updates,
                            ms(Some(d.starved_time))
                        ),
                    );
                    row(ui, "Extra catch-up ticks", d.catch_up_ticks.to_string());
                } else {
                    row(
                        ui,
                        "Host / remote input margin",
                        format!("{} / {}", ticks(0), ticks(d.remote_input_delay)),
                    );
                    row(ui, "Local pending commands", frame.pending.to_string());
                    row(
                        ui,
                        "Late remote inputs (session total)",
                        frame.stats.late_inputs.to_string(),
                    );
                }
                row(
                    ui,
                    "Ticks run in last update",
                    d.steps_last_update.to_string(),
                );
                let rates = self.rates();
                row(
                    ui,
                    "Send / receive packets per second",
                    rates.map_or("Unknown".into(), |(tx, rx, _, _)| {
                        format!("{tx:.1} / {rx:.1}")
                    }),
                );
                row(
                    ui,
                    "Send / receive KiB per second",
                    rates.map_or("Unknown".into(), |(_, _, tx, rx)| {
                        format!("{:.2} / {:.2}", tx / 1024., rx / 1024.)
                    }),
                );
                row(
                    ui,
                    "Packets sent / received (session)",
                    format!(
                        "{} / {}",
                        frame.stats.packets_sent, frame.stats.packets_received
                    ),
                );
            });
        ui.small("Receipt acknowledgements do not report simulation application. Tick lag uses the last host packet, not a synchronized host clock. Packet loss is not measured.");
        ui.separator();
        if client {
            self.graph(ui, "RTT", "ms", |f| {
                f.stats.rtt.map(|d| d.as_secs_f64() * 1000.)
            });
            self.graph(ui, "Ready turns", "ticks", |f| {
                Some(f.stats.turns_buffered as f64)
            });
            self.graph(ui, "Behind last observed host", "ticks", |f| {
                f.diagnostics
                    .host_tick
                    .map(|_| f.stats.ticks_behind_host as f64)
            });
        }
        self.graph(ui, "Frame wall time", "ms", |f| f.frame_ms);
        self.graph(ui, "Session update", "ms", |f| Some(f.update_ms));
        self.graph(ui, "Slowest simulation tick in frame", "ms", |f| f.tick_ms);

        if client {
            ui.collapsing("Movement preview", |ui| {
                if let Some(preview) = &self.preview {
                    ui.label(format!(
                        "Horizontal offset from confirmed position: {:.3} units",
                        preview.offset
                    ));
                    ui.label(format!(
                        "Explicit reconciliation in last preview update: {:.3} units",
                        preview.correction
                    ));
                    ui.label(format!(
                        "{} clamped frames · {} distance-reset snaps · sample {:.1} s ago",
                        self.preview_clamps,
                        self.preview_snaps,
                        preview.at.elapsed().as_secs_f64()
                    ));
                } else {
                    ui.label("Unknown: no local avatar preview sample yet.");
                }
                ui.checkbox(&mut self.show_corrections, "Show corrections in the world (2 s)");
                ui.small("Box: where the avatar was drawn when the correction started. Line: where it was pulled to. Yellow ease, orange clamp, magenta snap.");
                let kept: Vec<_> = self.corrections().collect();
                ui.label(format!("{} corrections of at least {MIN_CORRECTION} units kept", kept.len()));
                for c in kept.iter().rev().take(8) {
                    ui.monospace(format!(
                        "{:>5} {:.2} units over {:.0} ms · {:.1} s ago",
                        c.kind.name(),
                        c.distance,
                        c.ended.saturating_duration_since(c.started).as_secs_f64() * 1000.,
                        c.started.elapsed().as_secs_f64()
                    ));
                }
            });
        } else {
            ui.collapsing("Host peers", |ui| {
                if d.peers.is_empty() {
                    ui.label("No remote peers.");
                }
                for peer in &d.peers {
                    ui.push_id(peer.player.0, |ui| {
                        ui.label(format!(
                            "Player {} · {}",
                            peer.player.0,
                            if peer.loading { "loading" } else { "playing" }
                        ));
                        egui::Grid::new("peer").num_columns(2).show(ui, |ui| {
                            row(ui, "Last packet age", ms(Some(peer.last_packet_age)));
                            row(
                                ui,
                                "Next needed turn (receipt ack)",
                                peer.turn_ack.to_string(),
                            );
                            row(
                                ui,
                                "Unacknowledged closed turns",
                                frame.tick.map_or("Unknown".into(), |tick| {
                                    tick.saturating_sub(peer.turn_ack).to_string()
                                }),
                            );
                            row(
                                ui,
                                "Last accepted input sequence",
                                peer.input_ack.to_string(),
                            );
                            row(
                                ui,
                                "Last requested → assigned tick",
                                format!(
                                    "{} → {}",
                                    optional(peer.last_requested_tick),
                                    optional(peer.last_assigned_tick)
                                ),
                            );
                            row(
                                ui,
                                "Late inputs / total ticks shifted",
                                format!("{} / {}", peer.late_inputs, peer.shifted_ticks),
                            );
                        });
                    });
                }
            });
        }
        ui.small("Per-shot animation, authoritative fire, and hit-feedback timelines are not instrumented in this first window.");
    }

    fn graph(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        unit: &str,
        value: impl Fn(&NetworkFrame) -> Option<f64>,
    ) {
        let values: Vec<_> = self
            .frames
            .iter()
            .filter_map(|frame| value(frame).map(|v| (frame.at, v)))
            .collect();
        if values.is_empty() {
            ui.label(format!("{label}: unknown"));
            return;
        }
        let mut sorted: Vec<_> = values.iter().map(|(_, v)| *v).collect();
        sorted.sort_by(f64::total_cmp);
        let p95 = sorted[((sorted.len() - 1) as f64 * 0.95).round() as usize];
        let latest = values.last().unwrap().1;
        ui.label(format!("{label}: {latest:.1} {unit} · p95 {p95:.1} {unit}"));
        let ceiling = sorted.last().copied().unwrap_or(1.).max(1.);
        let (response, painter) = ui.allocate_painter(
            egui::vec2(ui.available_width().max(120.), 48.),
            egui::Sense::hover(),
        );
        let rect = response.rect;
        let end = self.frames.back().unwrap().at;
        let color = ui.visuals().selection.bg_fill;
        painter.rect_filled(rect, 2., ui.visuals().extreme_bg_color);
        // Binning by screen column preserves peaks at high frame rates and
        // avoids drawing thousands of overlapping marks in a narrow window.
        let width = rect.width().ceil().max(1.) as usize;
        let mut peaks = vec![None::<f64>; width];
        for (at, value) in values {
            let x =
                (1. - end.duration_since(at).as_secs_f64() / HISTORY.as_secs_f64()).clamp(0., 1.);
            let index = ((x * width as f64) as usize).min(width - 1);
            peaks[index] = Some(peaks[index].map_or(value, |peak| peak.max(value)));
        }
        for (index, value) in peaks.into_iter().enumerate() {
            if let Some(value) = value {
                let x = rect.left() + index as f32;
                let y = rect.bottom() - rect.height() * (value / ceiling) as f32;
                painter.line_segment(
                    [egui::pos2(x, rect.bottom()), egui::pos2(x, y)],
                    egui::Stroke::new(1., color),
                );
            }
        }
        ui.horizontal(|ui| {
            ui.small("−10 s");
            ui.small(format!("scale 0–{ceiling:.1} {unit} · latest →"));
        });
    }

    pub fn snapshot(&self) -> Value {
        let Some(frame) = self.frames.back() else {
            return Value::Null;
        };
        let d = &frame.diagnostics;
        let connection = json!({
            "role": frame.role, "status": frame.status, "world": frame.world, "next_tick": frame.tick,
            "sample_age_ms": frame.at.elapsed().as_secs_f64() * 1000., "samples": self.frames.len(),
            "host_next_tick": d.host_tick, "input_delay_ticks": d.input_delay,
            "remote_input_delay_ticks": d.remote_input_delay, "jitter_target_ticks": d.jitter_buffer,
            "rtt_ms": frame.stats.rtt.map(|v| v.as_secs_f64() * 1000.),
            "last_packet_age_ms": d.last_packet_age.map(|v| v.as_secs_f64() * 1000.),
            "ready_turns": frame.stats.turns_buffered, "ticks_behind_last_host": frame.stats.ticks_behind_host,
        });
        let inputs = json!({
            "unacknowledged_inputs": d.unacked_inputs, "pending_application": frame.pending,
            "inputs_queued": d.inputs_queued, "inputs_applied": d.inputs_applied,
            "last_receipt_ack": d.last_input_ack, "last_requested_tick": d.last_requested_tick,
            "starved_updates": d.starved_updates, "starved_ms": d.starved_time.as_secs_f64() * 1000.,
            "extra_catch_up_ticks": d.catch_up_ticks, "late_inputs": frame.stats.late_inputs,
        });
        let view = json!({
            "packets_sent": frame.stats.packets_sent, "packets_received": frame.stats.packets_received,
            "bytes_sent": frame.stats.bytes_sent, "bytes_received": frame.stats.bytes_received,
            "preview_offset": self.preview.as_ref().map(|p| p.offset),
            "preview_correction": self.preview.as_ref().map(|p| p.correction),
            "preview_clamped_frames": self.preview_clamps, "preview_distance_snaps": self.preview_snaps,
            "preview_corrections": self.corrections().count(),
        });
        let peers = d.peers.iter().map(|peer| json!({
                "player": peer.player.0, "loading": peer.loading, "turn_receipt_ack": peer.turn_ack,
                "input_ack": peer.input_ack, "last_packet_age_ms": peer.last_packet_age.as_secs_f64() * 1000.,
                "late_inputs": peer.late_inputs, "shifted_ticks": peer.shifted_ticks,
                "last_requested_tick": peer.last_requested_tick, "last_assigned_tick": peer.last_assigned_tick
            })).collect::<Vec<_>>();
        let mut fields = serde_json::Map::new();
        for group in [connection, inputs, view] {
            if let Value::Object(group) = group {
                fields.extend(group);
            }
        }
        fields.insert("peers".into(), Value::Array(peers));
        Value::Object(fields)
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(label);
    ui.monospace(value.into());
    ui.end_row();
}
fn optional(value: Option<u64>) -> String {
    value.map_or("Unknown".into(), |v| v.to_string())
}
fn ms(value: Option<Duration>) -> String {
    value.map_or("Unknown".into(), |v| {
        format!("{:.1} ms", v.as_secs_f64() * 1000.)
    })
}
fn ticks(value: u64) -> String {
    format!(
        "{value} ticks ({:.1} ms)",
        value as f64 * net::TICK.as_secs_f64() * 1000.
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(at: Instant, world: u32, packets: u64) -> NetworkFrame {
        NetworkFrame {
            at,
            role: "Client",
            status: "playing".into(),
            world,
            tick: Some(12),
            pending: 1,
            stats: net::NetStats {
                packets_sent: packets,
                ..Default::default()
            },
            diagnostics: net::NetDiagnostics::default(),
            update_ms: 1.,
            tick_ms: None,
            frame_ms: Some(16.7),
        }
    }

    #[test]
    fn network_history_is_bounded_and_world_changes_reset_rates() {
        let start = Instant::now();
        let mut network = NetworkDiagnostics::default();
        assert!(network.snapshot().is_null());
        network.record(frame(start, 1, 0));
        assert!(network.rates().is_none());
        for i in 1..=1000 {
            network.record(frame(start + Duration::from_millis(i), 1, i));
        }
        assert_eq!(network.frames.len(), MAX_SAMPLES);
        assert!((network.rates().unwrap().0 - 1000.).abs() < 0.01);
        assert!(
            network.snapshot()["rtt_ms"].is_null(),
            "unavailable RTT is not zero"
        );
        network.record(frame(start + Duration::from_secs(20), 1, 1001));
        assert_eq!(
            network.frames.len(),
            1,
            "old samples expire by time as well as count"
        );
        network.preview(start, update(1.0, 0.1, 0.2, true));
        network.record(frame(start + Duration::from_secs(21), 2, 1002));
        assert_eq!(network.frames.len(), 1);
        assert!(network.preview.is_none());
        assert!(network.rates().is_none());
    }

    #[test]
    fn window_draws_missing_measurements_and_retained_client_and_host_history() {
        let mut network = NetworkDiagnostics::default();
        let context = egui::Context::default();
        let draw = |network: &mut NetworkDiagnostics| {
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                network.show(ui, false);
            });
            assert!(!output.shapes.is_empty());
            output.drop_without_applying_deltas();
        };
        draw(&mut network);
        let now = Instant::now();
        network.record(frame(now, 1, 0));
        draw(&mut network);
        let mut host = frame(now + Duration::from_millis(20), 1, 1);
        host.role = "Host";
        host.diagnostics.peers.push(net::PeerDiagnostics {
            player: sim::PlayerId(1),
            loading: false,
            last_packet_age: Duration::ZERO,
            turn_ack: 10,
            input_ack: 1,
            late_inputs: 1,
            shifted_ticks: 2,
            last_requested_tick: Some(8),
            last_assigned_tick: Some(10),
        });
        network.record(host);
        draw(&mut network);
    }

    fn update(x: f32, eased: f32, clamped: f32, snapped: bool) -> PreviewUpdate {
        PreviewUpdate {
            shown: Vec3::new(x, 0.0, 0.0),
            confirmed: Vec3::ZERO,
            eased,
            clamped,
            snapped,
        }
    }

    #[test]
    fn preview_corrections_merge_consecutive_frames_and_drop_small_ones() {
        let start = Instant::now();
        let at = |i: u64| start + Duration::from_millis(16 * i);
        let mut network = NetworkDiagnostics::default();
        network.record(frame(start, 1, 0));
        network.preview(at(0), update(1.0, 0.0, 0.0, false));
        // Three leashed frames, then a change of kind to easing.
        network.preview(at(1), update(1.1, 0.0, 0.03, false));
        network.preview(at(2), update(1.2, 0.0, 0.03, false));
        network.preview(at(3), update(1.3, 0.0, 0.03, false));
        network.preview(at(4), update(1.2, 0.02, 0.0, false));
        network.preview(at(5), update(1.1, 0.0, 0.0, false));
        // A snap is its own episode.
        network.preview(at(6), update(4.0, 0.0, 0.0, true));
        network.preview(at(7), update(4.0, 0.0, 0.0, false));

        let kept: Vec<_> = network.corrections().collect();
        assert_eq!(kept.len(), 2, "the 0.02 unit ease is below the minimum");
        assert_eq!(kept[0].kind, CorrectionKind::Clamp);
        assert_eq!(kept[0].frames, 3);
        assert!((kept[0].distance - 0.09).abs() < 1e-5);
        assert_eq!(kept[0].from, Vec3::new(1.0, 0.0, 0.0), "the box marks where the avatar was before the pull");
        assert_eq!(kept[0].to, Vec3::new(1.3, 0.0, 0.0));
        assert_eq!(kept[0].tick, Some(12));
        assert_eq!(kept[1].kind, CorrectionKind::Snap);
        assert!((kept[1].distance - 2.9).abs() < 1e-5);
        assert_eq!(network.snapshot()["preview_corrections"], 2);

        network.show_corrections = true;
        network.begin();
        assert_eq!(network.corrections().count(), 0);
        assert!(network.show_corrections, "the overlay toggle survives a new session");
    }
}

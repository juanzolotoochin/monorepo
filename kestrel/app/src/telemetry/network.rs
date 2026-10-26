//! A read-only network view. Samples survive the Session layer's teardown.
use super::{egui, json, Duration, Instant, Value, VecDeque};
use std::collections::BTreeMap;
use bevy::prelude::{Color, Gizmos, Isometry3d, Transform, Vec3};

const HISTORY: Duration = Duration::from_secs(10);
const MAX_SAMPLES: usize = 600;
/// How long a correction stays drawn in the world after it ends.
const CORRECTION_SHOWN: Duration = Duration::from_secs(2);
/// Episodes that pull the avatar less than this (units) are not kept.
const MIN_CORRECTION: f32 = 0.05;
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
    preview_mispredictions: u64,
    preview_snaps: u64,
    /// Finished correction episodes this session, oldest first.
    corrections: VecDeque<PreviewCorrection>,
    /// The episode the latest preview frames belong to.
    open_correction: Option<PreviewCorrection>,
    /// Episodes dropped for being under `MIN_CORRECTION`: count and distance.
    small_corrections: (u32, f32),
    last_shown: Option<Vec3>,
    /// The local avatar's predicted events this session, by kind.
    events: BTreeMap<&'static str, EventStats>,
    /// Recent outcomes of every kind, oldest first.
    recent_events: VecDeque<EventOutcome>,
    /// Draw recent corrections in the world. Kept across sessions.
    pub show_corrections: bool,
}

/// The local avatar's predicted events of one kind this session.
#[derive(Clone, Copy, Default)]
struct EventStats {
    presented: u64,
    confirmed: u64,
    mispredicted: u64,
    /// The latest confirmed event's lead: how long before its confirmation
    /// it was presented, in seconds.
    last_lead: Option<f64>,
}

/// How a presented event turned out, in view space.
#[derive(Clone, Debug)]
pub struct EventOutcome {
    /// Its kind (`event_prediction::kind_of`): "laser", "bow".
    pub kind: &'static str,
    /// Where the presented event ended: a beam's end, or for an event
    /// without one, where the avatar was drawn.
    pub predicted: Vec3,
    /// Where the confirmed one ended; `None` if it never confirmed.
    pub confirmed: Option<Vec3>,
    /// Seconds from presenting it to its confirmation.
    pub lead: Option<f64>,
    /// It confirmed exactly as presented.
    pub exact: bool,
}

/// One frame of the client's predicted local avatar, in view space.
pub struct PreviewUpdate {
    /// Where the avatar is drawn this frame.
    pub shown: Vec3,
    /// Where the confirmed simulation has it.
    pub confirmed: Vec3,
    /// Where it is being pulled to: the prediction without the visual offset
    /// a misprediction left.
    pub target: Vec3,
    /// How far this frame's change of prediction moved the avatar: the
    /// difference between what was and is now predicted for the drawn tick.
    pub mispredicted: f32,
    /// The avatar was too far off and was put where it is predicted.
    pub snapped: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorrectionKind {
    Mispredict,
    Snap,
    /// A predicted event of `kind` ("laser", "bow") the confirmed one didn't
    /// match: it ended elsewhere, or never confirmed.
    EventMispredict { kind: &'static str },
}

impl CorrectionKind {
    /// An event misprediction is named by its kind.
    fn name(self) -> &'static str {
        match self {
            Self::Mispredict => "mispredict",
            Self::Snap => "snap",
            Self::EventMispredict { kind } => kind,
        }
    }

    fn color(self, alpha: f32) -> Color {
        match self {
            Self::Mispredict => Color::srgba(1.0, 0.55, 0.15, alpha),
            Self::Snap => Color::srgba(1.0, 0.2, 0.9, alpha),
            Self::EventMispredict { .. } => Color::srgba(1.0, 0.15, 0.15, alpha),
        }
    }
}

/// Consecutive frames whose prediction changed (or a snap): one correction
/// as the player feels it. An event misprediction is one event.
#[derive(Clone, Debug)]
pub struct PreviewCorrection {
    pub kind: CorrectionKind,
    pub started: Instant,
    pub ended: Instant,
    /// Where the avatar was drawn just before the episode; for an event,
    /// where the presented one ended (`EventOutcome::predicted`).
    pub from: Vec3,
    /// Where it was being pulled to on the episode's last frame; for an
    /// event, where the confirmed one ended (`from` if it never confirmed).
    pub to: Vec3,
    /// Total distance, summed over frames: each misprediction's starting
    /// error, or a snap's jump. For an event, between the two ends.
    pub distance: f32,
    pub frames: u32,
    pub tick: Option<u64>,
    pub pending: Option<u64>,
    pub rtt: Option<Duration>,
    /// An event that was presented but never confirmed.
    pub never_fired: bool,
}

impl PreviewCorrection {
    fn kept(&self) -> bool {
        matches!(self.kind, CorrectionKind::Snap | CorrectionKind::EventMispredict { .. }) || self.distance >= MIN_CORRECTION
    }

    fn is_event(&self) -> bool {
        matches!(self.kind, CorrectionKind::EventMispredict { .. })
    }
}

struct PreviewSample {
    at: Instant,
    offset: f32,
    mispredicted: f32,
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
            mispredicted: update.mispredicted,
        });
        self.preview_mispredictions += u64::from(update.mispredicted > 0.0);
        self.preview_snaps += u64::from(update.snapped);

        // Predictions are exact, so any change at all counts.
        let (kind, distance) = if update.snapped {
            let from = self.last_shown.unwrap_or(update.shown);
            (Some(CorrectionKind::Snap), flat(update.shown - from))
        } else if update.mispredicted > 0.0 {
            (Some(CorrectionKind::Mispredict), update.mispredicted)
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
                to: update.target,
                distance: 0.0,
                frames: 0,
                tick: latest.and_then(|f| f.tick),
                pending: latest.map(|f| f.pending),
                rtt: latest.and_then(|f| f.stats.rtt),
                never_fired: false,
            });
            open.ended = now;
            open.to = update.target;
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
        self.keep(correction);
    }

    fn keep(&mut self, correction: PreviewCorrection) {
        if self.corrections.len() >= MAX_CORRECTIONS {
            self.corrections.pop_front();
        }
        self.corrections.push_back(correction);
    }

    /// Prediction presented one of the local avatar's events of `kind`.
    pub fn event_presented(&mut self, kind: &'static str) {
        self.events.entry(kind).or_default().presented += 1;
    }

    /// How a presented event turned out. One that didn't confirm exactly as
    /// presented, or never confirmed, is an event misprediction.
    pub fn event_outcome(&mut self, now: Instant, outcome: EventOutcome) {
        let stats = self.events.entry(outcome.kind).or_default();
        if outcome.confirmed.is_some() {
            stats.confirmed += 1;
            stats.last_lead = outcome.lead;
        }
        if !outcome.exact {
            stats.mispredicted += 1;
        }
        if self.recent_events.len() >= MAX_CORRECTIONS {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(outcome.clone());
        if outcome.exact {
            return;
        }
        let latest = self.frames.back();
        let to = outcome.confirmed.unwrap_or(outcome.predicted);
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).length();
        let correction = PreviewCorrection {
            kind: CorrectionKind::EventMispredict { kind: outcome.kind },
            started: now,
            ended: now,
            from: outcome.predicted,
            to,
            distance: flat(to - outcome.predicted),
            frames: 1,
            tick: latest.and_then(|f| f.tick),
            pending: latest.map(|f| f.pending),
            rtt: latest.and_then(|f| f.stats.rtt),
            never_fired: outcome.confirmed.is_none(),
        };
        self.keep(correction);
    }

    /// Recent presented events of `kind` and how they turned out, oldest first.
    #[cfg(test)]
    pub fn event_outcomes<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a EventOutcome> + 'a {
        self.recent_events.iter().filter(move |o| o.kind == kind)
    }

    /// Events of `kind` presented by prediction so far.
    #[cfg(test)]
    pub fn events_presented(&self, kind: &str) -> u64 {
        self.events.get(kind).map_or(0, |stats| stats.presented)
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
        self.events.clear();
        self.recent_events.clear();
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
            if c.is_event() {
                // Both ends are at beam height, or where the avatar was drawn.
                gizmos.sphere(Isometry3d::from_translation(c.from), 0.2, color);
                gizmos.line(c.from, c.to, color);
                gizmos.sphere(Isometry3d::from_translation(c.to), 0.12, color);
                continue;
            }
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
        let mut value = json!({
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
        });
        if c.is_event() {
            // `from` is the presented event's end, `to` the confirmed one's.
            value["event_mispredict"] = json!(true);
            value["never_fired"] = json!(c.never_fired);
        }
        value
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
                    "input_margin_ticks": f.diagnostics.input_delay,
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
                    row(
                        ui,
                        "Input scheduling margin",
                        format!("{} · {}", ticks(d.input_delay), if d.input_margin_pinned { "pinned" } else { "adaptive" }),
                    );
                    row(
                        ui,
                        "Late inputs (detected locally)",
                        format!("{} · {} shifted", d.late_inputs, ticks(d.shifted_ticks)),
                    );
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
                        "Host / clients' starting input margin",
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
                        "Misprediction in the last update: {:.3} units",
                        preview.mispredicted
                    ));
                    ui.label(format!(
                        "{} mispredicted frames · {} snaps · sample {:.1} s ago",
                        self.preview_mispredictions,
                        self.preview_snaps,
                        preview.at.elapsed().as_secs_f64()
                    ));
                } else {
                    ui.label("Unknown: no local avatar prediction sample yet.");
                }
                if self.events.is_empty() {
                    ui.label("Predicted events: none presented yet.");
                }
                for (kind, stats) in &self.events {
                    ui.label(format!(
                        "Predicted {kind} events: {} drawn · {} confirmed · {} mispredicted · last drawn {} before confirmation",
                        stats.presented,
                        stats.confirmed,
                        stats.mispredicted,
                        stats.last_lead.map_or("Unknown".into(), |lead| format!("{:.0} ms", lead * 1000.))
                    ));
                }
                ui.checkbox(&mut self.show_corrections, "Show corrections in the world (2 s)");
                ui.small("Box: where the avatar was drawn when the correction started. Line: where it was pulled to. Orange mispredict, magenta snap. Red event (laser, bow): a ball where the presented event ended (a beam's end, else the avatar), a line to where the confirmed one ended.");
                let kept: Vec<_> = self.corrections().collect();
                let (small, small_distance) = self.small_corrections;
                ui.label(format!(
                    "{} corrections of at least {MIN_CORRECTION} units kept · {small} smaller ({small_distance:.3} units in all)",
                    kept.len()
                ));
                for c in kept.iter().rev().take(8) {
                    if c.is_event() {
                        let what = if c.never_fired { "never fired".to_string() } else { format!("ended {:.2} units away", c.distance) };
                        ui.monospace(format!("{:>10} {what} · {:.1} s ago", c.kind.name(), c.started.elapsed().as_secs_f64()));
                        continue;
                    }
                    ui.monospace(format!(
                        "{:>10} {:.2} units over {:.0} ms · {:.1} s ago",
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
            "host_next_tick": d.host_tick, "input_delay_ticks": d.input_delay, "input_margin_pinned": d.input_margin_pinned,
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
            "late_inputs_detected_locally": d.late_inputs, "late_input_shifted_ticks_detected_locally": d.shifted_ticks,
        });
        let laser = self.events.get("laser").copied().unwrap_or_default();
        let view = json!({
            "packets_sent": frame.stats.packets_sent, "packets_received": frame.stats.packets_received,
            "bytes_sent": frame.stats.bytes_sent, "bytes_received": frame.stats.bytes_received,
            "preview_offset": self.preview.as_ref().map(|p| p.offset),
            "preview_mispredicted": self.preview.as_ref().map(|p| p.mispredicted),
            "preview_mispredicted_frames": self.preview_mispredictions, "preview_snaps": self.preview_snaps,
            "preview_corrections": self.corrections().count(),
            "preview_corrections_under_threshold": self.small_corrections.0,
            "preview_corrections_under_threshold_distance": self.small_corrections.1,
            "laser_shots_drawn": laser.presented, "laser_shots_confirmed": laser.confirmed,
            "laser_mispredictions": laser.mispredicted,
            "laser_last_lead_ms": laser.last_lead.map(|lead| lead * 1000.),
            "predicted_events": self.events.iter().map(|(kind, stats)| (kind.to_string(), json!({
                "presented": stats.presented, "confirmed": stats.confirmed, "mispredicted": stats.mispredicted,
                "last_lead_ms": stats.last_lead.map(|lead| lead * 1000.),
            }))).collect::<serde_json::Map<_, _>>(),
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
        network.preview(start, update(1.0, 0.9, 0.1, true));
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

    /// A frame drawn at `x`, pulled toward `target` by `mispredicted`.
    fn update(x: f32, target: f32, mispredicted: f32, snapped: bool) -> PreviewUpdate {
        PreviewUpdate {
            shown: Vec3::new(x, 0.0, 0.0),
            confirmed: Vec3::ZERO,
            target: Vec3::new(target, 0.0, 0.0),
            mispredicted,
            snapped,
        }
    }

    #[test]
    fn preview_corrections_merge_consecutive_frames_and_drop_small_ones() {
        let start = Instant::now();
        let at = |i: u64| start + Duration::from_millis(16 * i);
        let mut network = NetworkDiagnostics::default();
        network.record(frame(start, 1, 0));
        network.preview(at(0), update(1.0, 1.0, 0.0, false));
        // Three mispredicted frames in a row are one correction.
        network.preview(at(1), update(1.1, 1.0, 0.03, false));
        network.preview(at(2), update(1.2, 1.1, 0.03, false));
        network.preview(at(3), update(1.3, 1.2, 0.03, false));
        network.preview(at(4), update(1.3, 1.3, 0.0, false));
        // One too small to keep, but counted.
        network.preview(at(5), update(1.4, 1.38, 0.02, false));
        network.preview(at(6), update(1.5, 1.5, 0.0, false));
        // A snap is its own episode.
        network.preview(at(7), update(4.0, 4.0, 0.0, true));
        network.preview(at(8), update(4.0, 4.0, 0.0, false));

        let kept: Vec<_> = network.corrections().collect();
        assert_eq!(kept.len(), 2, "the 0.02 unit misprediction is below the minimum");
        assert_eq!(kept[0].kind, CorrectionKind::Mispredict);
        assert_eq!(kept[0].frames, 3);
        assert!((kept[0].distance - 0.09).abs() < 1e-5);
        assert_eq!(kept[0].from, Vec3::new(1.0, 0.0, 0.0), "the box marks where the avatar was before the pull");
        assert_eq!(kept[0].to, Vec3::new(1.2, 0.0, 0.0), "the line ends where the prediction pulls it");
        assert_eq!(kept[0].tick, Some(12));
        assert_eq!(kept[1].kind, CorrectionKind::Snap);
        assert!((kept[1].distance - 2.5).abs() < 1e-5);
        let (small, small_distance) = network.small_corrections();
        assert_eq!(small, 1);
        assert!((small_distance - 0.02).abs() < 1e-6);
        assert_eq!(network.snapshot()["preview_corrections"], 2);
        assert_eq!(network.snapshot()["preview_corrections_under_threshold"], 1);

        network.show_corrections = true;
        network.begin();
        assert_eq!(network.corrections().count(), 0);
        assert!(network.show_corrections, "the overlay toggle survives a new session");
    }

    #[test]
    fn event_mispredictions_are_kept_per_kind_with_both_ends_and_reported() {
        let start = Instant::now();
        let mut network = NetworkDiagnostics::default();
        network.record(frame(start, 1, 0));
        let end = |x: f32| Vec3::new(x, 0.8, 2.0);
        let outcome = |kind, predicted: f32, confirmed: Option<f32>, exact: bool| EventOutcome {
            kind,
            predicted: end(predicted),
            confirmed: confirmed.map(end),
            lead: confirmed.map(|_| 0.2),
            exact,
        };
        for _ in 0..3 {
            network.event_presented("laser");
        }
        network.event_presented("bow");
        network.event_outcome(start, outcome("laser", 4.0, Some(4.0), true));
        // A shot the confirmed one ended elsewhere, however close.
        network.event_outcome(start, outcome("laser", 4.0, Some(4.01), false));
        // One the confirmed world never fired.
        network.event_outcome(start, outcome("laser", 6.0, None, false));
        // An arrow that never confirmed, marked where the avatar was drawn.
        network.event_outcome(start, outcome("bow", 1.0, None, false));

        let kept: Vec<_> = network.corrections().collect();
        assert_eq!(kept.len(), 3, "an exact shot is no correction; an event misprediction of any size is kept");
        assert_eq!(kept[0].kind, CorrectionKind::EventMispredict { kind: "laser" });
        assert_eq!((kept[0].from, kept[0].to), (end(4.0), end(4.01)));
        assert!((kept[0].distance - 0.01).abs() < 1e-4 && !kept[0].never_fired);
        assert_eq!((kept[1].from, kept[1].to, kept[1].distance), (end(6.0), end(6.0), 0.0));
        assert!(kept[1].never_fired);
        assert_eq!(kept[2].kind, CorrectionKind::EventMispredict { kind: "bow" });
        assert_eq!(network.small_corrections(), (0, 0.0));
        assert_eq!((network.event_outcomes("laser").count(), network.event_outcomes("bow").count()), (3, 1));
        assert_eq!((network.events_presented("laser"), network.events_presented("bow")), (3, 1));
        let snapshot = network.snapshot();
        assert_eq!(snapshot["laser_shots_drawn"], 3);
        assert_eq!(snapshot["laser_shots_confirmed"], 2);
        assert_eq!(snapshot["laser_mispredictions"], 2);
        assert!((snapshot["laser_last_lead_ms"].as_f64().unwrap() - 200.0).abs() < 1e-6);
        let bow = &snapshot["predicted_events"]["bow"];
        assert_eq!((bow["presented"].as_u64(), bow["confirmed"].as_u64(), bow["mispredicted"].as_u64()), (Some(1), Some(0), Some(1)));
        assert!(bow["last_lead_ms"].is_null());
        assert_eq!(snapshot["predicted_events"]["laser"]["mispredicted"], 2);
        let report = NetworkDiagnostics::correction_json(kept[1], start);
        assert_eq!(report["kind"], "laser");
        assert_eq!(report["never_fired"], true);
        assert_eq!(NetworkDiagnostics::correction_json(kept[2], start)["kind"], "bow");

        let context = egui::Context::default();
        let output = context.run_ui(egui::RawInput::default(), |ui| network.show(ui, true));
        output.drop_without_applying_deltas();
        network.clear_corrections();
        assert_eq!((network.corrections().count(), network.event_outcomes("laser").count()), (0, 0));
        assert!(network.snapshot()["predicted_events"].as_object().unwrap().is_empty());
    }
}

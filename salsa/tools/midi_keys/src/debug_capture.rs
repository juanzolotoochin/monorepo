//! Opt-in preview diagnostics. Never reads or writes a learner profile.
use crate::Session;
use keyboard::Event;
use serde_json::{json, Value};
use std::{
    hash::{Hash, Hasher},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
const FORMAT: &str = "midi-keys-exercise-debug";

pub(super) struct Capture {
    path: PathBuf,
    started: Instant,
    started_unix: u64,
    last_write: Instant,
    dirty: bool,
    events: Vec<Value>,
    result: Option<Value>,
    runtime: Value,
}
impl Capture {
    pub(super) fn begin(&mut self, now: Instant) {
        self.started = now;
        self.started_unix = crate::now_seconds();
        self.events.clear();
        self.result = None;
        self.dirty = true;
    }
    pub(super) fn reset_result(&mut self) {
        self.result = None;
        self.dirty = true;
    }
    fn record(&mut self, now: Instant, mut event: Value) {
        event["offset_us"] = json!(now.saturating_duration_since(self.started).as_micros());
        event["index"] = json!(self.events.len());
        self.events.push(event);
        self.dirty = true;
    }
    fn offset(&self, at: Option<Instant>) -> Option<i64> {
        at.map(|at| {
            if at >= self.started {
                at.duration_since(self.started).as_micros() as i64
            } else {
                -(self.started.duration_since(at).as_micros() as i64)
            }
        })
    }
}
fn event_value(event: Event) -> Value {
    match event {
        Event::Note {
            channel,
            note,
            velocity,
        } => json!({"type":"note","channel":channel,"note":note,"velocity":velocity}),
        Event::Sustain { channel, down } => json!({"type":"sustain","channel":channel,"down":down}),
        Event::ClearChannel(channel) => json!({"type":"clear_channel","channel":channel}),
        Event::ResetControllers(channel) => json!({"type":"reset_controllers","channel":channel}),
        Event::Reset => json!({"type":"reset"}),
        Event::CountIn(number) => json!({"type":"spoken_count_in","number":number}),
        Event::MetronomeClick => json!({"type":"metronome_click"}),
        Event::DrumBeat { beat } => json!({"type":"drum_beat","beat":beat}),
    }
}
impl Session {
    pub fn enable_debug(&mut self, path: PathBuf) -> Result<(), String> {
        if !self.previewing() {
            return Err("Exercise debug capture is only available in preview mode".into());
        }
        if path.exists() {
            let previous: Value = serde_json::from_slice(
                &std::fs::read(&path).map_err(|e| e.to_string())?,
            )
            .map_err(|_| {
                "Refusing to overwrite a file that is not an exercise debug capture".to_string()
            })?;
            if previous["format"] != FORMAT {
                return Err(
                    "Refusing to overwrite a file that is not an exercise debug capture".into(),
                );
            }
        }
        let now = Instant::now();
        self.debug = Some(Capture {
            path,
            started: now,
            started_unix: crate::now_seconds(),
            last_write: now,
            dirty: true,
            events: vec![],
            result: None,
            runtime: Value::Null,
        });
        self.flush_debug(true)
    }
    /// Raw ALSA sequencer input is copied before native storage is reused.
    /// Captures every event delivered to the app, even if MIDI decoding/grading ignores it.
    pub fn debug_midi(
        &mut self,
        now: Instant,
        client: i32,
        port: i32,
        name: &str,
        event_type: i32,
        data: &[u8],
        message: Option<[u8; 3]>,
        selected: bool,
    ) {
        if let Some(capture) = &mut self.debug {
            capture.record(now,json!({"kind":"midi_input","source":{"client":client,"port":port,"name":name},"alsa_type":event_type,"alsa_data":data,"midi_message":message,"selected_source":selected,"phase":self.phase,"exploring":self.exploring,"visible":self.visible}));
        }
    }
    pub fn debug_event(&mut self, kind: &str, event: Event, now: Instant) {
        if let Some(capture) = &mut self.debug {
            capture.record(now,json!({"kind":kind,"event":event_value(event),"phase":self.phase,"exploring":self.exploring,"visible":self.visible}));
        }
    }
    pub fn debug_runtime(
        &mut self,
        columns: i32,
        rows: i32,
        silent: bool,
        muted: bool,
        volume: f32,
    ) {
        if let Some(capture) = &mut self.debug {
            let runtime = json!({"columns":columns,"rows":rows,"silent":silent,"muted":muted,"volume":volume});
            if runtime != capture.runtime {
                capture.runtime = runtime;
                capture.dirty = true;
            }
        }
    }
    pub fn debug_control(&mut self, key: i32, now: Instant) {
        if let Some(capture) = &mut self.debug {
            capture.record(now,json!({"kind":"terminal_key","key_code":key,"phase":self.phase,"exploring":self.exploring}));
        }
    }
    pub(super) fn debug_result(
        &mut self,
        correct: bool,
        pitch: bool,
        timing: bool,
        holds: bool,
        give_up: bool,
    ) {
        if let Some(capture) = &mut self.debug {
            capture.result = Some(
                json!({"performance_score":self.performance_score,"correct":correct,"pitch_correct":pitch,"timing_correct":timing,"duration_correct":holds,"gave_up":give_up,"assisted":self.assisted}),
            );
            capture.dirty = true;
        }
    }
    pub fn flush_debug(&mut self, force: bool) -> Result<(), String> {
        let Some(capture) = &self.debug else {
            return Ok(());
        };
        if !capture.dirty || (!force && capture.last_write.elapsed() < Duration::from_millis(250)) {
            return Ok(());
        }
        // Returning to the browser must leave the last exercise available to share.
        if self.exercise.is_none() && capture.path.exists() {
            return Ok(());
        }
        let exercise=self.exercise.as_ref().map(|e|json!({
            "skill_id":e.skill_id,"title":e.title,"prompt":e.prompt,"explanation":e.explanation,"variant":e.variant,
            "task":self.graph.iter().find(|s|s.id==e.skill_id).map(|s|format!("{:?}",s.task)),
            "answer_rules":e.answer,"answer_policy":e.answer_policy,"bpm":e.bpm,
            "count_in":e.count_in,"requires_explicit_start":e.requires_explicit_start(),"answer_after_ms":e.answer_after_ms,"metronome":e.metronome,"pulse_backing":e.pulse_backing,
            "expected_example":e.expected_evidence(),"spelling":e.spelling,
            "prompt_score":e.prompt_score,"expected_score":e.expected_score,"reading":e.reading,
            "playback":e.playback.iter().map(|(ms,event)|json!({"offset_ms":ms,"event":event_value(*event)})).collect::<Vec<_>>()
        }));
        let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
        for source in [
            include_str!("exercise.rs"),
            include_str!("tempo.rs"),
            include_str!("rhythm.rs"),
            include_str!("evidence.rs"),
            include_str!("performance.rs"),
            include_str!("practice.rs"),
            include_str!("reading.rs"),
            include_str!("trainer.rs"),
            include_str!("score_support.rs"),
        ] {
            source.hash(&mut fingerprint);
        }
        let snapshot = json!({
            "format":FORMAT,"version":1,"policy_fingerprint":format!("{:016x}",fingerprint.finish()),
            "started_unix_seconds":capture.started_unix,"captured_duration_us":capture.started.elapsed().as_micros(),
            "capture_notes":"Timestamps are monotonic receipt times relative to capture start. midi_input contains every ALSA event delivered to the app; alsa_data is the sequencer data union for fixed events or the complete payload for variable events, not a wire-level MIDI recording. decoded_input records the app interpretation separately. Browser previews never save learner progress.",
            "exercise":exercise,"phase":self.phase,"exploring":self.exploring,"visible":self.visible,
            "result":capture.result,"feedback":self.feedback,"played_notes":self.played,"grading_evidence":self.evidence,
            "timing_us":{"answer_started":capture.offset(self.answer_started_at),"input_started":capture.offset(self.input_started),"onsets":self.onsets.iter().map(|t|capture.offset(Some(*t))).collect::<Vec<_>>(),"listening_until":capture.offset(self.listening_until),"finish_at":capture.offset(self.finish_at)},
            "scores":{"expected_feedback":self.feedback_score,"played":self.played_score,"page":self.score_page},
            "reading_result":self.reading_result,"rhythm_report":self.rhythm_report,"rhythm_score":self.exercise.as_ref().and_then(|e|e.rhythm_score()),"events":capture.events,"runtime":capture.runtime,
            "terminal":{"term":std::env::var("TERM").ok(),"term_program":std::env::var("TERM_PROGRAM").ok(),"tmux":std::env::var_os("TMUX").is_some(),"score_images":self.reading_supported},
        });
        let path = capture.path.clone();
        let write = || -> Result<(), String> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            serde_json::to_writer_pretty(&mut temp, &snapshot).map_err(|e| e.to_string())?;
            temp.write_all(b"\n").map_err(|e| e.to_string())?;
            temp.as_file().sync_all().map_err(|e| e.to_string())?;
            temp.persist(&path).map_err(|e| e.to_string())?;
            Ok(())
        };
        write()
            .map_err(|e| format!("Cannot save exercise debug capture {}: {e}", path.display()))?;
        let capture = self.debug.as_mut().unwrap();
        capture.dirty = false;
        capture.last_write = Instant::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(path: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }
    #[test]
    fn captures_ignored_input_complete_grading_and_final_key_release_without_profile_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.json");
        let mut session = Session::preview("reading.right.0").unwrap();
        session.enable_debug(path.clone()).unwrap();
        let now = Instant::now();
        // Unknown input / SysEx is still preserved verbatim while waiting.
        session.debug_midi(
            now,
            32,
            0,
            "Test keyboard",
            130,
            &[0xf0, 1, 2, 0xf7],
            None,
            true,
        );
        session.start(now);
        let note = session.exercise.as_ref().unwrap().expected_evidence()[0];
        session.debug_control(13, now);
        session.debug_midi(
            now,
            32,
            0,
            "Test keyboard",
            6,
            &[0, 0, 0],
            Some([0x90, note as u8, 93]),
            true,
        );
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 93,
            },
            now,
        );
        session.tick(now).unwrap();
        assert_eq!(read(&path)["result"]["correct"], true);
        // Grading already completed, but the final release must still be in the capture.
        let released = now + Duration::from_millis(900);
        session.debug_midi(
            released,
            32,
            0,
            "Test keyboard",
            7,
            &[1, 2, 3],
            Some([0x80, note as u8, 0]),
            true,
        );
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 0,
            },
            released,
        );
        session.flush_debug(true).unwrap();
        let data = read(&path);
        assert_eq!(data["exercise"]["skill_id"], "reading.right.0");
        assert!(data["exercise"]["answer_rules"]["Performance"].is_object());
        assert!(data["exercise"]["expected_score"].is_object());
        let events = data["events"].as_array().unwrap();
        assert_eq!(events[0]["alsa_data"], json!([240, 1, 2, 247]));
        assert_eq!(events[0]["phase"], "Waiting");
        let release = events.iter().find(|e| e["midi_message"][0] == 128).unwrap();
        assert_eq!(release["phase"], "Feedback");
        assert!(release["offset_us"].as_u64().unwrap() >= 900_000);
        assert!(session.store.is_none());
        assert_eq!(session.profile.completed, 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let complete = std::fs::read(&path).unwrap();
        session.show_browser();
        session.flush_debug(true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), complete);
    }
    #[test]
    fn incomplete_capture_updates_and_new_variant_replaces_only_debug_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.json");
        let mut session = Session::preview("melody.0.3").unwrap();
        session.enable_debug(path.clone()).unwrap();
        let now = Instant::now();
        session.start(now);
        session.input(
            Event::Sustain {
                channel: 1,
                down: true,
            },
            now,
        );
        session.flush_debug(true).unwrap();
        let first = read(&path);
        assert!(first["result"].is_null());
        assert_eq!(first["events"][0]["event"]["type"], "sustain");
        session.prepare();
        session.flush_debug(true).unwrap();
        let second = read(&path);
        assert_ne!(first["exercise"]["variant"], second["exercise"]["variant"]);
        assert_eq!(second["events"], json!([]));
        assert!(second["result"].is_null());
    }
    #[test]
    fn refuses_unrelated_files_and_reports_write_failures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        std::fs::write(&path, b"{\"completed\":42}").unwrap();
        let mut session = Session::preview("reading.right.0").unwrap();
        assert!(session.enable_debug(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"completed\":42}");
        let debug = dir.path().join("debug.json");
        session.enable_debug(debug.clone()).unwrap();
        std::fs::remove_file(&debug).unwrap();
        std::fs::create_dir(&debug).unwrap();
        session.debug_control(13, Instant::now());
        assert!(session
            .flush_debug(true)
            .unwrap_err()
            .contains("Cannot save exercise debug capture"));
    }
}

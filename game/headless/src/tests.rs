use content::Content;

use crate::{first_difference, format_hashes, golden_hashes, record, replay, GOLDEN};

fn expected(name: &str) -> &'static str {
    match name {
        "small" => include_str!("../../testdata/small.hashes"),
        "large" => include_str!("../../testdata/large.hashes"),
        _ => panic!("no golden file for {name}"),
    }
}

/// Every golden scenario, run through a save and its input log, matches its
/// checked-in hash file. Regenerate with `bazel run //game/headless -- golden`.
#[test]
fn golden_hashes_match() {
    let content = Content::builtin();
    for golden in GOLDEN {
        let actual = golden_hashes(golden, crate::default_threads(), &content);
        if let Some(difference) = first_difference(expected(golden.name), &actual) {
            panic!(
                "{}: hashes changed at {difference}. If this is intended, run `bazel run //game/headless -- golden`.",
                golden.name
            );
        }
    }
}

#[test]
fn thread_count_through_replay() {
    let content = Content::builtin();
    let recording = record(&(GOLDEN[0].scenario)(), 120, 1, &content);
    let one = format_hashes(&replay(&recording.save, &recording.inputs, 1, &content).unwrap());
    let many = format_hashes(&replay(&recording.save, &recording.inputs, 16, &content).unwrap());
    assert_eq!(first_difference(&one, &many), None);
}

#[test]
fn input_logs_round_trip() {
    let content = Content::builtin();
    let recording = record(&(GOLDEN[0].scenario)(), 30, 2, &content);
    let bytes = recording.inputs.to_bytes();
    assert_eq!(save::InputLog::from_bytes(&bytes).unwrap(), recording.inputs);
    assert!(!recording.inputs.turn(0).is_empty());
    assert!(recording.inputs.turn(1).is_empty());
}

#[test]
fn mismatched_save_and_log_are_refused() {
    let content = Content::builtin();
    let recording = record(&(GOLDEN[0].scenario)(), 5, 1, &content);
    let mut inputs = recording.inputs.clone();
    inputs.start_tick = 3;
    assert!(replay(&recording.save, &inputs, 1, &content).is_err());
}

#[test]
fn differences_name_the_line() {
    assert_eq!(first_difference("0 a\n1 b\n", "0 a\r\n1 b\r\n"), None);
    assert_eq!(first_difference("0 a\n1 b\n", "0 a\n1 c\n").unwrap(), r#"line 2: expected "1 b", got "1 c""#);
    assert!(first_difference("0 a\n", "0 a\n1 b\n").unwrap().contains("<end>"));
}

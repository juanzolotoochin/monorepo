use content::Content;
use sim::scenario::Scenario;
use sim::Simulation;

use crate::{dump_ron, read, read_header, write, LoadError, FORMAT_VERSION};

const TOTAL: u64 = 240;

/// Saving at tick `at`, loading and continuing gives the same hashes as never stopping.
fn save_and_continue(at: u64) {
    let content = Content::builtin();
    let scenario = Scenario::small();
    let uninterrupted = scenario.run(TOTAL, 4);

    let mut first = scenario.start(4);
    scenario.run_from(&mut first, at);
    let bytes = write(first.world(), &content);

    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 4);
    resumed.world().check_invariants();
    let rest = scenario.run_from(&mut resumed, TOTAL - at);
    for (expected, actual) in uninterrupted[at as usize..].iter().zip(&rest) {
        if let Some(difference) = expected.first_difference(actual) {
            panic!("save at {at}: first difference at {difference}");
        }
    }
}

#[test]
fn save_at_tick_zero_and_continue() {
    save_and_continue(0);
}

#[test]
fn save_mid_game_and_continue() {
    save_and_continue(137);
}

#[test]
fn saving_a_loaded_world_gives_the_same_bytes() {
    let content = Content::builtin();
    let scenario = Scenario::small();
    let mut sim = scenario.start(2);
    scenario.run_from(&mut sim, 60);
    let bytes = write(sim.world(), &content);
    let again = write(&read(&bytes, &content).unwrap(), &content);
    assert_eq!(bytes, again);

    let header = read_header(&bytes).unwrap();
    assert_eq!(header.tick, 60);
    assert_eq!(header.rules_hash, content.rules_hash);
}

#[test]
fn other_format_versions_are_refused() {
    let content = Content::builtin();
    let mut bytes = write(Scenario::small().start(1).world(), &content);
    bytes[4..8].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    assert_eq!(
        read(&bytes, &content).unwrap_err(),
        LoadError::FormatVersion { found: FORMAT_VERSION + 1, expected: FORMAT_VERSION }
    );
    assert_eq!(read(b"nonsense", &content).unwrap_err(), LoadError::NotASave);
}

/// Adding a definition shifts indices; loading remaps them by name.
#[test]
fn content_indices_are_remapped_by_name() {
    let builtin = Content::builtin();
    let scenario = Scenario::small();
    let mut sim = scenario.start(2);
    scenario.run_from(&mut sim, 30);
    let bytes = write(sim.world(), &builtin);

    let extra = "(sim: (radius: 0.25, max_hp: 5, speed: 3, spawn_weight: 1), view: (color: (1, 1, 1)))";
    let mut sources = content::builtin_sources();
    sources.push(("mover/aardvark.ron".into(), extra.into()));
    let changed = Content::from_sources(sources).unwrap();
    assert_eq!(changed.names.movers[0], "mover/aardvark");

    let loaded = read(&bytes, &changed).unwrap();
    loaded.check_invariants();
    let names_before: Vec<&str> = sim
        .world()
        .chunks()
        .iter()
        .flat_map(|c| c.movers.kind.iter().map(|&k| builtin.names.movers[k as usize].as_str()))
        .collect();
    let names_after: Vec<&str> = loaded
        .chunks()
        .iter()
        .flat_map(|c| c.movers.kind.iter().map(|&k| changed.names.movers[k as usize].as_str()))
        .collect();
    assert_eq!(names_before, names_after);

    // And the remapped world keeps running.
    let mut resumed = Simulation::from_world(loaded, 2);
    resumed.step(&[]);
    resumed.world().check_invariants();
}

#[test]
fn missing_content_is_named() {
    let builtin = Content::builtin();
    let bytes = write(Scenario::small().start(1).world(), &builtin);
    let only_small = Content::from_sources([(
        "mover/small.ron".to_string(),
        "(sim: (radius: 0.5, max_hp: 20, speed: 7.5, spawn_weight: 6), view: (color: (1, 0, 0)))".to_string(),
    )])
    .unwrap();
    assert_eq!(read(&bytes, &only_small).unwrap_err(), LoadError::MissingContent { name: "mover/large".into() });
}

#[test]
fn dumps_print_as_ron() {
    let content = Content::builtin();
    let scenario = Scenario::small();
    let mut sim = scenario.start(1);
    scenario.run_from(&mut sim, 5);
    let dump = dump_ron(&write(sim.world(), &content)).unwrap();
    assert!(dump.contains("// header"));
    assert!(dump.contains("// chunk/63"));
    assert!(dump.contains("mover/small"));
    // Positions print as plain numbers, not raw bits.
    let pos = dump.find("pos: FxVec3(").expect("a position");
    let x = dump[pos..].find("x: ").map(|i| pos + i + 3).unwrap();
    assert!(dump[x..x + 12].contains('.'), "{}", &dump[pos..pos + 80]);
}

/// Prints the save size for the 100k-mover world. Run with `--test_arg=--ignored`.
#[test]
#[ignore]
fn large_save_size() {
    let content = Content::builtin();
    let scenario = Scenario::large();
    let mut sim = scenario.start(8);
    scenario.run_from(&mut sim, 10);
    let bytes = write(sim.world(), &content);
    println!("{} movers: {} KiB", sim.world().mover_count(), bytes.len() / 1024);
}

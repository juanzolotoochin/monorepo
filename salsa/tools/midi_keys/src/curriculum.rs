//! Curriculum data and prerequisites, independent of terminal and MIDI I/O.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Both,
    Together,
}

#[derive(Clone, Debug)]
pub enum Task {
    Practice(crate::practice::Practice),
    BuildInterval {
        semitones: usize,
        direction: Direction,
    },
    HearInterval {
        semitones: usize,
        direction: Direction,
    },
    BuildChord {
        root: usize,
        quality: usize,
    },
    HearChord {
        root: usize,
        quality: usize,
    },
    Scale {
        root: usize,
        minor: bool,
        direction: Direction,
    },
    ContextInterval {
        root: usize,
        first: usize,
        second: usize,
    },
    Inversion {
        root: usize,
        quality: usize,
        inversion: usize,
        hearing: bool,
    },
    TimedScale {
        root: usize,
        minor: bool,
        direction: Direction,
        bpm: u32,
    },
}

#[derive(Clone, Debug)]
pub struct Requirement {
    pub skill: String,
    pub score: f32,
    pub attempts: u32,
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub stage: u8,
    pub id: String,
    pub title: String,
    pub task: Task,
    pub requires: Vec<Requirement>,
}

pub const INTERVALS: [&str; 12] = [
    "minor 2nd",
    "major 2nd",
    "minor 3rd",
    "major 3rd",
    "perfect 4th",
    "tritone",
    "perfect 5th",
    "minor 6th",
    "major 6th",
    "minor 7th",
    "major 7th",
    "octave",
];
pub const ROOTS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
pub const QUALITIES: [&str; 13] = [
    "major",
    "minor",
    "diminished",
    "augmented",
    "dominant seventh",
    "major seventh",
    "minor seventh",
    "half-diminished seventh",
    "diminished seventh",
    "minor-major seventh",
    "dominant ninth",
    "major ninth",
    "minor ninth",
];
pub const CHORDS: [&[usize]; 13] = [
    &[0, 4, 7],
    &[0, 3, 7],
    &[0, 3, 6],
    &[0, 4, 8],
    &[0, 4, 7, 10],
    &[0, 4, 7, 11],
    &[0, 3, 7, 10],
    &[0, 3, 6, 10],
    &[0, 3, 6, 9],
    &[0, 3, 7, 11],
    &[0, 4, 7, 10, 14],
    &[0, 4, 7, 11, 14],
    &[0, 3, 7, 10, 14],
];

fn requirement(id: &str) -> Requirement {
    Requirement {
        skill: id.into(),
        score: 7.0,
        attempts: 6,
    }
}
pub(crate) fn add(
    graph: &mut Vec<Skill>,
    id: String,
    title: String,
    task: Task,
    dependencies: &[String],
) {
    graph.push(Skill {
        stage: 0,
        id,
        title,
        task,
        requires: dependencies.iter().map(|id| requirement(id)).collect(),
    });
}

pub fn curriculum() -> Vec<Skill> {
    let mut graph = Vec::new();
    // Introduce small intervals before larger ones. Each direction and each
    // mode of response is independent; construction isn't proof of recognition.
    for interval in [2, 1, 4, 3, 7, 5, 12, 9, 8, 10, 11, 6] {
        let foundation = match interval {
            2 => vec![],
            1 | 3 | 4 | 5 | 7 | 12 => vec!["interval.build.2.up".into()],
            _ => vec!["interval.build.4.up".into(), "interval.build.1.up".into()],
        };
        for (suffix, direction) in [("up", Direction::Up), ("down", Direction::Down)] {
            let deps = if direction == Direction::Up {
                foundation.clone()
            } else {
                vec![format!("interval.build.{interval}.up")]
            };
            add(
                &mut graph,
                format!("interval.build.{interval}.{suffix}"),
                format!("Construct {} · {suffix}", INTERVALS[interval - 1]),
                Task::BuildInterval {
                    semitones: interval,
                    direction,
                },
                &deps,
            );
        }
        for (suffix, direction) in [
            ("up", Direction::Up),
            ("down", Direction::Down),
            ("together", Direction::Together),
        ] {
            // Directions are parallel listening skills. Requiring upward
            // recognition first unnecessarily postpones the other presentations.
            let deps = vec![format!("interval.build.{interval}.up")];
            add(
                &mut graph,
                format!("interval.hear.{interval}.{suffix}"),
                format!("Recognize {} · {suffix}", INTERVALS[interval - 1]),
                Task::HearInterval {
                    semitones: interval,
                    direction,
                },
                &deps,
            );
        }
    }
    for root in 0..12 {
        for minor in [false, true] {
            let mode = if minor { "natural-minor" } else { "major" };
            for (suffix, direction) in [
                ("up", Direction::Up),
                ("down", Direction::Down),
                ("both", Direction::Both),
            ] {
                let base = format!("scale.{root}.{mode}");
                let deps = match direction {
                    Direction::Down => vec![format!("{base}.up")],
                    Direction::Both => vec![format!("{base}.up"), format!("{base}.down")],
                    _ if minor => vec![
                        format!("scale.{root}.major.up"),
                        "interval.build.3.up".into(),
                    ],
                    _ if root == 0 => {
                        vec!["interval.build.2.up".into(), "interval.build.1.up".into()]
                    }
                    _ => vec!["scale.0.major.both".into()],
                };
                add(
                    &mut graph,
                    format!("{base}.{suffix}"),
                    format!("{} {mode} scale · {suffix}", ROOTS[root]),
                    Task::Scale {
                        root,
                        minor,
                        direction,
                    },
                    &deps,
                );
                for (index, bpm) in [60, 90, 120].into_iter().enumerate() {
                    let prerequisite = if index == 0 {
                        format!("{base}.{suffix}")
                    } else {
                        format!("{base}.{suffix}.bpm{}", [60, 90, 120][index - 1])
                    };
                    add(
                        &mut graph,
                        format!("{base}.{suffix}.bpm{bpm}"),
                        format!("{} {mode} scale · {suffix} · {bpm} BPM", ROOTS[root]),
                        Task::TimedScale {
                            root,
                            minor,
                            direction,
                            bpm,
                        },
                        &[prerequisite],
                    );
                }
            }
        }
        for quality in 0..QUALITIES.len() {
            let mut deps = vec![
                format!("interval.build.{}.up", CHORDS[quality][1]),
                format!("interval.build.{}.up", CHORDS[quality][2]),
            ];
            if root != 0 {
                deps.push(format!("chord.build.0.{quality}"));
            }
            if quality >= 4 {
                let parent = match quality {
                    4 | 5 => 0,
                    6 | 9 => 1,
                    7 | 8 => 2,
                    10 => 4,
                    11 => 5,
                    12 => 6,
                    _ => 0,
                };
                deps.push(format!("chord.build.{root}.{parent}"));
                let interval = CHORDS[quality].last().unwrap() % 12;
                deps.push(format!("interval.build.{interval}.up"));
            }
            add(
                &mut graph,
                format!("chord.build.{root}.{quality}"),
                format!("Construct {} {} chord", ROOTS[root], QUALITIES[quality]),
                Task::BuildChord { root, quality },
                &deps,
            );
            add(
                &mut graph,
                format!("chord.hear.{root}.{quality}"),
                format!("Recognize {} {} chord", ROOTS[root], QUALITIES[quality]),
                Task::HearChord { root, quality },
                &[format!("chord.build.{root}.{quality}")],
            );
            for inversion in 0..CHORDS[quality].len() {
                let prerequisite = if inversion == 0 {
                    format!("chord.build.{root}.{quality}")
                } else {
                    format!("inversion.build.{root}.{quality}.{}", inversion - 1)
                };
                add(
                    &mut graph,
                    format!("inversion.build.{root}.{quality}.{inversion}"),
                    format!(
                        "Construct {} {} · inversion {inversion}",
                        ROOTS[root], QUALITIES[quality]
                    ),
                    Task::Inversion {
                        root,
                        quality,
                        inversion,
                        hearing: false,
                    },
                    &[prerequisite],
                );
                // Symmetrical augmented/diminished-seventh pitch sets do not
                // identify a unique root or inversion without extra context.
                if quality != 3 && quality != 8 {
                    add(
                        &mut graph,
                        format!("inversion.hear.{root}.{quality}.{inversion}"),
                        format!(
                            "Recognize {} {} · inversion {inversion}",
                            ROOTS[root], QUALITIES[quality]
                        ),
                        Task::Inversion {
                            root,
                            quality,
                            inversion,
                            hearing: true,
                        },
                        &[
                            format!("inversion.build.{root}.{quality}.{inversion}"),
                            format!("chord.hear.{root}.{quality}"),
                        ],
                    );
                }
            }
        }
        let degrees = [0, 2, 4, 5, 7, 9, 11];
        for first in 0..7 {
            for second in first + 1..7 {
                let interval = degrees[second] - degrees[first];
                add(
                    &mut graph,
                    format!("context.{root}.{first}.{second}"),
                    format!(
                        "{} major · hear degrees {} and {}",
                        ROOTS[root],
                        first + 1,
                        second + 1
                    ),
                    Task::ContextInterval {
                        root,
                        first,
                        second,
                    },
                    &[
                        format!("scale.{root}.major.both"),
                        format!("interval.hear.{interval}.up"),
                    ],
                );
            }
        }
    }
    crate::courses::extend(&mut graph);
    crate::courses::prioritize_foundations(&mut graph);
    graph
}

pub fn validate(graph: &[Skill]) -> Result<(), String> {
    if graph
        .iter()
        .any(|skill| skill.id.is_empty() || skill.title.trim().is_empty())
    {
        return Err("Every curriculum skill needs a stable ID and descriptive title".into());
    }
    let map: BTreeMap<_, _> = graph
        .iter()
        .map(|skill| (skill.id.as_str(), skill))
        .collect();
    if map.len() != graph.len() {
        return Err("Duplicate curriculum skill ID".into());
    }
    fn visit<'a>(
        id: &'a str,
        map: &BTreeMap<&'a str, &'a Skill>,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            return Err(format!("Curriculum cycle at {id}"));
        }
        let skill = map
            .get(id)
            .ok_or_else(|| format!("Missing prerequisite {id}"))?;
        for requirement in &skill.requires {
            visit(&requirement.skill, map, active, done)?;
        }
        active.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in map.keys() {
        visit(id, &map, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

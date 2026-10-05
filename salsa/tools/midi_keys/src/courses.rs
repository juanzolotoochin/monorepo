//! Curriculum expansion. IDs are stable; new prerequisites never copy mastery
//! between hearing, construction, keys, or scale families.
use crate::curriculum::{add, Direction, Skill, Task, ROOTS};
use crate::music::Scale;
use crate::practice::{Harmony, Motif, Practice, Rhythm};

fn tone(root: usize, scale: Scale, degree: usize) -> String {
    format!("tone.{root}.{}.{}", scale.id(), degree + 1)
}
fn tonic(root: usize, scale: Scale) -> String {
    format!("tonic.{root}.{}", scale.id())
}
fn melody(root: usize, scale: Scale, length: usize) -> String {
    if scale == Scale::Major && length >= 3 {
        format!("melody.{root}.{length}")
    } else {
        format!("dictation.{root}.{}.{length}", scale.id())
    }
}
fn scale_build(root: usize, scale: Scale, direction: &str) -> String {
    format!("scale.{root}.{}.{direction}", scale.id())
}
fn movement(root: usize, scale: Scale, triad: bool, direction: Direction) -> String {
    format!(
        "movement.{root}.{}.{}.{}",
        scale.id(),
        if triad { "triad" } else { "scale" },
        if direction == Direction::Up {
            "up"
        } else {
            "down"
        }
    )
}
fn scale_hear(root: usize, scale: Scale) -> String {
    format!("scale.hear.{root}.{}", scale.id())
}
fn harmony(root: usize, scale: Scale, suffix: &str) -> String {
    format!("harmony.{root}.{}.{suffix}", scale.id())
}
fn add_practice(graph: &mut Vec<Skill>, id: String, task: Practice, deps: Vec<String>) {
    let title = match &task {
        Practice::Tonic { root, scale } => format!("Find tonic · {} {}", ROOTS[*root], scale.id()),
        Practice::Degree {
            root,
            scale,
            degree,
        } => format!(
            "Hear scale tone {} · {} {}",
            degree + 1,
            ROOTS[*root],
            scale.id()
        ),
        Practice::Movement {
            root,
            scale,
            direction,
            triad,
        } => format!(
            "Hear {} movement · {} {} · {}",
            if *triad { "tonic-triad" } else { "scale-tone" },
            ROOTS[*root],
            scale.id(),
            if *direction == Direction::Up {
                "up"
            } else {
                "down"
            }
        ),
        Practice::Resolve {
            root,
            scale,
            degree,
        } => format!(
            "Resolve tone {} · {} {}",
            degree + 1,
            ROOTS[*root],
            scale.id()
        ),
        Practice::Scale {
            root,
            scale,
            direction,
            hear,
            bpm,
        } => format!(
            "{} {} {} · {direction:?}{}",
            if *hear { "Recognize" } else { "Play" },
            ROOTS[*root],
            scale.id(),
            bpm.map(|b| format!(" · {b} BPM")).unwrap_or_default()
        ),
        Practice::Melody {
            root,
            scale,
            length,
            motif,
            rhythmic,
        } => format!(
            "{} {} · {length}-note {motif:?} phrase{}",
            ROOTS[*root],
            scale.id(),
            if *rhythmic { " with rhythm" } else { "" }
        ),
        Practice::Rhythm { kind, bpm } => format!("Rhythm · {kind:?} · {bpm} BPM"),
        Practice::Meter { beats, bpm } => format!("Find downbeats · {beats}/4 · {bpm} BPM"),
        Practice::Harmony {
            root,
            scale,
            kind: Harmony::Function(degree),
        } => format!(
            "Hear chord function · {} {} · {}",
            ROOTS[*root],
            scale.id(),
            if *scale == Scale::Minor {
                [
                    "i / tonic",
                    "ii° / supertonic",
                    "III / relative major",
                    "iv / subdominant",
                    "v / minor dominant",
                    "VI / submediant",
                    "VII / subtonic",
                ][*degree]
            } else {
                [
                    "I / tonic",
                    "ii / supertonic",
                    "iii / mediant",
                    "IV / subdominant",
                    "V / dominant",
                    "vi / relative minor",
                    "vii° / leading tone",
                ][*degree]
            }
        ),
        Practice::Harmony { root, scale, kind } => {
            format!("{} {} · {kind:?}", ROOTS[*root], scale.id())
        }
    };
    add(graph, id, title, Task::Practice(task), &deps);
}

pub fn extend(graph: &mut Vec<Skill>) {
    // Replace the old jump straight into three-note major melodies. Keep IDs
    // and past evidence, but future practice uses the new prerequisites.
    for skill in graph.iter_mut() {
        if let Task::Scale {
            root,
            minor: true,
            direction: Direction::Up,
        } = skill.task
        {
            skill.requires = vec![crate::curriculum::Requirement {
                skill: if root == 0 {
                    "interval.build.3.up".into()
                } else {
                    scale_build(0, Scale::Minor, "up")
                },
                score: 7.0,
                attempts: 6,
            }];
        }
        if let Task::ContextInterval {
            root,
            first,
            second,
        } = skill.task
        {
            for degree in [first, second] {
                skill.requires.push(crate::curriculum::Requirement {
                    skill: tone(root, Scale::Major, degree),
                    score: 7.0,
                    attempts: 6,
                });
            }
        }
    }
    // Pulses precede subdivisions, then rests, longer holds, and syncopation.
    for (index, kind) in [
        Rhythm::Pulse,
        Rhythm::Eighths,
        Rhythm::Rests,
        Rhythm::Ties,
        Rhythm::Syncopation,
        Rhythm::SilentPulse,
    ]
    .into_iter()
    .enumerate()
    {
        for bpm in [60, 90, 120] {
            let deps = if bpm != 60 {
                vec![format!(
                    "rhythm.{index}.{}",
                    if bpm == 90 { 60 } else { 90 }
                )]
            } else if index == 0 {
                vec!["interval.build.2.up".into()]
            } else {
                vec![format!(
                    "rhythm.{}.60",
                    if index == 5 { 0 } else { index - 1 }
                )]
            };
            add_practice(
                graph,
                format!("rhythm.{index}.{bpm}"),
                Practice::Rhythm { kind, bpm },
                deps,
            );
        }
    }
    for beats in [2, 3, 4] {
        add_practice(
            graph,
            format!("meter.{beats}"),
            Practice::Meter { beats, bpm: 90 },
            vec!["rhythm.0.90".into()],
        );
    }
    // Start each branch in C; additional keys depend on that branch's own C
    // foundation, never on mastery of all twelve keys of another scale family.
    for root in [0, 9, 7, 5, 2, 10, 4, 3, 11, 8, 6, 1] {
        for scale in Scale::ALL {
            let basic = matches!(scale, Scale::Major | Scale::Minor);
            if basic {
                let deps = if root == 0 {
                    vec![format!(
                        "interval.build.{}.up",
                        if scale == Scale::Major { 2 } else { 3 }
                    )]
                } else {
                    vec![tonic(0, scale)]
                };
                add_practice(
                    graph,
                    tonic(root, scale),
                    Practice::Tonic { root, scale },
                    deps,
                );
            } else {
                for (direction, suffix) in [
                    (Direction::Up, "up"),
                    (Direction::Down, "down"),
                    (Direction::Both, "both"),
                ] {
                    let mut deps = if direction == Direction::Up {
                        vec![scale_build(root, scale.parent(), "up")]
                    } else if direction == Direction::Down {
                        vec![scale_build(root, scale, "up")]
                    } else {
                        vec![
                            scale_build(root, scale, "up"),
                            scale_build(root, scale, "down"),
                        ]
                    };
                    if root != 0 && direction == Direction::Up {
                        deps.push(scale_build(0, scale, "up"));
                    }
                    let id = scale_build(root, scale, suffix);
                    add_practice(
                        graph,
                        id.clone(),
                        Practice::Scale {
                            root,
                            scale,
                            direction,
                            hear: false,
                            bpm: None,
                        },
                        deps,
                    );
                    for bpm in [60, 90, 120] {
                        let prev = if bpm == 60 {
                            id.clone()
                        } else {
                            format!("{id}.bpm{}", if bpm == 90 { 60 } else { 90 })
                        };
                        add_practice(
                            graph,
                            format!("{id}.bpm{bpm}"),
                            Practice::Scale {
                                root,
                                scale,
                                direction,
                                hear: false,
                                bpm: Some(bpm),
                            },
                            vec![prev],
                        );
                    }
                }
            }
            let mut deps = vec![scale_build(root, scale, "up")];
            if !basic {
                deps.push(scale_hear(root, scale.parent()));
            }
            // Recognition is separately tracked per family. Both introductory
            // families enter the pool after their own playing prerequisite.
            add_practice(
                graph,
                scale_hear(root, scale),
                Practice::Scale {
                    root,
                    scale,
                    direction: Direction::Up,
                    hear: true,
                    bpm: None,
                },
                deps,
            );
            let steps = scale.steps();
            let fifth = steps.iter().position(|n| *n == 7).unwrap_or(2);
            let third = steps.iter().position(|n| *n == 3 || *n == 4).unwrap_or(1);
            let mut order = vec![0, fifth, third];
            order.dedup();
            for d in 0..steps.len() {
                if !order.contains(&d) {
                    order.push(d);
                }
            }
            for &degree in &order {
                let mut deps = if degree == 0 || degree == fifth {
                    if basic {
                        vec![tonic(root, scale)]
                    } else {
                        vec![scale_hear(root, scale)]
                    }
                } else if degree == third {
                    vec![tone(root, scale, 0), tone(root, scale, fifth)]
                } else {
                    vec![tone(root, scale, third)]
                };
                if root != 0 {
                    deps.push(tone(0, scale, degree));
                }
                add_practice(
                    graph,
                    tone(root, scale, degree),
                    Practice::Degree {
                        root,
                        scale,
                        degree,
                    },
                    deps,
                );
            }
            // Hearing individual notes doesn't demonstrate hearing their motion.
            // Track each direction in context, independently of isolated intervals.
            for triad in [true, false] {
                for direction in [Direction::Up, Direction::Down] {
                    let mut deps = if triad {
                        vec![melody(root, scale, 2), tone(root, scale, third)]
                    } else {
                        let mut d = vec![movement(root, scale, true, direction)];
                        d.extend((0..steps.len()).map(|n| tone(root, scale, n)));
                        d
                    };
                    if root != 0 {
                        deps.push(movement(0, scale, triad, direction));
                    }
                    add_practice(
                        graph,
                        movement(root, scale, triad, direction),
                        Practice::Movement {
                            root,
                            scale,
                            direction,
                            triad,
                        },
                        deps,
                    );
                }
            }
            for length in [2, 3, 5, 8] {
                let deps = match length {
                    2 => vec![tone(root, scale, 0), tone(root, scale, fifth)],
                    3 => vec![
                        melody(root, scale, 2),
                        tone(root, scale, third),
                        movement(root, scale, true, Direction::Up),
                        movement(root, scale, true, Direction::Down),
                    ],
                    5 => {
                        let mut d = vec![melody(root, scale, 3)];
                        d.extend((0..steps.len()).map(|n| tone(root, scale, n)));
                        d.extend(
                            [Direction::Up, Direction::Down]
                                .map(|direction| movement(root, scale, false, direction)),
                        );
                        d
                    }
                    _ => vec![melody(root, scale, 5)],
                };
                add_practice(
                    graph,
                    melody(root, scale, length),
                    Practice::Melody {
                        root,
                        scale,
                        length,
                        motif: Motif::Free,
                        rhythmic: false,
                    },
                    deps,
                );
            }
            if basic {
                for (index, motif) in [
                    Motif::Repeated,
                    Motif::Steps,
                    Motif::Leaps,
                    Motif::Contour,
                    Motif::Transpose,
                ]
                .into_iter()
                .enumerate()
                {
                    add_practice(
                        graph,
                        format!("motif.{root}.{}.{index}", scale.id()),
                        Practice::Melody {
                            root,
                            scale,
                            length: 5,
                            motif,
                            rhythmic: false,
                        },
                        vec![melody(root, scale, 5)],
                    );
                }
                for length in [3, 5, 8] {
                    add_practice(
                        graph,
                        format!("melody.rhythm.{root}.{}.{length}", scale.id()),
                        Practice::Melody {
                            root,
                            scale,
                            length,
                            motif: Motif::Free,
                            rhythmic: true,
                        },
                        vec![
                            melody(root, scale, length),
                            "rhythm.1.90".into(),
                            "rhythm.3.90".into(),
                        ],
                    );
                }
                for degree in [1, 3, 5, 6] {
                    add_practice(
                        graph,
                        format!("resolve.{root}.{}.{degree}", scale.id()),
                        Practice::Resolve {
                            root,
                            scale,
                            degree,
                        },
                        vec![
                            tone(root, scale, degree),
                            tone(root, scale, 0),
                            tone(root, scale, 2),
                            tone(root, scale, 4),
                        ],
                    );
                }
                harmony_course(graph, root, scale);
            }
        }
    }
}
/// Stages describe breadth and complexity, not a hand-picked path to a few
/// milestones. Prerequisites inherit the earliest stage that needs them.
pub fn prioritize_foundations(graph: &mut [Skill]) {
    for skill in graph.iter_mut() {
        skill.stage = introduction_stage(&skill.task);
    }
    let indices: std::collections::BTreeMap<_, _> = graph
        .iter()
        .enumerate()
        .map(|(i, skill)| (skill.id.clone(), i))
        .collect();
    let mut pending: Vec<_> = (0..graph.len()).collect();
    while let Some(i) = pending.pop() {
        let stage = graph[i].stage;
        let dependencies: Vec<_> = graph[i]
            .requires
            .iter()
            .map(|r| indices[&r.skill])
            .collect();
        for dependency in dependencies {
            if graph[dependency].stage > stage {
                graph[dependency].stage = stage;
                pending.push(dependency);
            }
        }
    }
    // Preserve the authored order within a stage as a deterministic tie-break.
    graph.sort_by_key(|skill| skill.stage);
}

fn introduction_stage(task: &Task) -> u8 {
    // 0: first listening vocabulary; 1: complete foundations;
    // 2: familiar-key fluency; 3: transfer to other keys and extensions;
    // 4+: additional scale families.
    let key_stage = |root| if root == 0 { 0 } else { 3 };
    let scale_stage = |root, scale| {
        if matches!(scale, Scale::Major | Scale::Minor) {
            key_stage(root)
        } else if root == 0 {
            4
        } else {
            5
        }
    };
    match task {
        Task::Reading(task) => task.stage(),
        Task::GuidedScale(task) => introduction_stage(task),
        Task::BuildInterval { semitones, .. } | Task::HearInterval { semitones, .. } => {
            u8::from(![1, 2, 3, 4, 5, 7, 12].contains(semitones))
        }
        Task::BuildChord { root, quality } | Task::HearChord { root, quality } => key_stage(*root)
            .max(if *quality <= 1 {
                0
            } else if *quality <= 3 {
                1
            } else if *quality < 10 {
                2
            } else {
                3
            }),
        Task::Inversion { root, quality, .. } => key_stage(*root).max(if *quality <= 3 {
            1
        } else if *quality < 10 {
            2
        } else {
            3
        }),
        Task::Scale {
            root, direction, ..
        } => key_stage(*root).max(u8::from(*direction != Direction::Up)),
        Task::TimedScale { root, .. } | Task::ContextInterval { root, .. } => {
            key_stage(*root).max(2)
        }
        Task::Practice(Practice::Tonic { root, scale }) => scale_stage(*root, *scale),
        Task::Practice(Practice::Degree {
            root,
            scale,
            degree,
        }) => {
            let steps = scale.steps();
            let basic = *degree == 0 || [3, 4, 7].contains(&steps[*degree]);
            scale_stage(*root, *scale).max(u8::from(!basic))
        }
        Task::Practice(Practice::Movement {
            root, scale, triad, ..
        }) => scale_stage(*root, *scale).max(u8::from(!triad)),
        Task::Practice(Practice::Resolve { root, scale, .. }) => scale_stage(*root, *scale).max(2),
        Task::Practice(Practice::Scale {
            root, scale, bpm, ..
        }) => scale_stage(*root, *scale).max(if bpm.is_some() { 2 } else { 1 }),
        Task::Practice(Practice::Melody {
            root,
            scale,
            length,
            motif,
            rhythmic,
        }) => {
            let stage = if *motif != Motif::Free || *rhythmic || *length > 5 {
                2
            } else if *length > 3 {
                1
            } else {
                0
            };
            scale_stage(*root, *scale).max(stage)
        }
        Task::Practice(Practice::Rhythm { kind, bpm }) => {
            if *bpm > 60 {
                2
            } else {
                match kind {
                    Rhythm::Pulse | Rhythm::Eighths => 0,
                    Rhythm::Rests => 1,
                    _ => 2,
                }
            }
        }
        Task::Practice(Practice::Meter { .. }) => 2,
        Task::Practice(Practice::Harmony { root, scale, kind }) => {
            let basic = matches!(
                kind,
                Harmony::Function(0 | 3 | 4 | 5)
                    | Harmony::Progression(_)
                    | Harmony::Bass(false)
                    | Harmony::Cadence(_)
            );
            scale_stage(*root, *scale).max(if basic { 1 } else { 2 })
        }
    }
}

fn harmony_course(graph: &mut Vec<Skill>, root: usize, scale: Scale) {
    let minor = scale == Scale::Minor;
    for degree in [0, 4, 3, 5, 1, 6, 2] {
        let chord = scale.triad(root, degree);
        let intervals = [chord[1] - chord[0], chord[2] - chord[0]];
        let quality = match intervals {
            [4, 7] => 0,
            [3, 7] => 1,
            _ => 2,
        };
        let mut deps = vec![
            tonic(root, scale),
            tone(root, scale, degree),
            format!("chord.hear.{}.{quality}", chord[0] % 12),
        ];
        if degree != 0 {
            let previous = match degree {
                4 => 0,
                3 => 4,
                5 => 3,
                1 => 5,
                6 => 1,
                _ => 6,
            };
            deps.push(harmony(root, scale, &format!("function.{previous}")));
        }
        add_practice(
            graph,
            harmony(root, scale, &format!("function.{degree}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Function(degree),
            },
            deps,
        );
    }
    for length in [2, 4] {
        let deps = if length == 2 {
            let mut deps: Vec<_> = [0, 3, 4, 5]
                .into_iter()
                .map(|d| harmony(root, scale, &format!("function.{d}")))
                .collect();
            deps.push(harmony(root, scale, "bass.false"));
            deps
        } else {
            vec![harmony(root, scale, "progression.2")]
        };
        add_practice(
            graph,
            harmony(root, scale, &format!("progression.{length}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Progression(length),
            },
            deps,
        );
    }
    for resolved in [false, true] {
        add_practice(
            graph,
            harmony(root, scale, &format!("cadence.{resolved}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Cadence(resolved),
            },
            // A V-I or I-V pair needs these two functions, not a longer course
            // containing unrelated functions and multi-chord bass transcription.
            vec![
                harmony(root, scale, "function.0"),
                harmony(root, scale, "function.4"),
            ],
        );
    }
    for roots in [false, true] {
        let deps = if roots {
            vec![
                harmony(root, scale, "bass.false"),
                harmony(root, scale, "progression.4"),
                format!("inversion.hear.{root}.{}.1", usize::from(minor)),
            ]
        } else {
            vec![
                melody(root, scale, 3),
                movement(root, scale, false, Direction::Up),
                movement(root, scale, false, Direction::Down),
            ]
        };
        add_practice(
            graph,
            harmony(root, scale, &format!("bass.{roots}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Bass(roots),
            },
            deps,
        );
    }
    for thirds in [false, true] {
        add_practice(
            graph,
            harmony(root, scale, &format!("shell.{thirds}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Shell(thirds),
            },
            vec![format!("chord.build.{root}.{}", if minor { 6 } else { 5 })],
        );
    }
    add_practice(
        graph,
        harmony(root, scale, "voice-leading"),
        Practice::Harmony {
            root,
            scale,
            kind: Harmony::VoiceLeading,
        },
        vec![
            harmony(root, scale, "progression.4"),
            format!("inversion.build.{root}.{}.2", usize::from(minor)),
        ],
    );
    for length in [1, 4] {
        let deps = if length == 1 {
            vec![harmony(root, scale, "function.0"), melody(root, scale, 3)]
        } else {
            vec![
                harmony(root, scale, "harmonize.1"),
                harmony(root, scale, "progression.4"),
            ]
        };
        add_practice(
            graph,
            harmony(root, scale, &format!("harmonize.{length}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Harmonize(length),
            },
            deps,
        );
    }
    for thirds in [false, true] {
        for bars in [4, 12] {
            let deps = if bars == 4 {
                vec![
                    harmony(root, scale, &format!("shell.{thirds}")),
                    harmony(root, scale, "progression.4"),
                    "meter.4".into(),
                    "rhythm.3.60".into(),
                ]
            } else {
                vec![harmony(root, scale, &format!("accompany.4.{thirds}"))]
            };
            add_practice(
                graph,
                harmony(root, scale, &format!("accompany.{bars}.{thirds}")),
                Practice::Harmony {
                    root,
                    scale,
                    kind: Harmony::Accompany { bars, thirds },
                },
                deps,
            );
        }
    }
    for changing in [false, true] {
        let deps = if changing {
            vec![
                harmony(root, scale, "coordination.false"),
                harmony(root, scale, "bass.false"),
            ]
        } else {
            vec![
                melody(root, scale, 5),
                "rhythm.1.60".into(),
                "rhythm.3.60".into(),
            ]
        };
        add_practice(
            graph,
            harmony(root, scale, &format!("coordination.{changing}")),
            Practice::Harmony {
                root,
                scale,
                kind: Harmony::Coordination(changing),
            },
            deps,
        );
    }
}

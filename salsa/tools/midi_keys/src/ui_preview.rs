use keyboard::{Event, Keyboard};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: ui_preview FILE.svg [WIDTH HEIGHT]");
    let width = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(120);
    let height = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(38);
    let mut keyboard = Keyboard::default();
    let now = Instant::now();
    for (i, note) in [48, 55, 60, 64, 67, 72, 71, 67, 64, 60, 64, 67]
        .iter()
        .enumerate()
    {
        keyboard.apply(Event::Note {
            channel: 0,
            note: *note,
            velocity: 74 + (i % 4) as u8 * 10,
        });
        keyboard.strikes.back_mut().unwrap().at =
            now - Duration::from_millis((12 - i) as u64 * 400);
        keyboard.apply(Event::Note {
            channel: 0,
            note: *note,
            velocity: 0,
        });
        let strike = keyboard.strikes.back_mut().unwrap();
        strike.released_at = Some(strike.at + Duration::from_millis(250));
    }
    for note in [60, 64, 67] {
        keyboard.apply(Event::Note {
            channel: 0,
            note,
            velocity: 104,
        });
        keyboard.strikes.back_mut().unwrap().at = now - Duration::from_millis(650);
    }
    keyboard.apply(Event::Sustain {
        channel: 0,
        down: true,
    });
    let training = args.get(4).is_some_and(|value| value == "--training");
    if training {
        keyboard.clear();
    }
    let view = ui::View {
        keyboard: &keyboard,
        source: "UltraLite-mk5 / MIDI 1",
        connected: true,
        demo: false,
        silent: false,
        muted: false,
        note_names: false,
        volume: 0.6,
        now: Instant::now(),
    };
    let svg = if training {
        let directory = tempfile::tempdir().unwrap();
        let mut session = if args.get(5).is_some_and(|value| value == "--browser-demo") {
            let mut session = trainer::Session::browse().unwrap();
            for _ in 0..6 {
                session.browser_category(true);
            }
            session
        } else {
            trainer::Session::open(directory.path().join("learner.json")).unwrap()
        };
        if args.get(5).is_some_and(|value| value == "--rhythm-demo") {
            session = trainer::Session::browse().unwrap();
            for &key in b"rhythm.0.60" {
                session.browser_edit(key);
            }
            session.browser_select();
            let start = Instant::now();
            session.start(start);
            for i in 0..8 {
                let at =
                    start + Duration::from_millis(3000 + i * 1000 + if i == 3 { 260 } else { 0 });
                session.input(
                    Event::Note {
                        channel: 0,
                        note: 60,
                        velocity: 90,
                    },
                    at,
                );
                session.input(
                    Event::Note {
                        channel: 0,
                        note: 60,
                        velocity: 0,
                    },
                    at + Duration::from_millis(if i == 5 { 250 } else { 940 }),
                );
            }
            session.tick(start + Duration::from_secs(12)).unwrap();
        }
        if let Some(id) = args.get(5).and_then(|arg| arg.strip_prefix("--reading=")) {
            session = trainer::Session::preview(id).unwrap();
        }
        if !args.iter().any(|arg| arg == "--prepare") {
            session.start(Instant::now());
        }
        if let Some(answer) = args.iter().find_map(|a| a.strip_prefix("--answer=")) {
            let start = Instant::now() + Duration::from_secs(30);
            session.tick(start).unwrap();
            if session.view().exploring {
                session.submit(false).unwrap();
            }
            for (i, note) in answer
                .split(',')
                .map(|n| n.parse::<usize>().unwrap())
                .enumerate()
            {
                let at = start + Duration::from_millis(i as u64 * 500);
                session.input(
                    Event::Note {
                        channel: 0,
                        note,
                        velocity: 90,
                    },
                    at,
                );
                session.input(
                    Event::Note {
                        channel: 0,
                        note,
                        velocity: 0,
                    },
                    at + Duration::from_millis(350),
                );
            }
            session.submit(false).unwrap();
        }
        let mut training = session.view();
        if args.get(5).is_some_and(|value| value == "--insights-demo") {
            training.insights = vec![
                "Mastered: C natural minor scale · ascending".into(),
                "Mastered: seconds and thirds · ascending recognition".into(),
                "Confusion: major 3rd → perfect 5th · ascending (4/10 recent major 3rd prompts)"
                    .into(),
                "Checking: through fifths and octaves · ascending (4/6 intervals established)"
                    .into(),
            ];
        }
        ui::training_preview_svg(width, height, &view, &training)
    } else {
        ui::preview_svg(width, height, &view)
    };
    std::fs::write(path, svg).unwrap();
}

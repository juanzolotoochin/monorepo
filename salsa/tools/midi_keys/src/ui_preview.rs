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
        let mut session = trainer::Session::open(directory.path().join("learner.json")).unwrap();
        session.ready(Instant::now());
        ui::training_preview_svg(width, height, &view, &session.view())
    } else {
        ui::preview_svg(width, height, &view)
    };
    std::fs::write(path, svg).unwrap();
}

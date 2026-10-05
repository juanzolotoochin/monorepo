use keyboard::{note_name, Keyboard};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph, Widget, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::Instant;

const BG: Color = Color::Rgb(10, 17, 27);
const PANEL: Color = Color::Rgb(15, 24, 36);
const BORDER: Color = Color::Rgb(38, 55, 70);
const TEXT: Color = Color::Rgb(227, 236, 239);
const MUTED: Color = Color::Rgb(116, 140, 155);
const TEAL: Color = Color::Rgb(89, 225, 192);
const AMBER: Color = Color::Rgb(244, 190, 105);
const ABOVE: Color = Color::Rgb(133, 183, 255);
const BELOW: Color = Color::Rgb(227, 153, 220);

pub struct View<'a> {
    pub keyboard: &'a Keyboard,
    pub source: &'a str,
    pub connected: bool,
    pub demo: bool,
    pub silent: bool,
    pub muted: bool,
    pub note_names: bool,
    pub volume: f32,
    pub now: Instant,
}

pub struct Display(Terminal<CrosstermBackend<io::Stdout>>);

impl Display {
    pub fn new() -> io::Result<Self> {
        // The native terminal guard already clears the alternate screen. Avoid
        // Terminal::clear(), which queries the cursor and competes with MIDI's
        // terminal input loop for a terminal response.
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(Self(terminal))
    }

    pub fn draw(&mut self, view: &View<'_>) -> io::Result<()> {
        self.0.draw(|frame| draw(frame, view))?;
        Ok(())
    }
    pub fn draw_training(
        &mut self,
        view: &View<'_>,
        session: &trainer::TrainingView<'_>,
    ) -> io::Result<()> {
        self.0.draw(|frame| draw_training(frame, view, session))?;
        Ok(())
    }
}

pub fn draw_training(frame: &mut Frame<'_>, view: &View<'_>, session: &trainer::TrainingView<'_>) {
    use trainer::Phase;
    let area = frame.area();
    if !training_fits(area.width, area.height) {
        frame.render_widget(
            Block::default().style(Style::default().bg(BG).fg(TEXT)),
            area,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(text("EAR / THEORY · PAUSED", AMBER)),
                Line::from("Resize to at least 64 × 22 to resume."),
                Line::from("Exercise audio is stopped. q to quit."),
            ])
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(9),
        Constraint::Length(if area.height >= 34 { 8 } else { 0 }),
        Constraint::Length(3),
    ])
    .split(area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    }));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                text("◈  EAR / THEORY", TEAL),
                text(format!("    Exercise {}", session.completed + 1), TEXT),
            ]),
            Line::from(text(
                format!(
                    "{} skills established · {} in the curriculum",
                    session.mastered, session.total
                ),
                MUTED,
            )),
        ]),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(text(
            if view.connected {
                format!("●  {}", view.source)
            } else {
                "◌  Tap and release a MIDI key to connect.".into()
            },
            MUTED,
        )),
        parts[1],
    );
    let title = session.title;
    let (exercise_area, log_area) = if area.width >= 120 {
        let columns = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
            .spacing(1)
            .split(parts[2]);
        (columns[0], columns[1])
    } else {
        let needed = wrapped_rows(session.prompt, parts[2].width.saturating_sub(4)) + 4;
        let rows = Layout::vertical([
            Constraint::Min(7),
            Constraint::Length(parts[2].height.saturating_sub(needed.max(7)).clamp(4, 10)),
        ])
        .split(parts[2]);
        (rows[0], rows[1])
    };
    let block = card(title);
    let inner = block.inner(exercise_area).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    frame.render_widget(block, exercise_area);
    let prompt_rows = wrapped_rows(session.prompt, inner.width);
    let mut lines = if inner.height >= prompt_rows + 6 {
        vec![Line::from(text(session.reason, MUTED)), Line::from("")]
    } else {
        Vec::new()
    };
    if session.phase != Phase::Rest {
        lines.push(instruction_line(session.prompt, session.prompt_highlights));
        if inner.height >= prompt_rows + 3 {
            lines.push(Line::from(""));
        }
        let status = match session.phase {
            Phase::Waiting => {
                if view.connected {
                    "Release keys to begin."
                } else {
                    "Waiting for your keyboard."
                }
            }
            Phase::Listening => "●  Listen…  r to restart",
            Phase::Answering if session.exploring => {
                "Explore freely · ungraded · Enter to start your answer"
            }
            Phase::Answering if session.timed_phrase => "Your turn · auto finish · r to restart",
            Phase::Answering => "Play all answer notes · submits automatically",
            Phase::Feedback => "Next exercise shortly… Enter to continue now.",
            Phase::Rest => "No exercises due.",
        };
        lines.push(Line::from(text(status, TEAL)));
        if session.phase != Phase::Listening && !session.played.is_empty() {
            lines.push(Line::from(text(
                format!(
                    "Played: {}",
                    session
                        .played
                        .iter()
                        .take(20)
                        .map(|&n| session
                            .spelling
                            .map_or_else(|| note_name(n), |s| s.note_name(n)))
                        .collect::<Vec<_>>()
                        .join("  ")
                ),
                MUTED,
            )));
        }
        if session.phase == Phase::Feedback {
            // Keep the result and next action visible even on short terminals.
            // Repeating the original prompt would push long scale feedback out.
            lines = vec![
                Line::from(text(session.feedback, AMBER)),
                Line::from(""),
                Line::from(text(status, TEAL)),
            ];
        } else if !session.feedback.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(text(session.feedback, AMBER)));
        }
    } else {
        lines.push(Line::from(text("Everything currently available is established. Come back for a retention check in two weeks.",TEXT)));
        lines.push(Line::from(text("Your progress is saved. q to quit.", TEAL)));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    draw_exercise_log(frame, log_area, &session.recent_exercises);
    if parts[3].height > 0 {
        let piano = card("YOUR KEYBOARD");
        let inner = piano.inner(parts[3]).inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        frame.render_widget(piano, parts[3]);
        frame.render_widget(
            Keys {
                keyboard: view.keyboard,
                span: visible_span(area.width),
                note_names: view.note_names,
                spelling: session.spelling,
            },
            inner,
        );
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                text("r", TEAL),
                text(" replay  ", MUTED),
                text("⌫", TEAL),
                text(" clear  ", MUTED),
                text("h", TEAL),
                text(" hint  ", MUTED),
                text("x", TEAL),
                text(" don't know  ", MUTED),
                text("Enter", TEAL),
                text(
                    match session.phase {
                        Phase::Feedback => " next",
                        Phase::Answering if session.exploring => " start answer",
                        Phase::Answering => " submit",
                        _ => "",
                    },
                    MUTED,
                ),
            ]),
            Line::from(text(
                "−/+ volume · m mute · n labels · space panic · q quit",
                MUTED,
            )),
        ]),
        parts[4],
    );
}

pub fn training_fits(width: u16, height: u16) -> bool {
    width >= 64 && height >= 22
}

fn wrapped_rows(text: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let mut rows = 1usize;
    let mut used = 0;
    for word in text.split_whitespace() {
        let len = Span::raw(word).width();
        if used > 0 && used + 1 + len > width {
            rows += 1;
            used = 0;
        }
        if used > 0 {
            used += 1;
        }
        used += len;
        while used > width {
            rows += 1;
            used -= width;
        }
    }
    rows.min(u16::MAX as usize) as u16
}

fn draw_exercise_log(frame: &mut Frame<'_>, area: Rect, entries: &[trainer::ExerciseLogEntry<'_>]) {
    let block = card("RECENT EXERCISES");
    let inner = block.inner(area).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    frame.render_widget(block, area);
    let mut lines = Vec::new();
    if entries.is_empty() {
        lines.push(Line::from(text(
            "Your completed exercises will appear here.",
            MUTED,
        )));
    }
    for entry in entries {
        lines.push(Line::from(vec![
            text(
                if entry.correct { "✓ " } else { "✗ " },
                if entry.correct { TEAL } else { BELOW },
            ),
            text(format!("#{}  {}", entry.number, entry.title), TEXT),
            text(if entry.assisted { " · hint used" } else { "" }, AMBER),
        ]));
        lines.push(Line::from(vec![
            text("Asked: ", MUTED),
            text(entry.prompt, TEXT),
        ]));
        lines.push(Line::from(vec![
            text("Played: ", MUTED),
            text(&entry.entered, TEAL),
        ]));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn instruction_line<'a>(prompt: &'a str, highlights: &[trainer::PromptHighlight]) -> Line<'a> {
    let mut spans = Vec::new();
    let mut end = 0;
    for highlight in highlights {
        spans.push(Span::styled(
            &prompt[end..highlight.range.start],
            Style::default().fg(TEXT),
        ));
        let color = match highlight.role {
            trainer::PromptRole::Note => TEAL,
            trainer::PromptRole::Interval => AMBER,
            trainer::PromptRole::Above => ABOVE,
            trainer::PromptRole::Below => BELOW,
        };
        spans.push(Span::styled(
            &prompt[highlight.range.clone()],
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        end = highlight.range.end;
    }
    spans.push(Span::styled(&prompt[end..], Style::default().fg(TEXT)));
    Line::from(spans)
}

fn text(value: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(value.into(), Style::default().fg(color))
}

fn card(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .title(Line::from(text(format!("  {title}  "), MUTED)))
}

/// Use every available three-column white key, up to eight octaves plus the
/// final C. Partial octaves avoid leaving unused space between octave widths.
pub fn visible_span(terminal_width: u16) -> usize {
    let white_keys = (terminal_width.saturating_sub(8) / 3).clamp(15, 57) as usize;
    let intervals = white_keys - 1;
    intervals / 7 * 12 + [0, 2, 4, 5, 7, 9, 11][intervals % 7]
}

pub fn draw(frame: &mut Frame<'_>, view: &View<'_>) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    if area.width < 64 || area.height < 22 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(text("MIDI KEYS", TEAL)),
                Line::from(""),
                Line::from(text("Resize to at least 64 × 22", TEXT)),
                Line::from(text("Your piano keeps playing.  q to quit", MUTED)),
            ])
            .alignment(Alignment::Center),
            Rect::new(0, area.height.saturating_sub(4) / 2, area.width, 4),
        );
        return;
    }
    let root = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let roomy = area.height >= 32;
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(10),
        if roomy {
            Constraint::Min(8)
        } else {
            Constraint::Length(0)
        },
        Constraint::Length(2),
    ])
    .split(root);

    let heading =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(25)]).split(sections[0]);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    "  ◈  MIDI KEYS",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                text("   /   GRAND PIANO", MUTED),
            ]),
            Line::from(text("     A little space to play.", MUTED)),
        ]),
        heading[0],
    );
    let badge = if view.demo {
        "●  DEMO SESSION"
    } else if view.connected {
        "●  FREE PLAY"
    } else {
        "◌  WAITING FOR MIDI"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(badge, if view.demo { AMBER } else { TEAL })),
            Line::from(text("SALAMANDER  ·  YAMAHA C5", MUTED)),
        ])
        .alignment(Alignment::Right),
        heading[1],
    );

    let input = if view.demo {
        "Example chords · no MIDI device required"
    } else if view.connected {
        view.source
    } else {
        "Press a key on your MIDI keyboard to connect."
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            text("  INPUT   ", MUTED),
            text(
                input,
                if view.connected || view.demo {
                    TEXT
                } else {
                    TEAL
                },
            ),
        ])),
        sections[1],
    );

    let cards = Layout::horizontal([
        Constraint::Percentage(44),
        Constraint::Percentage(28),
        Constraint::Percentage(28),
    ])
    .spacing(1)
    .split(sections[2]);
    let held: Vec<_> = (0..128)
        .filter(|&n| view.keyboard.velocity(n) > 0)
        .collect();
    let active = card("HELD NOTES");
    let active_inner = active.inner(cards[0]);
    frame.render_widget(active, cards[0]);
    let mut chips = vec![text(" ", TEXT)];
    if held.is_empty() {
        chips.push(text("—", MUTED));
    }
    for note in held.iter().take(5) {
        chips.push(Span::styled(
            format!(" {} ", note_name(*note)),
            Style::default()
                .fg(BG)
                .bg(TEAL)
                .add_modifier(Modifier::BOLD),
        ));
        chips.push(text(" ", TEXT));
    }
    if held.len() > 5 {
        chips.push(text(format!("+{}", held.len() - 5), MUTED));
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(chips),
            Line::from(text(
                if held.is_empty() {
                    " Play a note. Make it yours.".into()
                } else {
                    format!(
                        " {} {} held",
                        held.len(),
                        if held.len() == 1 { "key" } else { "keys" }
                    )
                },
                MUTED,
            )),
        ]),
        active_inner,
    );

    let touch = card("VELOCITY");
    let touch_inner = touch.inner(cards[1]);
    frame.render_widget(touch, cards[1]);
    let velocity = view.keyboard.last_strike.map(|s| s.1).unwrap_or(0);
    let touch_rows =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(touch_inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            text(format!(" {velocity:>3}"), AMBER),
            text(" / 127", MUTED),
        ])),
        touch_rows[0],
    );
    frame.render_widget(
        Gauge::default()
            .ratio(velocity as f64 / 127.0)
            .label("")
            .gauge_style(Style::default().fg(AMBER).bg(BORDER)),
        touch_rows[1].inner(Margin {
            horizontal: 1,
            vertical: 0,
        }),
    );

    let output = card("PIANO OUTPUT");
    let output_inner = output.inner(cards[2]);
    frame.render_widget(output, cards[2]);
    let label = if view.silent {
        " VISUAL ONLY".into()
    } else if view.muted {
        " MUTED".into()
    } else {
        format!(" {:.0}%  ·  STEREO", view.volume * 100.0)
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(
                label,
                if view.silent || view.muted {
                    MUTED
                } else {
                    TEAL
                },
            )),
            Line::from(text(
                if view.keyboard.sustaining() {
                    " SUSTAIN  ● DOWN"
                } else {
                    " SUSTAIN  ○ UP"
                },
                if view.keyboard.sustaining() {
                    AMBER
                } else {
                    MUTED
                },
            )),
        ]),
        output_inner,
    );

    let range = format!(
        "{} — {}",
        note_name(view.keyboard.start),
        note_name((view.keyboard.start + visible_span(area.width)).min(127))
    );
    let piano = card("KEYBOARD").title_bottom(Line::from(vec![
        text(format!("  {range}  "), TEXT),
        text(
            if view.keyboard.follow {
                "AUTO FOLLOW"
            } else {
                "MANUAL RANGE"
            },
            TEAL,
        ),
        text("  ·  [ / ] shift octave  ", MUTED),
    ]));
    let piano_inner = piano.inner(sections[3]).inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.render_widget(piano, sections[3]);
    frame.render_widget(
        Keys {
            keyboard: view.keyboard,
            span: visible_span(area.width),
            note_names: view.note_names,
            spelling: None,
        },
        piano_inner,
    );

    if roomy {
        let bottom = Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)])
            .spacing(1)
            .split(sections[4]);
        history(frame, bottom[0], view);
        let instrument = card("THE INSTRUMENT");
        let inner = instrument.inner(bottom[1]).inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        frame.render_widget(instrument, bottom[1]);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "Salamander Grand",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                )),
                Line::from(text("Yamaha C5 · Alexander Holm", MUTED)),
                Line::from(""),
                Line::from(vec![
                    text("48 kHz", TEAL),
                    text("   STEREO   256 VOICES", MUTED),
                ]),
                Line::from(text("Velocity layers · soft room reverb", MUTED)),
                Line::from(text("SoundFont by FreePats", MUTED)),
            ]),
            inner,
        );
    }
    let footer = if area.width >= 96 {
        vec![
            text("  [ ]", TEAL),
            text(" octave   ", MUTED),
            text("f", TEAL),
            text(" follow   ", MUTED),
            text("− +", TEAL),
            text(" volume   ", MUTED),
            text("m", TEAL),
            text(" mute   ", MUTED),
            text("n", TEAL),
            text(" labels   ", MUTED),
            text("space", TEAL),
            text(" silence all   ", MUTED),
            text("q", TEAL),
            text(" quit", MUTED),
        ]
    } else {
        vec![
            text(" [ ]", TEAL),
            text("oct ", MUTED),
            text("f", TEAL),
            text(" follow ", MUTED),
            text("±", TEAL),
            text(" vol ", MUTED),
            text("m", TEAL),
            text(" mute ", MUTED),
            text("n", TEAL),
            text(" names ", MUTED),
            text("space", TEAL),
            text(" panic ", MUTED),
            text("q", TEAL),
            text(" quit", MUTED),
        ]
    };
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), Line::from(footer)]),
        sections[5],
    );
}

struct Keys<'a> {
    keyboard: &'a Keyboard,
    span: usize,
    note_names: bool,
    spelling: Option<&'a trainer::NoteSpelling>,
}

impl Keys<'_> {
    fn note_name(&self, note: usize) -> String {
        self.spelling
            .map_or_else(|| note_name(note), |s| s.note_name(note))
    }
}

fn paint(buffer: &mut Buffer, area: Rect, color: Color) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buffer[(x, y)]
                .set_symbol(" ")
                .set_style(Style::default().bg(color));
        }
    }
}

impl Widget for Keys<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.height < 3 || area.width < 45 {
            return;
        }
        let end = (self.keyboard.start + self.span).min(127);
        let whites: Vec<_> = (self.keyboard.start..=end)
            .filter(|n| !matches!(n % 12, 1 | 3 | 6 | 8 | 10))
            .collect();
        let key_width = 3;
        let width = key_width * whites.len() as u16;
        let left = area.x + (area.width - width) / 2;
        let height = area.height.min(6);
        for (index, &note) in whites.iter().enumerate() {
            let x = left + index as u16 * key_width;
            let held = self.keyboard.velocity(note) > 0;
            let key = Rect::new(x, area.y, key_width, height);
            let color = if held {
                TEAL
            } else {
                Color::Rgb(218, 226, 227)
            };
            paint(buffer, key, color);
            paint(
                buffer,
                Rect::new(x, area.y, key.width, 1),
                if held {
                    Color::Rgb(159, 251, 224)
                } else {
                    Color::Rgb(245, 247, 240)
                },
            );
            paint(
                buffer,
                Rect::new(x, key.bottom() - 1, key.width, 1),
                if held {
                    Color::Rgb(45, 155, 143)
                } else {
                    Color::Rgb(148, 168, 175)
                },
            );
            if index + 1 < whites.len() {
                for y in key.y..key.bottom() {
                    buffer[(key.right() - 1, y)]
                        .set_symbol("│")
                        .set_fg(Color::Rgb(126, 149, 156));
                }
            }
            if self.note_names {
                let label = self.note_name(note);
                let label = if label.len() > key.width as usize {
                    &label[..1]
                } else {
                    &label
                };
                let label_x = x + (key.width.saturating_sub(label.len() as u16)) / 2;
                buffer.set_stringn(
                    label_x,
                    key.bottom() - 2,
                    label,
                    key.width as usize,
                    Style::default()
                        .fg(BG)
                        .bg(color)
                        .add_modifier(Modifier::BOLD),
                );
            }
            if held && height >= 6 {
                buffer.set_string(
                    x + key.width / 2,
                    key.bottom() - 4,
                    "●",
                    Style::default().fg(BG).bg(color),
                );
            }
        }
        let black_height = (height * 3 / 5).max(2);
        let black_width = (key_width * 2 / 3).max(2);
        for (index, &note) in whites.iter().enumerate() {
            if note >= end || !matches!((note + 1) % 12, 1 | 3 | 6 | 8 | 10) {
                continue;
            }
            let x = left + (index as u16 + 1) * key_width - black_width / 2;
            let held = self.keyboard.velocity(note + 1) > 0;
            let color = if held { AMBER } else { Color::Rgb(25, 35, 47) };
            paint(
                buffer,
                Rect::new(x, area.y, black_width, black_height),
                color,
            );
            paint(
                buffer,
                Rect::new(x, area.y + black_height - 1, black_width, 1),
                if held {
                    Color::Rgb(161, 111, 53)
                } else {
                    Color::Rgb(52, 69, 82)
                },
            );
            if black_height >= 3 && (held || self.note_names) {
                let label = if held {
                    "●".into()
                } else {
                    self.note_name(note + 1).chars().take(2).collect::<String>()
                };
                buffer.set_stringn(
                    x,
                    area.y + black_height - 2,
                    label,
                    black_width as usize,
                    Style::default().fg(if held { BG } else { MUTED }).bg(color),
                );
            }
        }
    }
}

fn history(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    let block = card("NOTE TRAIL")
        .title_bottom(Line::from(text("  LAST 6 SECONDS  ", MUTED)).alignment(Alignment::Left))
        .title_bottom(Line::from(text("  NOW  ", MUTED)).alignment(Alignment::Right));
    let inner = block.inner(area).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    frame.render_widget(block, area);
    if inner.width < 12 || inner.height < 3 {
        return;
    }
    let recent: Vec<_> = view
        .keyboard
        .strikes
        .iter()
        .filter(|strike| {
            strike
                .released_at
                .is_none_or(|end| view.now.saturating_duration_since(end).as_secs_f32() < 6.0)
        })
        .collect();
    // Each half-cell is one semitone. Preserve both pitches and their colors
    // when two notes land in the upper and lower halves of the same cell.
    let levels = (usize::from(inner.height) * 2).min(128);
    let latest = recent.last().map(|strike| strike.note).unwrap_or(60);
    let lowest = recent
        .iter()
        .map(|strike| strike.note)
        .min()
        .unwrap_or(latest);
    let highest = recent
        .iter()
        .map(|strike| strike.note)
        .max()
        .unwrap_or(latest);
    let center = if highest - lowest < levels {
        (lowest + highest) / 2
    } else {
        latest
    };
    let low = center.saturating_sub(levels / 2).min(128 - levels);
    let buffer = frame.buffer_mut();
    for row in 0..inner.height {
        let y = inner.y + row;
        if usize::from(row) * 2 >= levels {
            continue;
        }
        let upper_note = low + levels - 1 - usize::from(row) * 2;
        let lower_note = upper_note - 1;
        let label = if row == 0 {
            Some(upper_note)
        } else if usize::from(row + 1) * 2 == levels {
            Some(lower_note)
        } else if upper_note % 12 == 0 {
            Some(upper_note)
        } else if lower_note % 12 == 0 {
            Some(lower_note)
        } else {
            None
        };
        if let Some(note) = label {
            buffer.set_string(
                inner.x,
                y,
                note_name(note),
                Style::default().fg(MUTED).bg(PANEL),
            );
        }
        for x in (inner.x + 5..inner.right()).step_by(4) {
            buffer.set_string(x, y, "·", Style::default().fg(BORDER).bg(PANEL));
        }
    }
    for strike in recent {
        let age = view
            .now
            .saturating_duration_since(strike.at)
            .as_secs_f32()
            .min(6.0);
        let end_age = strike
            .released_at
            .map(|end| view.now.saturating_duration_since(end).as_secs_f32())
            .unwrap_or(0.0);
        if strike.note < low || strike.note >= low + levels {
            continue;
        }
        let x = inner.x + 5 + ((1.0 - age / 6.0) * (inner.width - 8) as f32) as u16;
        let end_x = inner.x + 5 + ((1.0 - end_age / 6.0) * (inner.width - 8) as f32) as u16;
        let level = (low + levels - 1 - strike.note) as u16;
        let y = inner.y + level / 2;
        let color = if end_age > 4.0 {
            MUTED
        } else if strike.velocity > 100 {
            AMBER
        } else {
            TEAL
        };
        for column in x..(end_x + 2).min(inner.right()) {
            let cell = &mut buffer[(column, y)];
            let upper = if cell.symbol() == "▀" {
                cell.fg
            } else {
                PANEL
            };
            let lower = if cell.symbol() == "▄" {
                cell.fg
            } else if cell.symbol() == "▀" {
                cell.bg
            } else {
                PANEL
            };
            let (upper, lower) = if level % 2 == 0 {
                (color, lower)
            } else {
                (upper, color)
            };
            if upper != PANEL {
                cell.set_symbol("▀")
                    .set_style(Style::default().fg(upper).bg(lower));
            } else {
                cell.set_symbol("▄")
                    .set_style(Style::default().fg(lower).bg(PANEL));
            }
        }
    }
}

pub fn snapshot(width: u16, height: u16, view: &View<'_>) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, view)).unwrap();
    terminal.backend().buffer().clone()
}

/// Export the actual Ratatui cell buffer for design review without a terminal.
pub fn preview_svg(width: u16, height: u16, view: &View<'_>) -> String {
    let buffer = snapshot(width, height, view);
    buffer_svg(&buffer)
}

pub fn training_preview_svg(
    width: u16,
    height: u16,
    view: &View<'_>,
    session: &trainer::TrainingView<'_>,
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_training(frame, view, session))
        .unwrap();
    buffer_svg(terminal.backend().buffer())
}

fn buffer_svg(buffer: &Buffer) -> String {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let mut out = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}'><rect width='100%' height='100%' fill='#0a111b'/><g font-family='DejaVu Sans Mono' font-size='14'>", width as u32 * 9, height as u32 * 19);
    let rgb = |color| match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#0a111b".into(),
    };
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            let symbol = cell
                .symbol()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            out.push_str(&format!(
                "<rect x='{}' y='{}' width='9' height='19' fill='{}'/>",
                x as u32 * 9,
                y as u32 * 19,
                rgb(cell.bg)
            ));
            if symbol == "▀" || symbol == "▄" {
                out.push_str(&format!(
                    "<rect x='{}' y='{}' width='9' height='9.5' fill='{}'/>",
                    x as u32 * 9,
                    y as f32 * 19.0 + if symbol == "▄" { 9.5 } else { 0.0 },
                    rgb(cell.fg)
                ));
            } else if symbol != " " {
                out.push_str(&format!(
                    "<text x='{}' y='{}' fill='{}' font-weight='{}'>{symbol}</text>",
                    x as u32 * 9,
                    y as u32 * 19 + 15,
                    rgb(cell.fg),
                    if cell.modifier.contains(Modifier::BOLD) {
                        "bold"
                    } else {
                        "normal"
                    }
                ));
            }
        }
    }
    out.push_str("</g></svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyboard::Event;

    #[test]
    fn training_layout_keeps_prompts_and_feedback_visible_without_answer_metadata() {
        let keyboard = Keyboard::default();
        let view = View {
            keyboard: &keyboard,
            source: "Test MIDI",
            connected: true,
            demo: false,
            silent: false,
            muted: false,
            note_names: false,
            volume: 0.6,
            now: Instant::now(),
        };
        for (width, height) in [(64, 22), (80, 24), (120, 38)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut training = trainer::TrainingView {
                recent_exercises: vec![],
                title: "CHORD RECOGNITION",
                prompt: "Listen, then play any chord of the same quality.",
                prompt_highlights: &[],
                reason: "Building fluency",
                feedback: "",
                played: &[],
                spelling: None,
                phase: trainer::Phase::Listening,
                completed: 10,
                mastered: 2,
                total: 2088,
                timed_phrase: false,
                exploring: false,
            };
            terminal
                .draw(|frame| draw_training(frame, &view, &training))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("CHORD RECOGNITION"));
            assert!(!text.contains("major"));
            assert!(text.contains("Listen"));
            training.recent_exercises = vec![trainer::ExerciseLogEntry {
                number: 10,
                title: "INTERVAL CONSTRUCTION",
                prompt: "Play C, then a major 2nd below it. Any starting octave.",
                entered: "C4 → Bb3".into(),
                correct: true,
                assisted: false,
            }];
            terminal
                .draw(|frame| draw_training(frame, &view, &training))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("RECENT EXERCISES"));
            assert!(text.contains("✓ #10"));
            assert!(text.contains("Asked: Play C"));
            assert!(text.contains("Played: C4 → Bb3"));
            training.phase = trainer::Phase::Feedback;
            training.feedback = "Correct! A major chord. Progress saved.";
            terminal
                .draw(|frame| draw_training(frame, &view, &training))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("Progress saved."));
            assert!(text.contains("Next exercise shortly"));
            training.title = "ACCOMPANIMENT";
            training.phase = trainer::Phase::Answering;
            training.timed_phrase = true;
            training.feedback = "";
            training.prompt = "In C major: 12 bars, 4/4, 60 BPM. After four count-in beats, accompany the melody with 1–7 voicings on chord degrees 1–1–1–1–4–4–1–1–5–4–1–5. Play two notes on each downbeat; hold 3½ beats and release. Any octave.";
            terminal
                .draw(|frame| draw_training(frame, &view, &training))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(
                text.contains("Any octave."),
                "Long prompt clipped at {width}x{height}"
            );
            assert!(
                text.contains("auto finish"),
                "Status clipped at {width}x{height}"
            );
            let mut small = Terminal::new(TestBackend::new(40, 10)).unwrap();
            small
                .draw(|frame| draw_training(frame, &view, &training))
                .unwrap();
            let text: String = small
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("EAR / THEORY · PAUSED"));
            assert!(!text.contains("MIDI KEYS"));
        }
    }

    #[test]
    fn keys_have_thin_seams_and_optional_names() {
        let keyboard = Keyboard::default();
        let area = Rect::new(0, 0, 45, 6);
        for note_names in [false, true] {
            let mut buffer = Buffer::empty(area);
            Keys {
                keyboard: &keyboard,
                span: 24,
                note_names,
                spelling: None,
            }
            .render(area, &mut buffer);
            let symbols: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert_eq!(symbols.contains("C4"), note_names);
            for x in 0..45 {
                assert_eq!(
                    buffer[(x, 4)].bg,
                    Color::Rgb(218, 226, 227),
                    "No dark gaps below black keys"
                );
            }
        }
    }

    #[test]
    fn held_trail_grows_past_six_seconds_then_scrolls_away_after_release() {
        let mut keyboard = Keyboard::default();
        keyboard.apply(Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        });
        let start = keyboard.strikes.back().unwrap().at;
        let columns = |keyboard: &Keyboard, elapsed| {
            let view = View {
                keyboard,
                source: "test",
                connected: true,
                demo: false,
                silent: true,
                muted: false,
                note_names: false,
                volume: 0.6,
                now: start + std::time::Duration::from_secs(elapsed),
            };
            let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
            terminal
                .draw(|frame| history(frame, frame.area(), &view))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let mut columns = std::collections::BTreeSet::new();
            for y in 0..16 {
                for x in 0..80 {
                    if matches!(buffer[(x, y)].symbol(), "▀" | "▄") {
                        columns.insert(x);
                    }
                }
            }
            columns
        };
        assert!(columns(&keyboard, 5).len() > columns(&keyboard, 2).len());
        let held = columns(&keyboard, 8);
        assert!(
            held.len() > 60,
            "Long-held notes must remain visible across the window"
        );
        keyboard.strikes.back_mut().unwrap().released_at =
            Some(start + std::time::Duration::from_secs(8));
        let released = columns(&keyboard, 9);
        assert!(
            released.last() < held.last(),
            "Released bars move away from NOW"
        );
        assert!(columns(&keyboard, 15).is_empty());
    }

    #[test]
    fn note_trail_preserves_semitone_spacing_and_both_halves_of_a_cell() {
        let mut keyboard = Keyboard::default();
        for note in [60, 62, 64, 65] {
            keyboard.apply(Event::Note {
                channel: 0,
                note,
                velocity: if note == 65 { 110 } else { 90 },
            });
        }
        let view = View {
            keyboard: &keyboard,
            source: "test",
            connected: true,
            demo: false,
            silent: true,
            muted: false,
            note_names: false,
            volume: 0.6,
            now: Instant::now(),
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        terminal
            .draw(|frame| history(frame, frame.area(), &view))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut strikes = std::collections::BTreeSet::new();
        let mut combined_colors = false;
        for y in 0..16 {
            for x in 7..78 {
                let cell = &buffer[(x, y)];
                if cell.symbol() == "▀" {
                    strikes.insert(y * 2);
                    if cell.bg != PANEL {
                        strikes.insert(y * 2 + 1);
                    }
                    combined_colors |= cell.fg == AMBER && cell.bg == TEAL;
                } else if cell.symbol() == "▄" {
                    strikes.insert(y * 2 + 1);
                }
            }
        }
        let strikes: Vec<_> = strikes.into_iter().collect();
        assert_eq!(strikes.len(), 4, "C, D, E and F need distinct half-rows");
        assert_eq!(strikes[1] - strikes[0], 1);
        assert_eq!(strikes[2] - strikes[1], 2);
        assert_eq!(strikes[3] - strikes[2], 2);
        assert!(
            combined_colors,
            "E and F must retain both velocity colors in one cell"
        );
    }

    #[test]
    fn responsive_layout_covers_compact_large_and_tiny_terminals() {
        let mut keyboard = Keyboard::default();
        for note in [60, 64, 67] {
            keyboard.apply(Event::Note {
                channel: 0,
                note,
                velocity: 100,
            });
        }
        for (width, height) in [
            (120, 38),
            (80, 24),
            (179, 38),
            (220, 38),
            (64, 22),
            (40, 10),
            (1, 1),
        ] {
            keyboard.set_visible_span(visible_span(width));
            let view = View {
                keyboard: &keyboard,
                source: "Test MIDI input",
                connected: true,
                demo: false,
                silent: false,
                muted: false,
                note_names: true,
                volume: 0.6,
                now: Instant::now(),
            };
            let buffer = snapshot(width, height, &view);
            let symbols: String = buffer.content.iter().map(|c| c.symbol()).collect();
            if width >= 64 {
                assert!(symbols.contains("HELD NOTES"));
                assert!(symbols.contains("KEYBOARD"));
                assert!(symbols.contains("C4"));
                assert!(symbols.contains("SUSTAIN"));
                assert!(buffer.content.iter().any(|c| c.bg == TEAL));
                assert_eq!(symbols.contains("NOTE TRAIL"), height >= 32);
                assert!(symbols.contains("quit"));
                let last_note = note_name((keyboard.start + visible_span(width)).min(127));
                assert!(symbols.contains(&format!("{} — {last_note}", note_name(keyboard.start))));
                if width >= 179 {
                    assert_eq!(visible_span(width), 96);
                    assert!(symbols.contains("C1 — C9"));
                }
                let last_key = (0..height).any(|y| {
                    (0..width.saturating_sub(1)).any(|x| {
                        buffer[(x, y)].symbol() == &last_note[..1]
                            && buffer[(x + 1, y)].symbol() == &last_note[1..]
                            && buffer[(x, y)].bg == Color::Rgb(218, 226, 227)
                    })
                });
                assert!(
                    last_key,
                    "The final octave must be painted, not only labeled"
                );
            } else if width >= 40 {
                assert!(symbols.contains("Resize to at least 64 × 22"));
            }
        }
    }
}

mod notation;
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

pub struct Display(
    Terminal<CrosstermBackend<io::Stdout>>,
    [notation::Graphics; 2],
);

impl Display {
    pub fn supports_score_images(&self) -> bool {
        self.1[0].enabled()
    }
    pub fn new() -> io::Result<Self> {
        // The native terminal guard already clears the alternate screen. Avoid
        // Terminal::clear(), which queries the cursor and competes with MIDI's
        // terminal input loop for a terminal response.
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(Self(
            terminal,
            [notation::Graphics::detect(0), notation::Graphics::detect(1)],
        ))
    }

    pub fn draw(&mut self, view: &View<'_>) -> io::Result<()> {
        for graphics in &mut self.1 {
            graphics.update(None)?;
        }
        self.0.draw(|frame| draw(frame, view))?;
        Ok(())
    }
    pub fn draw_training(
        &mut self,
        view: &View<'_>,
        session: &trainer::TrainingView<'_>,
    ) -> io::Result<()> {
        if !view.connected && !view.demo && session.browser.is_none() {
            return self.draw(view);
        }
        let mut images = vec![];
        let graphics = self.1[0].enabled();
        self.0.draw(|frame| {
            draw_training(frame, view, session);
            images = score_images(frame.area(), session)
                .into_iter()
                .enumerate()
                .map(|(i, (area, score))| (self.1[i].image_area(area), score))
                .collect();
            if graphics {
                for (i, (rect, _)) in images.iter().enumerate() {
                    frame.render_widget(Block::default().style(Style::default().bg(PANEL)), *rect);
                    self.1[i].placeholders(*rect, frame.buffer_mut());
                }
            }
        })?;
        for (i, graphics) in self.1.iter_mut().enumerate() {
            graphics.update(images.get(i).cloned())?;
        }
        Ok(())
    }
}

pub fn draw_training(frame: &mut Frame<'_>, view: &View<'_>, session: &trainer::TrainingView<'_>) {
    use trainer::Phase;
    if !view.connected && !view.demo && session.browser.is_none() {
        draw_connection(frame);
        return;
    }
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
    if let Some(browser) = &session.browser {
        draw_browser(frame, area, view, browser);
        return;
    }
    frame.render_widget(
        Paragraph::new(format!("ID: {}", session.skill_id)).style(Style::default().fg(MUTED)),
        Rect::new(
            area.x + 2,
            area.bottom().saturating_sub(1),
            area.width.saturating_sub(4),
            1,
        ),
    );
    if let Some(score) = session.display_score.as_ref() {
        draw_reading(frame, area, session, score);
        return;
    }
    if let Some(score) = &session.rhythm_score {
        draw_rhythm(frame, area, view, session, score);
        return;
    }
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
                text(
                    if session.preview {
                        "◈  EXERCISE PREVIEW"
                    } else {
                        "◈  EAR / THEORY"
                    },
                    TEAL,
                ),
                text(
                    if session.preview {
                        String::new()
                    } else {
                        format!("    Exercise {}", session.completed + 1)
                    },
                    TEXT,
                ),
            ]),
            Line::from(text(
                if session.preview {
                    "No progress recorded · b browse · v new variant".into()
                } else {
                    format!(
                        "{} skills established · {} in the curriculum",
                        session.mastered, session.total
                    )
                },
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
                if session.awaiting_start {
                    "Read the instructions · Enter to begin"
                } else if view.connected {
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
            Phase::Feedback if session.preview => {
                "r retry · Enter new variant · b exercise list"
            }
            Phase::Feedback if session.comparison_paused => "Result paused · r retry · Enter next",
            Phase::Feedback => "Next shortly · c pause · r retry · Enter next",
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
    let (insight_area, history_area) = if area.width >= 120 {
        let panels = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(log_area);
        (Some(panels[0]), Some(panels[1]))
    } else if session.insights_open {
        (Some(log_area), None)
    } else {
        (None, Some(log_area))
    };
    if let Some(area) = insight_area {
        let block = card(if area.width < 60 {
            "INSIGHTS · j/k browse"
        } else {
            "INSIGHTS"
        });
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines: Vec<_> = session
            .insights
            .iter()
            .cycle()
            .skip(session.insight_offset % session.insights.len().max(1))
            .take(session.insights.len())
            .map(|insight| {
                Line::from(text(
                    insight,
                    if insight.starts_with("Mastered:") {
                        TEAL
                    } else if insight.starts_with("Confusion:") {
                        AMBER
                    } else {
                        TEXT
                    },
                ))
            })
            .collect();
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }
    if let Some(area) = history_area {
        draw_exercise_log(frame, area, &session.recent_exercises);
    }
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
                text(" retry/restart  ", MUTED),
                text("⌫", TEAL),
                text(" clear  ", MUTED),
                text("h", TEAL),
                text(" hint  ", MUTED),
                text("x", TEAL),
                text(" don't know  ", MUTED),
                text("Enter", TEAL),
                text(
                    match session.phase {
                        Phase::Waiting if session.awaiting_start => " begin",
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
            Line::from(text(
                if session.preview {
                    "b exercise list · v new variant · r retry"
                } else {
                    "c pause result · i insights/history · j/k browse insights"
                },
                MUTED,
            )),
        ]),
        parts[4],
    );
}

fn draw_browser(
    frame: &mut Frame<'_>,
    area: Rect,
    view: &View<'_>,
    browser: &trainer::BrowserView<'_>,
) {
    let parts = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    }));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(
                "EXERCISE BROWSER · no training progress recorded",
                TEAL,
            )),
            Line::from(text(
                if view.connected {
                    "Select any exercise, including locked curriculum skills."
                } else {
                    "Tap a MIDI key to connect; you can browse meanwhile."
                },
                MUTED,
            )),
        ]),
        parts[0],
    );
    let category_label = format!(
        "Category: {} · ←/→ or Tab",
        browser.categories[browser.category]
    );
    frame.render_widget(
        Paragraph::new(text(category_label, TEAL)),
        Rect {
            y: parts[0].y + 2,
            height: 1,
            ..parts[0]
        },
    );
    frame.render_widget(
        Paragraph::new(format!("{}▏", browser.query)).block(card("SEARCH · title or skill ID")),
        parts[1],
    );
    let list_area = if area.width >= 100 {
        let columns = Layout::horizontal([Constraint::Length(24), Constraint::Min(20)])
            .spacing(1)
            .split(parts[2]);
        let categories: Vec<_> = browser
            .categories
            .iter()
            .enumerate()
            .map(|(index, name)| {
                Line::from(text(
                    format!(
                        "{} {name}",
                        if index == browser.category {
                            "▶"
                        } else {
                            " "
                        }
                    ),
                    if index == browser.category {
                        TEAL
                    } else {
                        MUTED
                    },
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(categories).block(card("CATEGORIES")),
            columns[0],
        );
        columns[1]
    } else {
        parts[2]
    };
    let title = format!("{} MATCHES", browser.items.len());
    let block = card(&title);
    let inner = block.inner(list_area);
    frame.render_widget(block, list_area);
    let capacity = usize::from(inner.height / 2).max(1);
    let start = browser
        .selected
        .saturating_sub(capacity / 2)
        .min(browser.items.len().saturating_sub(capacity));
    let mut rows = Vec::new();
    for (index, (title, id)) in browser.items.iter().enumerate().skip(start).take(capacity) {
        let selected = index == browser.selected;
        rows.push(Line::from(text(
            format!("{} {}", if selected { "▶" } else { " " }, title),
            if selected { TEAL } else { TEXT },
        )));
        rows.push(Line::from(text(format!("  {id}"), MUTED)));
    }
    if browser.items.is_empty() {
        rows.push(Line::from(text(
            "No matches. Backspace to edit your search.",
            AMBER,
        )));
    }
    frame.render_widget(Paragraph::new(rows), inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Type to search · ↑/↓ select · Enter preview"),
            Line::from("Backspace edit · Ctrl-U clear search · Ctrl-C quit"),
        ]),
        parts[3],
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
                entry.performance_score.map_or_else(
                    || {
                        if entry.correct {
                            "✓ ".into()
                        } else {
                            "✗ ".into()
                        }
                    },
                    |s| format!("{s}/100 "),
                ),
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
            trainer::PromptRole::Interval | trainer::PromptRole::ChordQuality => AMBER,
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

fn draw_connection(frame: &mut Frame<'_>) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text("CONNECT YOUR MIDI KEYBOARD", TEAL)),
            Line::from(""),
            Line::from("Tap and release any MIDI key to select your device."),
            Line::from("This first note connects the keyboard; it is NOT an answer."),
            Line::from("Release the key to finish connecting."),
            Line::from("q quit"),
        ])
        .wrap(Wrap { trim: true }),
        area.inner(Margin {
            horizontal: 2,
            vertical: 2,
        }),
    );
}

pub fn draw(frame: &mut Frame<'_>, view: &View<'_>) {
    if !view.connected && !view.demo {
        draw_connection(frame);
        return;
    }
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
    let mut svg = buffer_svg(terminal.backend().buffer());
    for (rect, score) in score_images(Rect::new(0, 0, width, height), session) {
        let image = score.svg().replacen(
            "<svg ",
            &format!(
                "<svg x='{}' y='{}' width='{}' height='{}' ",
                rect.x * 9,
                rect.y * 19,
                rect.width * 9,
                rect.height * 19
            ),
            1,
        );
        svg.truncate(svg.len() - 6);
        svg += &format!(
            "<rect x='{}' y='{}' width='{}' height='{}' fill='#0f1824'/>",
            rect.x * 9,
            rect.y * 19,
            rect.width * 9,
            rect.height * 19
        );
        svg += &image;
        svg += "</svg>";
    }
    svg
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
    fn rhythm_feedback_keeps_the_score_errors_and_navigation_visible() {
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
        let mut session = trainer::Session::browse().unwrap();
        for &key in b"rhythm.0.60" {
            session.browser_edit(key);
        }
        session.browser_select();
        let now = Instant::now();
        session.start(now);
        session.input(
            Event::Note {
                channel: 0,
                note: 60,
                velocity: 90,
            },
            now,
        );
        session.input(
            Event::Note {
                channel: 0,
                note: 60,
                velocity: 0,
            },
            now + std::time::Duration::from_millis(100),
        );
        session
            .tick(now + std::time::Duration::from_secs(9))
            .unwrap();
        for (width, height) in [(64, 22), (80, 24), (120, 34)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| draw_training(frame, &view, &session.view()))
                .unwrap();
            let content = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            for expected in ["SCORE", "100ms / 940ms", "MISSING", "j/k", "Enter next"] {
                assert!(
                    content.contains(expected),
                    "{width}x{height}: missing {expected}"
                );
            }
        }
        for _ in 0..8 {
            session.cycle_insights(true);
        }
        let mut terminal = Terminal::new(TestBackend::new(64, 22)).unwrap();
        terminal
            .draw(|frame| draw_training(frame, &view, &session.view()))
            .unwrap();
        let content = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(
            content.contains("8.0"),
            "last beat must be accessible on small terminals"
        );
    }
    #[test]
    fn exercise_catalog_is_searchable_and_legible_at_supported_sizes() {
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
        let mut session = trainer::Session::browse().unwrap();
        for key in b"rhythm.0.60" {
            session.browser_edit(*key);
        }
        for (width, height) in [(64, 22), (80, 24), (140, 42)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| draw_training(frame, &view, &session.view()))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            for expected in [
                "EXERCISE BROWSER",
                "1 MATCHES",
                "rhythm.0.60",
                "Enter preview",
                "Ctrl-C quit",
            ] {
                assert!(
                    text.contains(expected),
                    "Missing {expected} at {width}x{height}"
                );
            }
        }
    }

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
                skill_id: "test.exercise",
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
                browser: None,
                preview: false,
                insights_open: true,
                insight_offset: 0,
                insights: vec!["Mastered: seconds · ascending recognition".into()],
                display_score: None,
                played_score: None,
                score_page: 0,
                current_bar: None,
                score_pages: 1,
                reading_score: None,
                reading_result: None,
                rhythm_score: None,
                rhythm_report: None,
                comparison_paused: false,
                comparison_offset: 0,
                timed_phrase: false,
                awaiting_start: false,
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
            assert!(text.contains("INSIGHTS"));
            assert!(text.contains("Mastered: seconds"));
            training.insights_open = false;
            training.recent_exercises = vec![trainer::ExerciseLogEntry {
                performance_score: None,
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
            assert!(text.contains("Next shortly"));
            assert!(text.contains("r retry"));
            training.title = "ACCOMPANIMENT";
            training.phase = trainer::Phase::Answering;
            training.timed_phrase = true;
            training.feedback = "";
            training.prompt = "In C major: 12 bars, 4/4, 60 BPM. After four count-in beats, play each chord's root and seventh together on beat 1 and hold through the bar. Any octave. One chord per bar: Cmaj7 | Cmaj7 | Cmaj7 | Cmaj7 | Fmaj7 | Fmaj7 | Cmaj7 | Cmaj7 | G7 | Fmaj7 | Cmaj7 | G7.";
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

fn rhythm_areas(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(7),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    }))
}
fn draw_rhythm(
    frame: &mut Frame<'_>,
    area: Rect,
    view: &View<'_>,
    session: &trainer::TrainingView<'_>,
    score: &trainer::RhythmScore,
) {
    let parts = rhythm_areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(
                format!("◈ {} · {} BPM", session.title, score.bpm),
                TEAL,
            )),
            Line::from(text(
                if view.connected {
                    view.source
                } else {
                    "Waiting for your keyboard"
                },
                MUTED,
            )),
        ]),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(session.prompt)
            .style(Style::default().fg(TEXT))
            .wrap(Wrap { trim: true }),
        parts[1],
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .title_style(Style::default().fg(TEAL))
        .title(if score.quarter_notes {
            " SCORE · 4/4 · each ♩ = one beat "
        } else {
            " RHYTHM · expected attacks in beats "
        });
    let inner = block.inner(parts[2]);
    frame.render_widget(block, parts[2]);
    let beat = 60000.0 / f64::from(score.bpm);
    let fallback = if score.quarter_notes {
        vec![
            Line::from(""),
            Line::from("  4/4    ♩    ♩    ♩    ♩   │   ♩    ♩    ♩    ♩   │"),
            Line::from("         1    2    3    4       1    2    3    4"),
        ]
    } else {
        vec![Line::from(
            score
                .notes
                .iter()
                .map(|n| format!("{:.1}", 1.0 + n.0 as f64 / beat))
                .collect::<Vec<_>>()
                .join("  "),
        )]
    };
    frame.render_widget(
        Paragraph::new(fallback).style(Style::default().fg(TEXT).bg(PANEL)),
        inner,
    );
    let mut lines = Vec::new();
    if let Some(report) = session.rhythm_report {
        lines.push(Line::from(text(session.feedback, AMBER)));
        lines.push(Line::from(text(
            format!(
                "Attack / hold tolerance ±{}ms · hold uses key release",
                score.tolerance_ms
            ),
            MUTED,
        )));
        lines.push(Line::from(text(
            "Beat   Played key / attack     Held: played / expected",
            TEAL,
        )));
        let available = parts[3].height.saturating_sub(lines.len() as u16) as usize;
        let offset = session
            .comparison_offset
            .min(report.rows.len().saturating_sub(available.max(1)));
        for row in report.rows.iter().skip(offset).take(available) {
            let beat_label = row
                .expected_ms
                .map(|t| format!("{:.1}", 1.0 + t as f64 / beat))
                .unwrap_or_else(|| "—".into());
            let key = row.note.map(note_name).unwrap_or_else(|| "—".into());
            let held = if row.actual_ms.is_none() {
                "—".into()
            } else {
                row.actual_hold_ms
                    .map(|h| format!("{h}ms"))
                    .unwrap_or_else(|| "no release".into())
            };
            let expected = row
                .expected_hold_ms
                .map(|h| format!("{h}ms"))
                .unwrap_or_else(|| "—".into());
            let ok = row.correct(score.tolerance_ms);
            lines.push(Line::from(text(
                format!(
                    "{beat_label:<5} {key:<4} {:<17} {held} / {expected} {}",
                    row.timing_label(),
                    if ok { "✓" } else { "✗" }
                ),
                if ok { TEAL } else { AMBER },
            )));
        }
    } else {
        lines.push(Line::from(text(
            match session.phase {
                trainer::Phase::Waiting if session.awaiting_start => {
                    "Read the score · Enter starts the count-in"
                }
                trainer::Phase::Waiting => "Release keys to begin.",
                trainer::Phase::Listening => "Listen… r to repeat",
                _ if score.quarter_notes => "Join on any drum beat · answer finishes automatically",
                _ => "Your turn · answer finishes automatically",
            },
            TEAL,
        )));
        lines.push(Line::from(text(
            format!(
                "{} / {} notes played",
                session.played.len(),
                score.notes.len()
            ),
            TEXT,
        )));
        if score.quarter_notes {
            lines.push(Line::from(text(
                "One-line percussion staff: pitches are up to you.",
                MUTED,
            )));
            lines.push(Line::from(text(
                "Hold each quarter note until just before the next beat.",
                MUTED,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), parts[3]);
    let status = if session.rhythm_report.is_some() {
        if session.preview || session.comparison_paused {
            "Paused · r retry · Enter next · j/k scroll"
        } else {
            "c pause · r retry · Enter next · j/k scroll"
        }
    } else {
        "r restart · Backspace clear · x give up · q quit"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(status, TEAL)),
            Line::from(text(
                if session.preview {
                    "b exercise list · v new variant · q quit"
                } else {
                    "q quit · +/- volume · m mute"
                },
                MUTED,
            )),
        ]),
        parts[4],
    );
}

fn score_panels(area: Rect, session: &trainer::TrainingView<'_>) -> Vec<Rect> {
    let rect = reading_areas(area, session)[2];
    if session.played_score.is_some() {
        // Equal pixel scale requires equal terminal rectangles. With an odd
        // available height, leave the spare row unused instead of stretching
        // one score to an extra row.
        let height = rect.height / 2;
        vec![
            Rect::new(rect.x, rect.y, rect.width, height),
            Rect::new(rect.x, rect.y + height, rect.width, height),
        ]
    } else {
        vec![rect]
    }
}
fn notation_area(panel: Rect, _score: &trainer::WrittenScore) -> Rect {
    panel.inner(Margin {
        horizontal: 1,
        vertical: 1,
    })
}

fn score_images(
    area: Rect,
    session: &trainer::TrainingView<'_>,
) -> Vec<(Rect, notation::ImageScore)> {
    if !training_fits(area.width, area.height) || session.browser.is_some() {
        return vec![];
    }
    if let Some(score) = &session.display_score {
        let panels = score_panels(area, session);
        let ticks = score
            .ticks
            .max(session.played_score.as_ref().map_or(0, |s| s.ticks));
        let comparison = session.played_score.is_some();
        let hands: Vec<_> = [trainer::Hand::Right, trainer::Hand::Left]
            .into_iter()
            .filter(|h| {
                score.hands.contains(h)
                    || session
                        .played_score
                        .as_ref()
                        .is_some_and(|s| s.hands.contains(h))
            })
            .collect();
        [Some(score), session.played_score.as_ref()]
            .into_iter()
            .flatten()
            .zip(panels)
            .filter_map(|(score, panel)| {
                if score.symbols_only {
                    return None;
                }
                let area = notation_area(panel, score);
                if area.is_empty() {
                    return None;
                }
                let mut score = score.clone();
                score.ticks = ticks;
                if comparison {
                    score.hands = hands.clone();
                }
                Some((area, notation::ImageScore::Reading(score)))
            })
            .collect()
    } else {
        session
            .rhythm_score
            .as_ref()
            .filter(|s| s.quarter_notes)
            .map(|score| {
                (
                    rhythm_areas(area)[2].inner(Margin {
                        horizontal: 1,
                        vertical: 1,
                    }),
                    notation::ImageScore::Pulse(score.notes.len()),
                )
            })
            .into_iter()
            .collect()
    }
}
fn reading_areas(area: Rect, session: &trainer::TrainingView<'_>) -> std::rc::Rc<[Rect]> {
    let feedback = session.phase == trainer::Phase::Feedback;
    let height = |score: &trainer::WrittenScore| {
        if score.symbols_only {
            3
        } else {
            (if score.hands.len() > 1 && !score.lead {
                12
            } else {
                9
            }) + u16::from(!score.chords.is_empty())
        }
    };
    let desired = session.display_score.as_ref().map_or(9, height)
        + session.played_score.as_ref().map_or(0, height);
    Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(if feedback { 1 } else { 3 }),
        Constraint::Length(desired.min(area.height.saturating_sub(13).max(6))),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    }))
}
fn draw_reading(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &trainer::TrainingView<'_>,
    score: &trainer::WrittenScore,
) {
    let parts = reading_areas(area, session);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(
                format!(
                    "◈ {} · {}",
                    session.title,
                    score
                        .bpm
                        .map(|b| format!("{b} BPM"))
                        .unwrap_or_else(|| "UNTIMED".into())
                ),
                TEAL,
            )),
            Line::from(text(
                if score.key_fifths > 0 {
                    format!("Key signature: {} sharp(s)", score.key_fifths)
                } else if score.key_fifths < 0 {
                    format!("Key signature: {} flat(s)", -score.key_fifths)
                } else {
                    "Treble / bass notation".into()
                },
                MUTED,
            )),
        ]),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(session.prompt)
            .style(Style::default().fg(TEXT))
            .wrap(Wrap { trim: true }),
        parts[1],
    );
    let panels = score_panels(area, session);
    draw_score_panel(
        frame,
        panels[0],
        session,
        score,
        if session.played_score.is_some() {
            " EXPECTED "
        } else if score.lead {
            " LEAD SHEET "
        } else {
            " SCORE "
        },
    );
    if let Some(played) = &session.played_score {
        draw_score_panel(
            frame,
            panels[1],
            session,
            played,
            " PLAYED · red = pitch / timing / hold error ",
        );
    }
    let mut lines = vec![];
    if session.phase == trainer::Phase::Feedback && session.reading_result.is_none() {
        lines.push(Line::from(text(session.feedback, AMBER)));
        lines.push(Line::from(text(&score.caption, MUTED)));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), parts[3]);
    } else if let Some(result) = session.reading_result {
        lines.push(Line::from(text(session.feedback, AMBER)));
        lines.push(Line::from(text(
            result
                .components()
                .iter()
                .map(|(part, ok)| format!("{part} {}", if *ok { "✓" } else { "✗" }))
                .collect::<Vec<_>>()
                .join("  "),
            TEAL,
        )));
        for detail in &result.details {
            lines.push(Line::from(text(detail, TEXT)));
        }
        let rows: usize = lines
            .iter()
            .map(|line| wrapped_rows(&line.to_string(), parts[3].width) as usize)
            .sum();
        let offset = session
            .comparison_offset
            .min(rows.saturating_sub(parts[3].height as usize));
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .scroll((offset as u16, 0)),
            parts[3],
        );
    } else {
        lines.push(Line::from(text(
            match session.phase {
                trainer::Phase::Waiting if session.awaiting_start => {
                    "Read the score · Enter starts the count-in"
                }
                trainer::Phase::Waiting => "Release keys to begin.",
                trainer::Phase::Listening => "Count-in / playback · wait before playing your part.",
                _ => "Play now · follow the score; p/n changes pages.",
            },
            TEAL,
        )));
        lines.push(Line::from(text(
            format!(
                "{} notes entered · {}",
                session.played.len(),
                if session.reading_score.is_some() {
                    "MIDI checks the written parts, not physical hands"
                } else {
                    &score.caption
                }
            ),
            MUTED,
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), parts[3]);
    }
    let page_label = format!(
        "Page {}/{} · p previous / n next · {}",
        session.score_page + 1,
        session.score_pages,
        if session.preview {
            "b list · q quit"
        } else {
            "q quit"
        }
    );
    let actions = if session.awaiting_start {
        "Enter start count-in · p/n score pages · q quit"
    } else if session.phase == trainer::Phase::Feedback {
        if session.reading_score.is_some() {
            "r retry · e example · c pause · j/k details · Enter next"
        } else {
            "r retry · c pause · p/n score pages · Enter next"
        }
    } else {
        "r restart · x give up · auto submit"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text(actions, TEAL)),
            Line::from(text(page_label, MUTED)),
        ]),
        parts[4],
    );
}

fn draw_score_panel(
    frame: &mut Frame<'_>,
    panel: Rect,
    session: &trainer::TrainingView<'_>,
    score: &trainer::WrittenScore,
    title: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .title_style(Style::default().fg(TEAL))
        .title(title);
    let inner = notation_area(panel, score);
    frame.render_widget(block, panel);
    if score.symbols_only && !score.chords.is_empty() {
        let row = Rect::new(panel.x + 2, panel.y + 1, panel.width.saturating_sub(4), 1);
        let count = score.chords.len() as u16;
        for (i, chord) in score.chords.iter().enumerate() {
            let x = row.x + row.width * i as u16 / count;
            let end = row.x + row.width * (i as u16 + 1) / count;
            let bar =
                session.score_page * score.bars_per_page as usize + chord.tick as usize / 8 + 1;
            let beat = chord.tick as f32 / 2. % 4. + 1.;
            let active =
                session.current_bar == Some(bar - 1) && session.phase == trainer::Phase::Answering;
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    text(
                        if score.bars_per_page > 2 {
                            format!("{}{bar}: ", if active { "▶" } else { "" })
                        } else {
                            format!("Bar {bar} · beat {beat}: ")
                        },
                        MUTED,
                    ),
                    Span::styled(
                        &chord.text,
                        Style::default()
                            .fg(if active { TEAL } else { AMBER })
                            .add_modifier(Modifier::BOLD),
                    ),
                ])),
                Rect::new(x, row.y, end - x, 1),
            );
        }
    }
    if score.symbols_only {
        return;
    }
    // Unsupported terminals pause reading; never replace staff reading with note names.
    // Ghostty/Kitty replace this rectangle with the engraved image.
    let mut fallback = vec![Line::from(text(
        if std::env::var_os("TMUX").is_some() {
            "Outer Ghostty/Kitty not detected in tmux. Enable allow-passthrough and start tmux from Ghostty/Kitty."
        } else if std::env::var_os("STY").is_some() {
            "Images disabled inside GNU Screen. Run directly in Ghostty/Kitty."
        } else if session.reading_score.is_some() {
            "PAUSED: score images unavailable. Use Ghostty/Kitty directly."
        } else {
            "Score images unavailable: Ghostty/Kitty was not detected. Text pattern below."
        },
        MUTED,
    ))];
    if session.reading_score.is_none() {
        for hand in &score.hands {
            fallback.push(Line::from(text(
                format!(
                    "{}: {}",
                    hand.label(),
                    score
                        .events
                        .iter()
                        .filter(|e| e.hand == *hand)
                        .map(|e| format!(
                            "{} [{} beats]",
                            if e.notes().is_empty() {
                                "rest".into()
                            } else {
                                e.notes()
                                    .iter()
                                    .map(|n| score.spelling().note_name(*n))
                                    .collect::<Vec<_>>()
                                    .join("+")
                            },
                            e.duration as f32 / 2.
                        ))
                        .collect::<Vec<_>>()
                        .join(" → ")
                ),
                TEXT,
            )));
        }
    } else if !score.symbols_only {
        fallback.push(Line::from(text(
            "Use a graphics-capable terminal to read the staff.",
            AMBER,
        )));
    }
    frame.render_widget(Paragraph::new(fallback).wrap(Wrap { trim: true }), inner);
}

#[cfg(test)]
mod score_layout_tests {
    use super::*;
    #[test]
    fn comparison_images_have_equal_sizes_even_at_odd_terminal_heights() {
        let session = trainer::Session::preview("harmony.7.major.accompany.12.true").unwrap();
        let mut view = session.view();
        view.phase = trainer::Phase::Feedback;
        view.played_score = view.display_score.clone();
        for height in 30..60 {
            let images = score_images(Rect::new(0, 0, 120, height), &view);
            assert_eq!(images.len(), 2);
            assert_eq!(images[0].0.width, images[1].0.width);
            assert_eq!(images[0].0.height, images[1].0.height);
        }
    }
    #[test]
    fn comparisons_have_compact_images_and_real_terminal_labels() {
        let session = trainer::Session::preview("guided.scale.0.major.up").unwrap();
        let mut view = session.view();
        view.phase = trainer::Phase::Feedback;
        view.played_score = view.display_score.clone();
        let area = Rect::new(0, 0, 120, 50);
        let images = score_images(area, &view);
        assert_eq!(images.len(), 2);
        assert!(images[0].0.bottom() < images[1].0.top());
        assert!(images.iter().all(|(rect, _)| rect.height <= 7));
        let mut terminal = Terminal::new(TestBackend::new(120, 50)).unwrap();
        terminal
            .draw(|frame| draw_reading(frame, area, &view, view.display_score.as_ref().unwrap()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("EXPECTED"));
        assert!(text.contains("PLAYED"));
    }
    #[test]
    fn chord_symbols_share_the_score_image_instead_of_an_independent_text_row() {
        let session = trainer::Session::preview("reading.lead.2").unwrap();
        let view = session.view();
        let area = Rect::new(0, 0, 120, 38);
        let image = score_images(area, &view).remove(0).0;
        let panel = score_panels(area, &view)[0];
        assert_eq!(image.y, panel.y + 1);
        let mut terminal = Terminal::new(TestBackend::new(120, 38)).unwrap();
        terminal
            .draw(|frame| draw_reading(frame, area, &view, view.display_score.as_ref().unwrap()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!text.contains("Bar 1 · beat 1:"));
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    #[test]
    fn connection_screen_explains_consumed_note_before_showing_exercise() {
        let keyboard = Keyboard::default();
        let view = View {
            keyboard: &keyboard,
            source: "",
            connected: false,
            demo: false,
            silent: true,
            muted: false,
            note_names: false,
            volume: 1.0,
            now: Instant::now(),
        };
        let session = trainer::Session::preview("reading.together.3").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw_training(frame, &view, &session.view()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("CONNECT YOUR MIDI KEYBOARD"));
        assert!(text.contains("NOT an answer"));
        assert!(!text.contains("Enter starts"));
    }
}

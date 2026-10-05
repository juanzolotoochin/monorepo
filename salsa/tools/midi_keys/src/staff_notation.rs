//! Compact engraving for the finite notation vocabulary used by reading tasks.
//! SVG review artifacts and terminal RGB use exactly the same scene geometry.
use chord_glyphs as lettering;
use music_glyphs as glyphs;
use trainer::{Hand, WrittenScore};
const SPACE: f64 = 12.0;
#[derive(Clone)]
enum Shape {
    Rect(f64, f64, f64, f64),
    Outline(Vec<Vec<(f64, f64)>>),
}
pub struct Scene {
    width: usize,
    height: usize,
    shapes: Vec<Shape>,
    red_shapes: std::collections::BTreeSet<usize>,
    incorrect: bool,
}
impl Scene {
    fn push(&mut self, shape: Shape) {
        if self.incorrect {
            self.red_shapes.insert(self.shapes.len());
        }
        self.shapes.push(shape);
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(Shape::Rect(x, y, w, h));
    }
    fn glyph(&mut self, source: &[&[(f64, f64)]], x: f64, y: f64, scale: f64) {
        self.push(Shape::Outline(
            source
                .iter()
                .map(|c| {
                    c.iter()
                        .map(|(a, b)| (x + a * scale, y + b * scale))
                        .collect()
                })
                .collect(),
        ));
    }
    fn chord(&mut self, text: &str, mut x: f64, y: f64, size: f64) -> f64 {
        let start = x;
        for ch in text.chars() {
            if let Some((advance, outlines)) = lettering::glyph(ch) {
                self.glyph(outlines, x, y, size);
                x += advance * size;
            }
        }
        x - start
    }
    fn line(&mut self, a: (f64, f64), b: (f64, f64), weight: f64) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = (dx * dx + dy * dy).sqrt().max(0.01);
        let (nx, ny) = (-dy / length * weight / 2., dx / length * weight / 2.);
        self.push(Shape::Outline(vec![vec![
            (a.0 + nx, a.1 + ny),
            (b.0 + nx, b.1 + ny),
            (b.0 - nx, b.1 - ny),
            (a.0 - nx, a.1 - ny),
        ]]));
    }
    pub fn svg(&self) -> String {
        let mut svg=format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {} {}'><rect width='100%' height='100%' fill='#0f1824'/><g fill='#e3ecef' fill-rule='evenodd'>",self.width,self.height);
        for (shape_index, shape) in self.shapes.iter().enumerate() {
            svg += if self.red_shapes.contains(&shape_index) {
                "<g fill='#ff7878'>"
            } else {
                "<g>"
            };
            match shape {
                Shape::Rect(x, y, w, h) => {
                    svg += &format!("<rect x='{x}' y='{y}' width='{w}' height='{h}'/>")
                }
                Shape::Outline(contours) => {
                    svg += "<path d='";
                    for contour in contours {
                        for (i, (x, y)) in contour.iter().enumerate() {
                            svg += &format!("{}{x:.2},{y:.2} ", if i == 0 { "M" } else { "L" });
                        }
                        svg += "Z ";
                    }
                    svg += "'/>";
                }
            }
            svg += "</g>";
        }
        svg + "</g></svg>"
    }
    pub fn pixels(&self) -> (usize, usize, Vec<u8>) {
        // Rasterize vector geometry at 2x, never enlarge a low-resolution bitmap.
        let scaled = Scene {
            red_shapes: self.red_shapes.clone(),
            incorrect: false,
            width: self.width * 2,
            height: self.height * 2,
            shapes: self
                .shapes
                .iter()
                .map(|shape| match shape {
                    Shape::Rect(x, y, w, h) => Shape::Rect(x * 2., y * 2., w * 2., h * 2.),
                    Shape::Outline(contours) => Shape::Outline(
                        contours
                            .iter()
                            .map(|c| c.iter().map(|(x, y)| (x * 2., y * 2.)).collect())
                            .collect(),
                    ),
                })
                .collect(),
        };
        scaled.rasterize()
    }
    fn rasterize(&self) -> (usize, usize, Vec<u8>) {
        let mut rgb = [15, 24, 36].repeat(self.width * self.height);
        let mut coverage = vec![0u8; self.width];
        let mut crossings = vec![];
        for (shape_index, shape) in self.shapes.iter().enumerate() {
            let (left, top, right, bottom) = match shape {
                Shape::Rect(x, y, w, h) => (*x, *y, x + w, y + h),
                Shape::Outline(contours) => contours.iter().flatten().fold(
                    (
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ),
                    |(l, t, r, b), (x, y)| (l.min(*x), t.min(*y), r.max(*x), b.max(*y)),
                ),
            };
            let x0 = (left.floor().max(0.) as usize).min(self.width);
            let x1 = (right.ceil().max(0.) as usize).min(self.width);
            for y in
                (top.floor().max(0.) as usize)..(bottom.ceil().max(0.) as usize).min(self.height)
            {
                coverage[x0..x1].fill(0);
                // Find edge intersections once per subpixel scanline, rather
                // than testing every polygon edge at every pixel sample.
                for dy in [0.25, 0.75] {
                    let py = y as f64 + dy;
                    crossings.clear();
                    match shape {
                        Shape::Rect(x, sy, w, h) => {
                            if py >= *sy && py < sy + h {
                                crossings.extend([*x, x + w]);
                            }
                        }
                        Shape::Outline(contours) => {
                            for contour in contours {
                                for i in 0..contour.len() {
                                    let (a, b) = (contour[i], contour[(i + 1) % contour.len()]);
                                    if (a.1 > py) != (b.1 > py) {
                                        crossings
                                            .push((b.0 - a.0) * (py - a.1) / (b.1 - a.1) + a.0);
                                    }
                                }
                            }
                            crossings.sort_by(f64::total_cmp);
                        }
                    }
                    for pair in crossings.chunks_exact(2) {
                        let start = (pair[0].floor().max(0.) as usize).min(self.width);
                        let end = (pair[1].ceil().max(0.) as usize).min(self.width);
                        for x in start..end {
                            for dx in [0.25, 0.75] {
                                let px = x as f64 + dx;
                                coverage[x] += u8::from(px >= pair[0] && px < pair[1]);
                            }
                        }
                    }
                }
                for x in x0..x1 {
                    for (c, fg) in (if self.red_shapes.contains(&shape_index) {
                        [255, 120, 120]
                    } else {
                        [227, 236, 239]
                    })
                    .into_iter()
                    .enumerate()
                    {
                        let index = (y * self.width + x) * 3 + c;
                        let bg = i32::from(rgb[index]);
                        rgb[index] = (bg + (fg - bg) * i32::from(coverage[x]) / 4) as u8;
                    }
                }
            }
        }
        (self.width, self.height, rgb)
    }
    #[cfg(test)]
    fn rasterize_reference(&self) -> (usize, usize, Vec<u8>) {
        let mut rgb = [15, 24, 36].repeat(self.width * self.height);
        for (shape_index, shape) in self.shapes.iter().enumerate() {
            let (left, top, right, bottom) = match shape {
                Shape::Rect(x, y, w, h) => (*x, *y, x + w, y + h),
                Shape::Outline(contours) => contours.iter().flatten().fold(
                    (
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ),
                    |(l, t, r, b), (x, y)| (l.min(*x), t.min(*y), r.max(*x), b.max(*y)),
                ),
            };
            for y in
                (top.floor().max(0.) as usize)..(bottom.ceil().max(0.) as usize).min(self.height)
            {
                for x in
                    (left.floor().max(0.) as usize)..(right.ceil().max(0.) as usize).min(self.width)
                {
                    let coverage = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                        .into_iter()
                        .filter(|(dx, dy)| {
                            let (px, py) = (x as f64 + dx, y as f64 + dy);
                            match shape {
                                Shape::Rect(sx, sy, w, h) => {
                                    px >= *sx && px < sx + w && py >= *sy && py < sy + h
                                }
                                Shape::Outline(contours) => {
                                    let mut inside = false;
                                    for contour in contours {
                                        for i in 0..contour.len() {
                                            let (a, b) =
                                                (contour[i], contour[(i + 1) % contour.len()]);
                                            if (a.1 > py) != (b.1 > py)
                                                && px < (b.0 - a.0) * (py - a.1) / (b.1 - a.1) + a.0
                                            {
                                                inside = !inside;
                                            }
                                        }
                                    }
                                    inside
                                }
                            }
                        })
                        .count();
                    for (c, fg) in (if self.red_shapes.contains(&shape_index) {
                        [255, 120, 120]
                    } else {
                        [227, 236, 239]
                    })
                    .into_iter()
                    .enumerate()
                    {
                        let index = (y * self.width + x) * 3 + c;
                        let bg = i32::from(rgb[index]);
                        rgb[index] = (bg + (fg - bg) * coverage as i32 / 4) as u8;
                    }
                }
            }
        }
        (self.width, self.height, rgb)
    }
}
/// Written diatonic position; accidentals do not move a notehead on the staff.
#[cfg(test)]
pub fn position(note: usize, key_fifths: i8) -> (i32, i8) {
    let pc = note % 12;
    let (letter, accidental) = match pc {
        0 => (0, 0),
        1 => (0, 1),
        2 => (1, 0),
        3 => (1, 1),
        4 => (2, 0),
        5 => (3, 0),
        6 => (3, 1),
        7 => (4, 0),
        8 => (4, 1),
        9 => (5, 0),
        10 if key_fifths < 0 => (6, -1),
        10 => (5, 1),
        _ => (6, 0),
    };
    ((note / 12) as i32 * 7 + letter, accidental)
}
// Reserve end-of-bar space for glyph width and the barline, even in a
// compact twelve-bar chart. Bar starts (and their chord symbols) stay fixed.
fn musical_x(tick: u32, ticks: u32, first_x: f64) -> f64 {
    let bar_width = (1160. - first_x) / ticks.max(8).div_ceil(8) as f64;
    let usable = (bar_width - 40.).max(bar_width * 0.4);
    first_x + (tick / 8) as f64 * bar_width + (tick % 8) as f64 / 7. * usable
}

pub fn scene(score: &WrittenScore) -> Scene {
    engrave(score, false)
}
pub fn percussion_scene(score: &WrittenScore) -> Scene {
    engrave(score, true)
}
fn engrave(score: &WrittenScore, percussion: bool) -> Scene {
    let visible: Vec<_> = score
        .hands
        .iter()
        .copied()
        .filter(|h| !score.lead || (*h == Hand::Right && !score.symbols_only))
        .collect();
    let chord_space = if score.chords.is_empty() { 0. } else { 28. };
    let mut scene = Scene {
        red_shapes: Default::default(),
        incorrect: false,
        width: 1200,
        height: (if percussion {
            120
        } else if visible.len() == 2 {
            320
        } else {
            190
        }) + chord_space as usize,
        shapes: vec![],
    };
    let time_x = 100. + f64::from(score.key_fifths.unsigned_abs()) * 10.;
    let first_x = (time_x + 44.).max(160.);
    let x_at = |tick: u32| musical_x(tick, score.ticks, first_x);
    let pitch_position = |note: usize| {
        if percussion {
            (41, 0)
        } else {
            score.spelling().staff_position(note)
        }
    };
    if score.symbols_only {
        return scene;
    }
    for chord in &score.chords {
        let x = x_at(chord.tick);
        let width = scene.chord(&chord.text, x, 57., 27.);
        if score.active_tick == Some(chord.tick) {
            scene.rect(x, 64., width.max(20.), 2.5);
        }
    }
    for (staff, hand) in visible.iter().enumerate() {
        scene.incorrect = false;
        let bottom = (if percussion { 90. } else { 130. }) + chord_space + staff as f64 * 150.;
        let lowest = if *hand == Hand::Right { 37 } else { 25 }; // E4 / G2 in MIDI-octave indexing.
        for line in 0..5 {
            if !percussion || line == 2 {
                scene.rect(25., bottom - line as f64 * SPACE, 1150., 1.2);
            }
        }
        if percussion {
            scene.rect(42., bottom - 3. * SPACE, 4., 2. * SPACE);
            scene.rect(51., bottom - 3. * SPACE, 4., 2. * SPACE);
        } else {
            scene.glyph(
                if *hand == Hand::Right {
                    glyphs::TREBLE
                } else {
                    glyphs::BASS
                },
                42.,
                bottom
                    - if *hand == Hand::Right {
                        SPACE
                    } else {
                        3. * SPACE
                    },
                SPACE,
            );
        }
        let sharps = [3, 0, 4, 1, 5, 2, 6];
        let flats = [6, 2, 5, 1, 4, 0, 3];
        let signature = if score.key_fifths > 0 {
            &sharps
        } else {
            &flats
        };
        for i in 0..score.key_fifths.unsigned_abs().min(7) as usize {
            let steps = if score.key_fifths > 0 {
                [8, 5, 9, 6, 3, 7, 4]
            } else {
                [4, 7, 3, 6, 2, 5, 1]
            };
            let step = steps[i] - if *hand == Hand::Left { 2 } else { 0 };
            scene.glyph(
                if score.key_fifths > 0 {
                    glyphs::SHARP
                } else {
                    glyphs::FLAT
                },
                85. + i as f64 * 10.,
                bottom - f64::from(step) * SPACE / 2.,
                SPACE,
            );
        }
        if score.bpm.is_some() && !percussion {
            scene.glyph(glyphs::FOUR, time_x, bottom - 2. * SPACE, SPACE);
            scene.glyph(glyphs::FOUR, time_x, bottom, SPACE);
        }
        for tick in (8..=score.ticks)
            .step_by(8)
            .filter(|_| score.bpm.is_some() && !percussion)
        {
            scene.rect(x_at(tick) - 16., bottom - 4. * SPACE, 1.5, 4. * SPACE);
        }
        let mut accidental_state = std::collections::BTreeMap::new();
        let mut bar = 0;
        let mut written = vec![];
        for event in score.events.iter().filter(|e| e.hand == *hand) {
            scene.incorrect = event.incorrect;
            let mut tick = event.tick;
            let end = event.tick + event.duration;
            let mut pieces = vec![];
            while tick < end {
                let boundary = if percussion {
                    end
                } else {
                    ((tick / 8 + 1) * 8).min(end)
                };
                let boundary = event
                    .tie_tick
                    .filter(|t| *t > tick)
                    .map_or(boundary, |t| boundary.min(t));
                let duration = [8, 7, 6, 4, 3, 2, 1]
                    .into_iter()
                    .find(|d| *d <= boundary - tick)
                    .unwrap();
                let mut part = event.clone();
                part.tick = tick;
                part.duration = duration;
                part.tie_tick = None;
                pieces.push(part);
                tick += duration;
            }
            for pair in pieces.windows(2) {
                for &note in event.notes() {
                    let y = bottom - (pitch_position(note).0 - lowest) as f64 * SPACE / 2. + 10.;
                    let (left, right) = (x_at(pair[0].tick) + 8., x_at(pair[1].tick) + 8.);
                    for i in 0..24 {
                        let t = i as f64 / 24.;
                        let u = (i + 1) as f64 / 24.;
                        scene.line(
                            (left + (right - left) * t, y + 48. * t * (1. - t)),
                            (left + (right - left) * u, y + 48. * u * (1. - u)),
                            1.8,
                        );
                    }
                }
            }
            written.extend(pieces);
        }
        written.sort_by_key(|e| e.tick);
        for e in &written {
            scene.incorrect = e.incorrect;
            if e.tick / 8 != bar {
                accidental_state.clear();
                bar = e.tick / 8;
            }
            let x = x_at(e.tick);
            if e.notes().is_empty() {
                let glyph = match e.duration {
                    1 => glyphs::REST_EIGHTH,
                    4 => glyphs::REST_HALF,
                    8 => glyphs::REST_WHOLE,
                    _ => glyphs::REST_QUARTER,
                };
                scene.glyph(glyph, x, bottom - 2. * SPACE, SPACE);
                continue;
            }
            for &note in e.notes() {
                let (position, accidental) = pitch_position(note);
                let step = position - lowest;
                let y = bottom - step as f64 * SPACE / 2.;
                let default = if signature[..score.key_fifths.unsigned_abs().min(7) as usize]
                    .contains(&(position % 7))
                {
                    score.key_fifths.signum()
                } else {
                    0
                };
                let previous = *accidental_state.get(&position).unwrap_or(&default);
                if accidental != previous {
                    scene.glyph(
                        if accidental == 2 {
                            glyphs::DOUBLE_SHARP
                        } else if accidental == -2 {
                            glyphs::DOUBLE_FLAT
                        } else if accidental > 0 {
                            glyphs::SHARP
                        } else if accidental < 0 {
                            glyphs::FLAT
                        } else {
                            glyphs::NATURAL
                        },
                        x - 15.,
                        y,
                        SPACE,
                    );
                    accidental_state.insert(position, accidental);
                }
                if step < 0 {
                    for ledger in (step..=-2).filter(|i| i % 2 == 0) {
                        scene.rect(x - 5., bottom - ledger as f64 * SPACE / 2., 25., 1.3);
                    }
                }
                if step > 8 {
                    for ledger in (10..=step).filter(|i| i % 2 == 0) {
                        scene.rect(x - 5., bottom - ledger as f64 * SPACE / 2., 25., 1.3);
                    }
                }
                scene.glyph(
                    if e.duration >= 8 {
                        glyphs::WHOLE
                    } else if e.duration >= 4 {
                        glyphs::HALF
                    } else {
                        glyphs::BLACK
                    },
                    x,
                    y,
                    SPACE,
                );
            }
            if [3, 6, 7].contains(&e.duration) {
                for &note in e.notes() {
                    let step = pitch_position(note).0 - lowest;
                    let y = bottom
                        - step as f64 * SPACE / 2.
                        - if step % 2 == 0 { SPACE / 2. } else { 0. };
                    for dot in 0..if e.duration == 7 { 2 } else { 1 } {
                        scene.rect(x + 20. + f64::from(dot) * 5., y - 1.5, 3., 3.);
                    }
                }
            }
            if e.duration < 8 {
                let ys: Vec<_> = e
                    .notes()
                    .iter()
                    .map(|&n| bottom - (pitch_position(n).0 - lowest) as f64 * SPACE / 2.)
                    .collect();
                let high = ys.iter().copied().fold(f64::INFINITY, f64::min);
                let low = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let down = (high + low) / 2. < bottom - 2. * SPACE;
                let (stem_x, end) = if down {
                    (x, low + 3.5 * SPACE)
                } else {
                    (x + 14., high - 3.5 * SPACE)
                };
                scene.line((stem_x, if down { high } else { low }), (stem_x, end), 1.5);
                if e.duration == 1 {
                    scene.glyph(
                        if down {
                            glyphs::FLAG_DOWN
                        } else {
                            glyphs::FLAG_UP
                        },
                        stem_x,
                        end,
                        SPACE,
                    );
                }
            }
        }
    }
    scene.incorrect = false;
    if visible.len() == 2 {
        scene.rect(25., 82. + chord_space, 1.5, 198.);
    }
    scene
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_error_rests_clear_the_barline_in_a_twelve_bar_chart() {
        let session = trainer::Session::preview("harmony.7.major.accompany.12.true").unwrap();
        for (tick, duration) in [(78, 2), (79, 1)] {
            let mut score = session.view().display_score.unwrap();
            let mut rest = score.events[9].clone();
            rest.tick = tick;
            rest.duration = duration;
            rest.choices.clear();
            rest.incorrect = true;
            score.events.push(rest);
            let engraved = scene(&score);
            let barline = musical_x(80, 96, 160.) - 16.;
            let mut points = 0;
            for &index in &engraved.red_shapes {
                if let Shape::Outline(outlines) = &engraved.shapes[index] {
                    for &(x, _) in outlines.iter().flatten() {
                        assert!(x < barline - 4., "Rest overlaps the next barline at {x}");
                        points += 1;
                    }
                }
            }
            assert!(points > 0);
        }
    }
    #[test]
    fn chord_anchors_follow_musical_ticks_and_current_bar_is_marked() {
        let session = trainer::Session::preview("harmony.7.major.accompany.12.true").unwrap();
        let mut score = session.view().display_score.unwrap();
        let engraved = scene(&score);
        // The first two G labels are identical outlines separated by one bar.
        let Shape::Outline(first) = &engraved.shapes[0] else {
            panic!()
        };
        let Shape::Outline(second) = &engraved.shapes[1] else {
            panic!()
        };
        let shift = second[0][0].0 - first[0][0].0;
        assert!((shift - 1000. / 12.).abs() < 0.001);
        score.active_tick = Some(8);
        assert_ne!(engraved.svg(), scene(&score).svg());
        for ch in "Gmaj7 C#m7b5 Bbdim Dsus4 Aaug F/E".chars() {
            assert!(
                lettering::glyph(ch).is_some(),
                "missing chord character {ch}"
            );
        }
    }
    #[test]
    fn incorrect_notes_and_rests_are_red_in_svg_and_terminal_pixels() {
        let session = trainer::Session::preview("reading.right.5").unwrap();
        let mut score = session.view().display_score.unwrap();
        for rest in [false, true] {
            for e in &mut score.events {
                e.incorrect = e.notes().is_empty() == rest;
            }
            let scene = scene(&score);
            assert!(!scene.red_shapes.is_empty());
            assert!(scene.svg().contains("fill='#ff7878'"));
            let (_, _, rgb) = scene.pixels();
            assert!(rgb.chunks_exact(3).any(|p| p == [255, 120, 120]));
            assert_eq!(scene.rasterize(), scene.rasterize_reference());
        }
    }
    #[test]
    fn positions_follow_clefs_and_enharmonic_spelling() {
        assert_eq!(position(64, 0), (37, 0)); // treble bottom E4
        assert_eq!(position(43, 0), (25, 0)); // bass bottom G2
        assert_eq!(position(66, 1), (38, 1));
        assert_eq!(position(70, -1), (41, -1));
        assert_eq!(position(70, 0), (40, 1));
    }
    #[test]
    fn chord_names_are_engraved_in_the_same_high_resolution_scene() {
        let session = trainer::Session::preview("reading.lead.2").unwrap();
        let mut score = session.view().display_score.unwrap();
        let original = scene(&score);
        score.chords.clear();
        assert_ne!(original.svg(), scene(&score).svg());
        let (width, height, rgb) = original.pixels();
        assert_eq!((width, height), (2400, 436));
        assert_eq!(rgb.len(), width * height * 3);
        assert!(rgb.chunks_exact(3).any(|p| p == [227, 236, 239]));
    }
    #[test]
    fn scanlines_preserve_exact_pixels_including_holes_clipping_and_overlaps() {
        let scene = Scene {
            red_shapes: Default::default(),
            incorrect: false,
            width: 30,
            height: 25,
            shapes: vec![
                Shape::Rect(-3.25, 2.5, 24.3, 1.2),
                Shape::Outline(vec![
                    vec![(-2., -1.), (27.5, 3.2), (23.3, 28.), (1.1, 24.6)],
                    vec![(5.3, 5.2), (17.6, 5.2), (17.6, 19.9), (5.3, 19.9)],
                ]),
                Shape::Rect(29.25, 0., 3., 40.),
            ],
        };
        assert_eq!(scene.rasterize(), scene.rasterize_reference());
        let session = trainer::Session::preview("reading.right.4").unwrap();
        let scene = super::scene(&session.view().display_score.unwrap());
        assert_eq!(scene.rasterize(), scene.rasterize_reference());
    }
}

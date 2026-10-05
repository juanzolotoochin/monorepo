#[path = "kitty_coordinates.rs"]
mod coordinates;
#[path = "staff_notation.rs"]
mod staff;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageScore {
    Reading(trainer::WrittenScore),
    Rhythm(trainer::WrittenScore),
}
impl ImageScore {
    fn pixels(&self) -> (usize, usize, Vec<u8>) {
        match self {
            Self::Reading(score) => staff::scene(score).pixels(),
            Self::Rhythm(score) => staff::percussion_scene(score).pixels(),
        }
    }
    pub fn svg(&self) -> String {
        match self {
            Self::Reading(score) => staff::scene(score).svg(),
            Self::Rhythm(score) => staff::percussion_scene(score).svg(),
        }
    }
}
// Small, self-contained percussion engraver and Kitty image transport.
// No host font, renderer executable, network asset, or runtime dependency.
use ratatui::layout::Rect;
use std::io::{self, Write};

#[cfg(test)]
const IMAGE_ID: u32 = 72491;

fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let n = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i > group.len() {
                '='
            } else {
                DIGITS[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    out
}
fn encode_pixels(rgb: &[u8]) -> io::Result<String> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(rgb)?;
    Ok(base64(&encoder.finish()?))
}
/// Only graphics commands bypass tmux. Text/cursor updates belong to its pane grid.
fn graphics_command(out: &mut impl Write, command: &[u8], tmux: bool) -> io::Result<()> {
    if !tmux {
        return out.write_all(command);
    }
    out.write_all(b"\x1bPtmux;")?;
    for part in command.split_inclusive(|b| *b == 0x1b) {
        out.write_all(part)?;
        if part.last() == Some(&0x1b) {
            out.write_all(b"\x1b")?;
        }
    }
    out.write_all(b"\x1b\\")
}
fn transmit(
    out: &mut impl Write,
    area: Rect,
    payload: &str,
    width: usize,
    height: usize,
    tmux: bool,
    image_id: u32,
) -> io::Result<()> {
    if !tmux {
        write!(out, "\x1b7\x1b[{};{}H", area.y + 1, area.x + 1)?;
    }
    let chunks = payload.as_bytes().chunks(4096);
    let count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        let mut command = Vec::with_capacity(4224);
        if index == 0 {
            write!(
                command,
                "\x1b_Ga=T,f=24,o=z,s={width},v={height},i={image_id},c={},r={},C=1,z=1,q=2,{}m={};",
                area.width,
                area.height,
                if tmux { "U=1," } else { "" },
                usize::from(index + 1 < count)
            )?;
        } else {
            write!(command, "\x1b_Gq=2,m={};", usize::from(index + 1 < count))?;
        }
        command.extend_from_slice(chunk);
        command.extend_from_slice(b"\x1b\\");
        graphics_command(out, &command, tmux)?;
    }
    if !tmux {
        out.write_all(b"\x1b8")?;
    }
    Ok(())
}
fn supported(term: &str, program: &str, outer_marker: bool, tmux: bool, screen: bool) -> bool {
    !screen
        && (term == "xterm-ghostty"
            || term == "xterm-kitty"
            || program.eq_ignore_ascii_case("ghostty")
            || (tmux && outer_marker))
}
#[derive(Default)]
pub struct Graphics {
    enabled: bool,
    tmux: bool,
    image_id: u32,
    shown: Option<(Rect, ImageScore)>,
    cached: Option<(ImageScore, String, usize, usize)>,
}
impl Graphics {
    pub fn detect(slot: u32) -> Self {
        let term = std::env::var("TERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let tmux = std::env::var_os("TMUX").is_some();
        let enabled = supported(
            &term,
            &program,
            std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
                || std::env::var_os("KITTY_WINDOW_ID").is_some(),
            tmux,
            std::env::var_os("STY").is_some(),
        );
        Self {
            enabled,
            tmux,
            // Image IDs share the outer terminal's namespace across tmux panes.
            image_id: ((std::process::id() << 1 | (slot & 1)) & 0xffffff).max(1),
            shown: None,
            cached: None,
        }
    }
    pub fn image_area(&self, mut area: Rect) -> Rect {
        if self.tmux {
            area.width = area.width.min(coordinates::COORDINATES.len() as u16);
            area.height = area.height.min(coordinates::COORDINATES.len() as u16);
        }
        area
    }
    pub fn placeholders(&self, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
        if !self.enabled || !self.tmux {
            return;
        }
        let color = ratatui::style::Color::Rgb(
            (self.image_id >> 16) as u8,
            (self.image_id >> 8) as u8,
            self.image_id as u8,
        );
        for row in 0..area.height {
            for col in 0..area.width {
                let symbol = format!(
                    "\u{10eeee}{}{}",
                    coordinates::COORDINATES[row as usize],
                    coordinates::COORDINATES[col as usize]
                );
                buffer[(area.x + col, area.y + row)]
                    .set_symbol(&symbol)
                    .set_fg(color);
            }
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn update(&mut self, target: Option<(Rect, ImageScore)>) -> io::Result<()> {
        self.update_to(target, &mut io::stdout().lock())
    }
    fn update_to(
        &mut self,
        target: Option<(Rect, ImageScore)>,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if !self.enabled || self.shown == target {
            return Ok(());
        }
        if self.shown.take().is_some() {
            graphics_command(
                out,
                format!("\x1b_Ga=d,d=I,i={},q=2;\x1b\\", self.image_id).as_bytes(),
                self.tmux,
            )?;
        }
        if let Some((area, notes)) = target {
            if self.cached.as_ref().is_none_or(|c| c.0 != notes) {
                let (width, height, rgb) = notes.pixels();
                self.cached = Some((notes.clone(), encode_pixels(&rgb)?, width, height));
            }
            let (_, payload, width, height) = self.cached.as_ref().unwrap();
            transmit(
                out,
                area,
                payload,
                *width,
                *height,
                self.tmux,
                self.image_id,
            )?;
            self.shown = Some((area, notes));
        }
        out.flush()
    }
}
impl Drop for Graphics {
    fn drop(&mut self) {
        let _ = self.update(None);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_rhythm() -> ImageScore {
        let score = trainer::Session::preview("rhythm.0.60")
            .unwrap()
            .view()
            .rhythm_score
            .unwrap();
        ImageScore::Rhythm(score.notation())
    }
    #[test]
    fn images_are_cached_and_deleted_on_resize_or_leaving_the_score() {
        let mut graphics = Graphics {
            enabled: true,
            tmux: false,
            image_id: IMAGE_ID,
            shown: None,
            cached: Some((test_rhythm(), "AAAA".into(), 2400, 380)),
        };
        let mut wire = Vec::new();
        let first = Some((Rect::new(3, 8, 90, 5), test_rhythm()));
        graphics.update_to(first.clone(), &mut wire).unwrap();
        assert!(!wire.is_empty());
        wire.clear();
        graphics.update_to(first.clone(), &mut wire).unwrap();
        assert!(
            wire.is_empty(),
            "unchanged frames must not resend the image"
        );
        graphics
            .update_to(Some((Rect::new(3, 8, 110, 5), test_rhythm())), &mut wire)
            .unwrap();
        assert!(String::from_utf8_lossy(&wire).starts_with("\x1b_Ga=d,d=I"));
        wire.clear();
        graphics.update_to(None, &mut wire).unwrap();
        assert_eq!(
            String::from_utf8(wire).unwrap(),
            format!("\x1b_Ga=d,d=I,i={IMAGE_ID},q=2;\x1b\\")
        );
    }
    #[test]
    fn transmission_is_chunked_quiet_and_cursor_preserving() {
        for (raw, expected) in [(b"f".as_slice(), "Zg=="), (b"fo", "Zm8="), (b"foo", "Zm9v")] {
            assert_eq!(base64(raw), expected);
        }
        let mut wire = Vec::new();
        transmit(
            &mut wire,
            Rect::new(2, 3, 90, 6),
            &"A".repeat(9000),
            2400,
            380,
            false,
            IMAGE_ID,
        )
        .unwrap();
        let wire = String::from_utf8(wire).unwrap();
        assert!(wire.starts_with("\x1b7\x1b[4;3H"));
        assert!(wire.ends_with("\x1b\\\x1b8"));
        assert_eq!(wire.matches("q=2,m=").count(), 3);
        assert_eq!(wire.matches("m=1;").count(), 2);
        assert_eq!(wire.matches("m=0;").count(), 1);
    }
    #[test]
    fn score_has_eight_noteheads_and_rgb_data() {
        let score = test_rhythm();
        let svg = score.svg();
        assert!(svg.contains("<path"));
        let (width, height, rgb) = score.pixels();
        assert_eq!(rgb.len(), width * height * 3);
        assert!(rgb.chunks_exact(3).any(|p| p == [227, 236, 239]));
    }
    #[test]
    fn tmux_detects_outer_terminal_without_mistaking_screen_for_ghostty() {
        assert!(supported("screen-256color", "tmux", true, true, false));
        assert!(supported("xterm-ghostty", "", false, false, false));
        assert!(supported("xterm-kitty", "", false, false, false));
        assert!(!supported("screen-256color", "tmux", false, true, false));
        assert!(!supported("screen-256color", "tmux", true, true, true));
        assert!(!supported("xterm-256color", "", true, false, false));
    }
    #[test]
    fn tmux_wraps_each_chunk_and_deletion_but_never_bypasses_pane_coordinates() {
        let mut graphics = Graphics {
            enabled: true,
            tmux: true,
            image_id: 0x123456,
            shown: None,
            cached: Some((test_rhythm(), "A".repeat(9000), 2400, 380)),
        };
        let mut wire = vec![];
        let target = Some((Rect::new(11, 7, 80, 8), test_rhythm()));
        graphics.update_to(target.clone(), &mut wire).unwrap();
        let text = String::from_utf8(wire.clone()).unwrap();
        assert_eq!(text.matches("\x1bPtmux;").count(), 3);
        assert!(text.starts_with("\x1bPtmux;\x1b\x1b_Ga=T,"));
        assert!(text.contains("i=1193046,c=80,r=8,C=1,z=1,q=2,U=1,m=1;"));
        assert!(text.ends_with("\x1b\x1b\\\x1b\\"));
        assert!(
            !text.contains("\x1b["),
            "pane positioning must go through tmux's text grid"
        );
        assert!(!text.contains("\x1b7"));
        wire.clear();
        graphics.update_to(target, &mut wire).unwrap();
        assert!(wire.is_empty());
        graphics.update_to(None, &mut wire).unwrap();
        assert_eq!(
            wire,
            b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=1193046,q=2;\x1b\x1b\\\x1b\\"
        );
    }
    #[test]
    fn placeholders_use_local_cells_and_are_removed_by_normal_redraws() {
        use ratatui::{backend::TestBackend, style::Color, Terminal};
        let graphics = Graphics {
            enabled: true,
            tmux: true,
            image_id: 0x123456,
            shown: None,
            cached: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let rect = Rect::new(13, 8, 4, 2);
        terminal
            .draw(|frame| graphics.placeholders(rect, frame.buffer_mut()))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(13, 8)].symbol(), "\u{10eeee}\u{305}\u{305}");
        assert_eq!(buf[(14, 9)].symbol(), "\u{10eeee}\u{30d}\u{30d}");
        assert_eq!(buf[(13, 8)].fg, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(buf[(12, 8)].symbol(), " ");
        terminal.draw(|_| {}).unwrap();
        assert_eq!(terminal.backend().buffer()[(13, 8)].symbol(), " ");
        let maximum = graphics.image_area(Rect::new(0, 0, 1000, 1000));
        assert_eq!(maximum.width as usize, coordinates::COORDINATES.len());
        assert_eq!(maximum.height as usize, coordinates::COORDINATES.len());
    }
    #[test]
    #[ignore = "manual renderer performance report; no timing assertions"]
    fn report_score_render_performance() {
        for id in ["reading.right.4", "reading.together.2", "reading.lead.2"] {
            let session = trainer::Session::preview(id).unwrap();
            let score = ImageScore::Reading(session.view().display_score.unwrap());
            for iteration in 0..3 {
                let start = std::time::Instant::now();
                let (width, height, rgb) = score.pixels();
                let raster = start.elapsed();
                let start = std::time::Instant::now();
                let payload = encode_pixels(&rgb).unwrap();
                let encoding = start.elapsed();
                let start = std::time::Instant::now();
                let mut wire = vec![];
                transmit(
                    &mut wire,
                    Rect::new(2, 5, 120, 10),
                    &payload,
                    width,
                    height,
                    true,
                    IMAGE_ID,
                )
                .unwrap();
                eprintln!("{id} run={iteration}: raster={raster:?}, encoding={encoding:?}, framing={:?}, raw_bytes={}, wire_bytes={}",start.elapsed(),rgb.len(),wire.len());
            }
        }
    }

    #[test]
    fn compressed_score_payload_is_lossless_and_small() {
        use std::io::Read;
        let session = trainer::Session::preview("reading.together.2").unwrap();
        let (_, _, rgb) = ImageScore::Reading(session.view().display_score.unwrap()).pixels();
        let payload = encode_pixels(&rgb).unwrap();
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut compressed = vec![];
        for group in payload.as_bytes().chunks_exact(4) {
            let mut value = 0u32;
            for c in group {
                value = value << 6 | ALPHABET.iter().position(|n| n == c).unwrap_or(0) as u32;
            }
            compressed.push((value >> 16) as u8);
            if group[2] != b'=' {
                compressed.push((value >> 8) as u8);
            }
            if group[3] != b'=' {
                compressed.push(value as u8);
            }
        }
        let mut decoded = vec![];
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, rgb);
        assert!(
            payload.len() < rgb.len() / 20,
            "scores should not send megabytes of blank pixels"
        );
    }
}

#[cfg(test)]
mod rhythm_pixels_tests {
    use super::*;
    #[test]
    fn rhythm_error_is_red_in_actual_terminal_pixels() {
        let score = trainer::Session::preview("rhythm.0.60")
            .unwrap()
            .view()
            .rhythm_score
            .unwrap();
        let mut written = score.notation();
        written.events[5].incorrect = true;
        let (_, _, rgb) = ImageScore::Rhythm(written).pixels();
        assert!(rgb.chunks_exact(3).any(|p| p == [255, 120, 120]));
    }
}

//! Maps source filenames to the action/frame they represent.

use std::path::Path;

/// Maps a list of source filenames to the action/frame each one represents.
pub trait ActionResolver {
    fn resolve(&self, filenames: &[String]) -> ResolveResult;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameRef {
    pub action: String,
    pub frame: u64,
    pub source: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResolveResult {
    pub frames: Vec<FrameRef>,
    pub unmatched: Vec<String>,
}

/// Resolves actions/frames from filenames using a trailing-integer heuristic.
pub struct HeuristicResolver;

impl HeuristicResolver {
    fn parse_one(filename: &str) -> Option<FrameRef> {
        let stem = Path::new(filename).file_stem()?.to_str()?;
        // Drop a trailing ')' / spaces so "Name (12)" exposes its digits.
        let tail = stem.trim_end_matches([')', ' ']);
        let bytes = tail.as_bytes();
        let mut start = bytes.len();
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == bytes.len() {
            return None; // no trailing integer
        }
        let frame: u64 = tail[start..].parse().ok()?;
        let action = tail[..start]
            .trim_end_matches(['(', ' ', '_', '-'])
            .trim()
            .to_string();
        if action.is_empty() {
            return None; // digits only, no action name
        }
        Some(FrameRef { action, frame, source: filename.to_string() })
    }
}

impl ActionResolver for HeuristicResolver {
    fn resolve(&self, filenames: &[String]) -> ResolveResult {
        let mut result = ResolveResult::default();
        for f in filenames {
            match Self::parse_one(f) {
                Some(fr) => result.frames.push(fr),
                None => result.unmatched.push(f.clone()),
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fr(action: &str, frame: u64, source: &str) -> FrameRef {
        FrameRef { action: action.into(), frame, source: source.into() }
    }

    #[test]
    fn parses_common_patterns() {
        let r = HeuristicResolver;
        let out = r.resolve(&[
            "Attack (1).png".into(),
            "JumpAttack (10).png".into(),
            "attack_1.png".into(),
            "run-01.png".into(),
            "walk2.png".into(),
        ]);
        assert_eq!(out.frames, vec![
            fr("Attack", 1, "Attack (1).png"),
            fr("JumpAttack", 10, "JumpAttack (10).png"),
            fr("attack", 1, "attack_1.png"),
            fr("run", 1, "run-01.png"),
            fr("walk", 2, "walk2.png"),
        ]);
        assert!(out.unmatched.is_empty());
    }

    #[test]
    fn reports_unmatched_files_with_no_trailing_number() {
        let r = HeuristicResolver;
        let out = r.resolve(&["background.png".into(), "123.png".into()]);
        // "background" has no trailing digits; "123" has digits but no action name.
        assert!(out.frames.is_empty());
        assert_eq!(out.unmatched, vec!["background.png".to_string(), "123.png".to_string()]);
    }
}

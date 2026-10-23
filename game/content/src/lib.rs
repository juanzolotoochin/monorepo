//! Content: the definitions the game is built from.
//!
//! Each definition is one RON file with a `sim` section and a `view` section.
//! Files are named by their path, for example `mover/small.ron` defines
//! `mover/small`. At runtime definitions are referred to by dense indices,
//! assigned in name order; saves carry the index-to-name tables so they can be
//! remapped when content changes.
//!
//! Only the `sim` sections reach the simulation, as [`Rules`], and only they
//! feed the rules hash. Peers with different rules hashes can't play together;
//! changing a `view` section keeps them compatible.

use std::fmt;
use std::path::Path;

use fx::Fx;
use serde::{Deserialize, Serialize};

/// The simulation rate. Content is written in seconds and per-second rates and
/// converted to ticks at load, so this is the only place the rate appears.
pub const TICKS_PER_SECOND: i32 = 30;

/// The largest body radius content may define, in tiles. Spatial queries
/// widen their search by the largest radius in use, so this bounds their cost.
pub const MAX_BODY_RADIUS: Fx = Fx::from_int(8);

/// A mover's simulation rules, in per-tick units.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoverDef {
    pub radius: Fx,
    pub max_hp: i32,
    /// Tiles per tick.
    pub max_speed: Fx,
    /// Relative odds of being picked when a spawn command creates movers.
    pub spawn_weight: u32,
}

/// What the renderer needs to draw a mover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoverView {
    pub color: (f32, f32, f32),
}

/// Every `sim` section, by dense index. This is all the simulation sees.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub movers: Vec<MoverDef>,
}

impl Rules {
    pub fn max_body_radius(&self) -> Fx {
        self.movers.iter().map(|m| m.radius).max().unwrap_or(Fx::ZERO)
    }

    pub fn max_speed(&self) -> Fx {
        self.movers.iter().map(|m| m.max_speed).max().unwrap_or(Fx::ZERO)
    }
}

/// Index-to-name tables, one per kind of definition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Names {
    pub movers: Vec<String>,
}

impl Names {
    pub fn mover_index(&self, name: &str) -> Option<u16> {
        self.movers.binary_search_by(|n| n.as_str().cmp(name)).ok().map(|i| i as u16)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Views {
    pub movers: Vec<MoverView>,
}

#[derive(Clone, Debug)]
pub struct Content {
    pub rules: Rules,
    pub names: Names,
    pub views: Views,
    /// Hash of the names and rules. Peers must match to play together.
    pub rules_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentError {
    pub file: String,
    pub message: String,
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

impl std::error::Error for ContentError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoverFile {
    sim: MoverSimFile,
    view: MoverView,
}

/// The `sim` section as written, in seconds.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoverSimFile {
    radius: Fx,
    max_hp: i32,
    /// Tiles per second.
    speed: Fx,
    spawn_weight: u32,
}

/// The content shipped with the game, compiled into the binary.
const BUILTIN: &[(&str, &str)] = &[
    ("mover/large.ron", include_str!("../data/mover/large.ron")),
    ("mover/medium.ron", include_str!("../data/mover/medium.ron")),
    ("mover/small.ron", include_str!("../data/mover/small.ron")),
];

/// The builtin content files as `(path, text)` pairs, for building variants in tests and tools.
pub fn builtin_sources() -> Vec<(String, String)> {
    BUILTIN.iter().map(|&(path, text)| (path.to_string(), text.to_string())).collect()
}

impl Content {
    pub fn builtin() -> Content {
        Content::from_sources(builtin_sources()).expect("builtin content is valid")
    }

    /// Loads every `.ron` file under `root`, named by its path relative to `root`.
    pub fn load_dir(root: &Path) -> Result<Content, ContentError> {
        let mut sources = Vec::new();
        collect_files(root, root, &mut sources).map_err(|e| ContentError {
            file: root.display().to_string(),
            message: e.to_string(),
        })?;
        Content::from_sources(sources)
    }

    /// Builds content from `(path, text)` pairs, such as `("mover/small.ron", "...")`.
    pub fn from_sources(sources: impl IntoIterator<Item = (String, String)>) -> Result<Content, ContentError> {
        let mut movers = Vec::new();
        for (path, text) in sources {
            let error = |message: String| ContentError { file: path.clone(), message };
            let name = path.strip_suffix(".ron").ok_or_else(|| error("not a .ron file".into()))?.replace('\\', "/");
            match name.split_once('/') {
                Some(("mover", _)) => {
                    let file: MoverFile = ron::from_str(&text).map_err(|e| error(e.to_string()))?;
                    let def = mover_def(file.sim).map_err(error)?;
                    movers.push((name, def, file.view));
                }
                _ => return Err(error("unknown kind of definition; expected a path like mover/<name>.ron".into())),
            }
        }
        movers.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(pair) = movers.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(ContentError { file: pair[0].0.clone(), message: "defined twice".into() });
        }

        let names = Names { movers: movers.iter().map(|m| m.0.clone()).collect() };
        let rules = Rules { movers: movers.iter().map(|m| m.1.clone()).collect() };
        let views = Views { movers: movers.into_iter().map(|m| m.2).collect() };
        let rules_hash = rules_hash(&names, &rules);
        Ok(Content { rules, names, views, rules_hash })
    }
}

fn mover_def(file: MoverSimFile) -> Result<MoverDef, String> {
    if file.radius <= Fx::ZERO || file.radius > MAX_BODY_RADIUS {
        return Err(format!("radius must be in (0, {MAX_BODY_RADIUS}]"));
    }
    if file.max_hp <= 0 {
        return Err("max_hp must be positive".into());
    }
    let max_speed = file.speed.div_int(TICKS_PER_SECOND as i64);
    if file.speed < Fx::ZERO || max_speed > Fx::ONE {
        return Err(format!("speed must be in [0, {TICKS_PER_SECOND}] tiles per second"));
    }
    Ok(MoverDef { radius: file.radius, max_hp: file.max_hp, max_speed, spawn_weight: file.spawn_weight })
}

/// FNV-1a over the binary encoding of the tick rate, names and rules.
fn rules_hash(names: &Names, rules: &Rules) -> u64 {
    let bytes = postcard::to_allocvec(&(TICKS_PER_SECOND, names, rules)).expect("encode rules");
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, &byte| (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "ron") {
            let relative = path.strip_prefix(root).expect("under root").to_string_lossy().replace('\\', "/");
            out.push((relative, std::fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: &str, text: &str) -> (String, String) {
        (path.to_string(), text.to_string())
    }

    const SMALL: &str = "(sim: (radius: 0.5, max_hp: 20, speed: 7.5, spawn_weight: 6), view: (color: (1, 0, 0)))";

    #[test]
    fn builtin_loads_in_name_order() {
        let content = Content::builtin();
        assert_eq!(content.names.movers, ["mover/large", "mover/medium", "mover/small"]);
        let small = &content.rules.movers[2];
        assert_eq!(small.radius, Fx::HALF);
        assert_eq!(small.max_speed, Fx::from_ratio(1, 4));
        assert_eq!(content.names.mover_index("mover/small"), Some(2));
        assert_eq!(content.rules.max_body_radius(), Fx::from_int(2));
    }

    #[test]
    fn rules_hash_ignores_view_changes() {
        let a = Content::from_sources([source("mover/small.ron", SMALL)]).unwrap();
        let b = Content::from_sources([source("mover/small.ron", &SMALL.replace("(1, 0, 0)", "(0, 1, 0)"))]).unwrap();
        let c = Content::from_sources([source("mover/small.ron", &SMALL.replace("7.5", "9"))]).unwrap();
        let d = Content::from_sources([source("mover/tiny.ron", SMALL)]).unwrap();
        assert_eq!(a.rules_hash, b.rules_hash);
        assert_ne!(a.rules_hash, c.rules_hash);
        assert_ne!(a.rules_hash, d.rules_hash);
    }

    #[test]
    fn errors_name_the_file() {
        let bad = |path: &str, text: &str| Content::from_sources([source(path, text)]).unwrap_err();
        assert_eq!(bad("mover/x.ron", &SMALL.replace("0.5", "9")).file, "mover/x.ron");
        assert!(bad("mover/x.ron", &SMALL.replace("max_hp", "hp")).message.contains("hp"));
        assert!(bad("tower/x.ron", SMALL).message.contains("unknown kind"));
        let twice = Content::from_sources([source("mover/x.ron", SMALL), source("mover/x.ron", SMALL)]);
        assert_eq!(twice.unwrap_err().message, "defined twice");
    }

    #[test]
    fn decimals_parse_exactly() {
        let content = Content::from_sources([source("mover/x.ron", &SMALL.replace("0.5", "0.1"))]).unwrap();
        assert_eq!(content.rules.movers[0].radius, "0.1".parse().unwrap());
    }
}

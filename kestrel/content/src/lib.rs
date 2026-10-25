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

pub mod character;
pub mod scenario;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use fx::Fx;
use serde::{Deserialize, Serialize};

/// The simulation rate. Content is written in seconds and per-second rates and
/// converted to ticks at load, and code constants go through [`millis_to_ticks`],
/// [`per_tick`] or `scenario::ticks_from_60hz`, so this is the only place the
/// rate appears.
pub const TICKS_PER_SECOND: i32 = 60;

/// Whole ticks in `ms` milliseconds, rounded to the nearest (at least 1 for a
/// nonzero time).
pub const fn millis_to_ticks(ms: u64) -> u64 {
    let ticks = (ms * TICKS_PER_SECOND as u64 + 500) / 1000;
    if ticks == 0 && ms > 0 { 1 } else { ticks }
}

/// A per-second rate as a per-tick one, rounded to the nearest.
pub const fn per_tick(per_second: Fx) -> Fx {
    let (bits, rate) = (per_second.to_bits(), TICKS_PER_SECOND as i64);
    let half = if bits < 0 { -rate / 2 } else { rate / 2 };
    Fx::from_bits((bits + half) / rate)
}

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
    pub characters: Vec<character::CharacterDef>,
}

impl Rules {
    // Body radius and speed bound the mover chunk queries. Characters live in
    // the settlement's world-wide tables, so they are left out.
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
    pub characters: Vec<String>,
}

impl Names {
    pub fn mover_index(&self, name: &str) -> Option<u16> {
        self.movers.binary_search_by(|n| n.as_str().cmp(name)).ok().map(|i| i as u16)
    }

    /// A character by its short name, `ghoul` for `character/ghoul`.
    pub fn character_index(&self, name: &str) -> Option<character::CharacterIx> {
        let full = format!("character/{name}");
        self.characters.binary_search_by(|n| n.as_str().cmp(&full)).ok().map(|i| i as character::CharacterIx)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Views {
    pub movers: Vec<MoverView>,
    pub characters: Vec<character::CharacterView>,
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
    ("character/exosuit.ron", include_str!("../data/character/exosuit.ron")),
    ("character/ghoul.ron", include_str!("../data/character/ghoul.ron")),
    ("character/ghoul_arena_east.ron", include_str!("../data/character/ghoul_arena_east.ron")),
    ("character/ghoul_brute.ron", include_str!("../data/character/ghoul_brute.ron")),
    ("character/ghoul_forest.ron", include_str!("../data/character/ghoul_forest.ron")),
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
        let mut character_files = BTreeMap::new();
        for (path, text) in sources {
            let error = |message: String| ContentError { file: path.clone(), message };
            let name = path.strip_suffix(".ron").ok_or_else(|| error("not a .ron file".into()))?.replace('\\', "/");
            match name.split_once('/') {
                Some(("mover", _)) => {
                    let file: MoverFile = ron::from_str(&text).map_err(|e| error(e.to_string()))?;
                    let def = mover_def(file.sim).map_err(error)?;
                    movers.push((name, def, file.view));
                }
                Some(("character", _)) => {
                    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
                    let file: character::CharacterFile = options.from_str(&text).map_err(|e| error(e.to_string()))?;
                    if character_files.insert(name.clone(), file).is_some() {
                        return Err(ContentError { file: name, message: "defined twice".into() });
                    }
                }
                _ => return Err(error("unknown kind of definition; expected a path like mover/<name>.ron".into())),
            }
        }
        movers.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(pair) = movers.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(ContentError { file: pair[0].0.clone(), message: "defined twice".into() });
        }
        let mut characters = Vec::new();
        for name in character_files.keys() {
            let error = |message: String| ContentError { file: name.clone(), message };
            let sim = resolve_character(&character_files, name, &mut Vec::new()).map_err(error)?;
            let def = character::character_def(sim).map_err(error)?;
            characters.push((name.clone(), def, character_files[name].view.clone()));
        }

        let names = Names {
            movers: movers.iter().map(|m| m.0.clone()).collect(),
            characters: characters.iter().map(|c| c.0.clone()).collect(),
        };
        let rules = Rules {
            movers: movers.iter().map(|m| m.1.clone()).collect(),
            characters: characters.iter().map(|c| c.1.clone()).collect(),
        };
        let views = Views {
            movers: movers.into_iter().map(|m| m.2).collect(),
            characters: characters.into_iter().map(|c| c.2).collect(),
        };
        let rules_hash = rules_hash(&names, &rules);
        Ok(Content { rules, names, views, rules_hash })
    }
}

/// A character's `sim` section with its `base` chain merged in. `chain`
/// holds the characters being resolved, to catch cycles.
fn resolve_character(
    files: &BTreeMap<String, character::CharacterFile>,
    name: &str,
    chain: &mut Vec<String>,
) -> Result<character::SimFile, String> {
    use character::Merge;
    if chain.iter().any(|n| n == name) {
        chain.push(name.to_string());
        return Err(format!("base cycle: {}", chain.join(" -> ")));
    }
    let file = &files[name];
    let Some(base) = &file.base else { return Ok(file.sim.clone()) };
    let base_name = format!("character/{base}");
    if !files.contains_key(&base_name) {
        return Err(format!("unknown base {base:?}"));
    }
    chain.push(name.to_string());
    let base_sim = resolve_character(files, &base_name, chain)?;
    chain.pop();
    Ok(file.sim.clone().merge(base_sim))
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
        assert_eq!(small.max_speed, Fx::from_ratio(15, 2 * TICKS_PER_SECOND as i64)); // 7.5 tiles/s
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

    const GHOUL: &str = "(sim: (brain: FollowRoute, body: (radius: 0.27), movement: (walk: 0.75), \
        health: (hp: 3, breach_damage: 1)), view: ())";

    fn characters(files: &[(&str, &str)]) -> Result<Content, ContentError> {
        Content::from_sources(files.iter().map(|(path, text)| source(path, text)))
    }

    #[test]
    fn builtin_characters_load_by_short_name() {
        let content = Content::builtin();
        let exosuit = content.names.character_index(character::PLAYER_CHARACTER).expect("the player character");
        let def = &content.rules.characters[exosuit as usize];
        assert_eq!(def.brain, character::Brain::Player);
        assert!(def.avatar.is_some() && def.health.is_none());
        let ghoul = content.names.character_index("ghoul").unwrap();
        assert_eq!(content.names.characters[ghoul as usize], "character/ghoul");
        assert_eq!(content.views.characters.len(), content.rules.characters.len());
    }

    /// The builtin characters hold exactly the values the simulation used
    /// as code constants before they moved here, bit for bit. Change these
    /// only together with an intended golden update.
    #[test]
    fn builtin_characters_keep_the_former_constants() {
        use scenario::ticks_from_60hz;
        let content = Content::builtin();
        let get = |name: &str| &content.rules.characters[content.names.character_index(name).unwrap() as usize];
        let exosuit = get("exosuit");
        assert_eq!(exosuit.body.radius.to_bits(), 1_374_389_535); // BODY_RADIUS, 0.32
        assert_eq!(exosuit.movement.walk.to_bits(), 16_320_875_725); // WALK_SPEED, 3.8
        assert_eq!(exosuit.movement.run.to_bits(), 24_051_816_858); // RUN_SPEED, 5.6
        let avatar = exosuit.avatar.as_ref().unwrap();
        assert_eq!(avatar.acceleration, per_tick(Fx::from_int(26)));
        assert_eq!(avatar.braking, per_tick(Fx::from_int(32)));
        assert_eq!(avatar.attack_braking, per_tick(Fx::from_int(35)));
        let attack = avatar.attack;
        let frames = [attack.windup, attack.aim, attack.swing, attack.sword_windup, attack.sword_swing, attack.rebound, attack.combo_window];
        assert_eq!(frames, [13, 10, 35, 7, 26, 25, 75].map(ticks_from_60hz));
        assert_eq!(get("ghoul").body.radius.to_bits(), 1_159_641_170); // THREAT_RADIUS, 0.27
        let brute = get("ghoul_brute").health.as_ref().unwrap();
        assert_eq!(brute.armor.as_ref().unwrap().radius.to_bits(), 1_632_087_572); // ARMORED_RADIUS, 0.38
    }

    #[test]
    fn a_base_fills_what_a_variant_leaves_out() {
        let brute = "(base: \"ghoul\", sim: (movement: (walk: 0.59), health: (hp: 5, armor: (points: 2, radius: 0.38))), view: ())";
        let content = characters(&[("character/ghoul.ron", GHOUL), ("character/brute.ron", brute)]).unwrap();
        let def = &content.rules.characters[content.names.character_index("brute").unwrap() as usize];
        assert_eq!(def.brain, character::Brain::FollowRoute, "from the base");
        assert_eq!(def.body.radius, "0.27".parse().unwrap(), "from the base");
        assert_eq!(def.movement.walk, "0.59".parse().unwrap());
        assert_eq!(def.movement.run, def.movement.walk, "run defaults to walk");
        let health = def.health.as_ref().unwrap();
        assert_eq!((health.hp, health.breach_damage), (5, 1), "hp overridden, breach damage merged in");
        assert_eq!(health.armor.as_ref().map(|a| a.points), Some(2));
    }

    #[test]
    fn bad_bases_name_the_file() {
        let child = |base: &str| format!("(base: \"{base}\", sim: (), view: ())");
        let missing = characters(&[("character/a.ron", &child("nobody"))]).unwrap_err();
        assert_eq!(missing.file, "character/a");
        assert!(missing.message.contains("unknown base"));
        let cycle = characters(&[("character/a.ron", &child("b")), ("character/b.ron", &child("a"))]).unwrap_err();
        assert!(cycle.message.contains("cycle"), "{}", cycle.message);
        let own = characters(&[("character/a.ron", &child("a"))]).unwrap_err();
        assert!(own.message.contains("cycle"));
    }

    #[test]
    fn merged_characters_are_validated() {
        let bad = |text: &str| characters(&[("character/x.ron", text)]).unwrap_err().message;
        assert!(bad("(sim: (brain: FollowRoute), view: ())").contains("body"));
        assert!(bad(&GHOUL.replace("0.27", "9")).contains("radius"));
        assert!(bad(&GHOUL.replace("walk: 0.75", "walk: 0.75, run: 0.5")).contains("walk <= run"));
        assert!(bad(&GHOUL.replace("FollowRoute", "Player")).contains("Player"));
        assert!(bad(&GHOUL.replace("hp: 3", "hp: 3, armor: (points: 2, radius: 0.1)")).contains("armor"));
        assert!(bad(&GHOUL.replace("view: ()", "view: (skin: \"x\")")).contains("skin"));
        assert!(bad(&GHOUL.replace("radius: 0.27", "radius: 0.27, mass: 2")).contains("mass"));
    }

    #[test]
    fn character_rules_hash_ignores_views_only() {
        let hash = |files: &[(&str, &str)]| characters(files).unwrap().rules_hash;
        let base = hash(&[("character/ghoul.ron", GHOUL)]);
        assert_ne!(base, hash(&[("character/ghoul.ron", &GHOUL.replace("hp: 3", "hp: 4"))]));
        assert_ne!(base, hash(&[("character/ghast.ron", GHOUL)]));
        let comment = format!("// A comment.\n{GHOUL}");
        assert_eq!(base, hash(&[("character/ghoul.ron", &comment)]));
    }
}

//! Characters: one definition per kind of avatar, hauler or enemy.
//!
//! A file `character/<name>.ron` has an optional `base`, a `sim` section and
//! a `view` section. A file with a `base` starts from that character and
//! overrides only the fields it writes; nested sections merge field by field.
//! Durations are whole 60 Hz frames (see `docs/architecture.md`), rates are
//! per second, and both are converted to simulation units at load.

use fx::Fx;
use serde::{Deserialize, Serialize};

use crate::scenario::ticks_from_60hz;
use crate::{per_tick, MAX_BODY_RADIUS};

/// Index into `Rules::characters`, in name order.
pub type CharacterIx = u16;

/// The character every controlled avatar uses until scenes name their own.
pub const PLAYER_CHARACTER: &str = "exosuit";

/// How a character decides what to do. The behaviours themselves are code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brain {
    /// Driven by a player's commands.
    Player,
    /// Walks its wave's route to the target and breaches.
    FollowRoute,
}

/// A character's simulation rules, in simulation units.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterDef {
    pub brain: Brain,
    pub body: BodyDef,
    pub movement: MovementDef,
    /// Enemies only; player health is a scenario rule.
    pub health: Option<HealthDef>,
    /// Player characters only.
    pub avatar: Option<AvatarDef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyDef {
    /// Collision radius in meters.
    pub radius: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovementDef {
    /// Meters per second.
    pub walk: Fx,
    /// Meters per second.
    pub run: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthDef {
    pub hp: u32,
    pub armor: Option<ArmorDef>,
    /// Base health lost when this character breaches.
    pub breach_damage: u32,
}

/// Plating that takes hits first and widens the body until it breaks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmorDef {
    pub points: u32,
    pub radius: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvatarDef {
    /// Velocity change per tick, in m/s.
    pub acceleration: Fx,
    pub braking: Fx,
    /// Braking while a swing holds the avatar in place.
    pub attack_braking: Fx,
    pub attack: AttackTiming,
}

/// Swing timing in simulation ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackTiming {
    /// From the attack command to contact.
    pub windup: u64,
    /// The visible windup, while the avatar turns toward the aim.
    pub aim: u64,
    /// A swing that hits nothing ends after this.
    pub swing: u64,
    /// The swift sword's windup and swing.
    pub sword_windup: u64,
    pub sword_swing: u64,
    /// A swing that lands rebounds for this long.
    pub rebound: u64,
    /// Another sword press within this continues the combo.
    pub combo_window: u64,
}

/// What the renderer needs to draw a character. Empty until rigs move here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterView {}

/// A character file as written: every field optional, so a `base` can fill
/// what a variant leaves out.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CharacterFile {
    #[serde(default)]
    pub base: Option<String>,
    pub sim: SimFile,
    pub view: CharacterView,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct SimFile {
    brain: Option<Brain>,
    body: Option<BodyFile>,
    movement: Option<MovementFile>,
    health: Option<HealthFile>,
    avatar: Option<AvatarFile>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct BodyFile {
    radius: Option<Fx>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct MovementFile {
    walk: Option<Fx>,
    /// Defaults to `walk`.
    run: Option<Fx>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct HealthFile {
    hp: Option<u32>,
    /// Replaced as a whole, not merged.
    armor: Option<ArmorFile>,
    breach_damage: Option<u32>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmorFile {
    points: u32,
    radius: Fx,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct AvatarFile {
    /// m/s².
    acceleration: Option<Fx>,
    braking: Option<Fx>,
    attack_braking: Option<Fx>,
    attack: Option<AttackFile>,
}

/// 60 Hz frames.
#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct AttackFile {
    windup: Option<u64>,
    aim: Option<u64>,
    swing: Option<u64>,
    sword_windup: Option<u64>,
    sword_swing: Option<u64>,
    rebound: Option<u64>,
    combo_window: Option<u64>,
}

/// Fills the leaf fields `self` leaves out from `base`.
pub(crate) trait Merge: Sized {
    fn merge(self, base: Self) -> Self;
}

impl<T: Merge> Merge for Option<T> {
    fn merge(self, base: Self) -> Self {
        match (self, base) {
            (Some(own), Some(base)) => Some(own.merge(base)),
            (own, base) => own.or(base),
        }
    }
}

macro_rules! merge_fields {
    ($ty:ident { $($leaf:ident),* } { $($nested:ident),* }) => {
        impl Merge for $ty {
            fn merge(self, base: Self) -> Self {
                $ty { $($leaf: self.$leaf.or(base.$leaf),)* $($nested: self.$nested.merge(base.$nested),)* }
            }
        }
    };
}

merge_fields!(SimFile { brain } { body, movement, health, avatar });
merge_fields!(BodyFile { radius } {});
merge_fields!(MovementFile { walk, run } {});
merge_fields!(HealthFile { hp, armor, breach_damage } {});
merge_fields!(AvatarFile { acceleration, braking, attack_braking } { attack });
merge_fields!(AttackFile { windup, aim, swing, sword_windup, sword_swing, rebound, combo_window } {});

fn need<T>(value: Option<T>, field: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("missing {field} (here or in a base)"))
}

fn within(v: Fx, lo: Fx, hi: Fx) -> bool {
    v >= lo && v <= hi
}

/// Checks a merged definition and converts it to simulation units.
pub(crate) fn character_def(file: SimFile) -> Result<CharacterDef, String> {
    let brain = need(file.brain, "brain")?;
    let radius = need(need(file.body, "body")?.radius, "body.radius")?;
    if radius <= Fx::ZERO || radius > MAX_BODY_RADIUS {
        return Err(format!("body.radius must be in (0, {MAX_BODY_RADIUS}]"));
    }
    let movement = need(file.movement, "movement")?;
    let walk = need(movement.walk, "movement.walk")?;
    let run = movement.run.unwrap_or(walk);
    if !within(walk, Fx::from_ratio(1, 10), Fx::from_int(15)) || !within(run, walk, Fx::from_int(15)) {
        return Err("movement needs 0.1 <= walk <= run <= 15 m/s".into());
    }
    let health = file
        .health
        .map(|h| {
            let hp = need(h.hp, "health.hp")?;
            let breach_damage = need(h.breach_damage, "health.breach_damage")?;
            if hp == 0 || hp > 1000 || breach_damage == 0 || breach_damage > 1000 {
                return Err("health.hp and health.breach_damage must be in 1..=1000".to_string());
            }
            let armor = h.armor.map(|a| ArmorDef { points: a.points, radius: a.radius });
            if let Some(a) = &armor {
                if a.points == 0 || a.points > 8 || a.radius < radius || a.radius > MAX_BODY_RADIUS {
                    return Err("health.armor needs 1..=8 points and a radius from body.radius up".to_string());
                }
            }
            Ok(HealthDef { hp, armor, breach_damage })
        })
        .transpose()?;
    let avatar = file.avatar.map(avatar_def).transpose()?;
    match brain {
        Brain::Player if avatar.is_none() || health.is_some() => {
            return Err("a Player character needs `avatar` and no `health` (player health is a scenario rule)".into())
        }
        Brain::FollowRoute if health.is_none() || avatar.is_some() => {
            return Err("a FollowRoute character needs `health` and no `avatar`".into())
        }
        _ => {}
    }
    Ok(CharacterDef { brain, body: BodyDef { radius }, movement: MovementDef { walk, run }, health, avatar })
}

fn avatar_def(file: AvatarFile) -> Result<AvatarDef, String> {
    let rate = |value: Option<Fx>, field: &str| -> Result<Fx, String> {
        let value = need(value, field)?;
        if !within(value, Fx::ONE, Fx::from_int(200)) {
            return Err(format!("{field} must be in [1, 200] m/s²"));
        }
        Ok(per_tick(value))
    };
    let attack = need(file.attack, "avatar.attack")?;
    let frames = |value: Option<u64>, field: &str| -> Result<u64, String> {
        let value = need(value, field)?;
        if !(1..=600).contains(&value) {
            return Err(format!("{field} must be 1..=600 frames"));
        }
        Ok(ticks_from_60hz(value))
    };
    let attack = AttackTiming {
        windup: frames(attack.windup, "avatar.attack.windup")?,
        aim: frames(attack.aim, "avatar.attack.aim")?,
        swing: frames(attack.swing, "avatar.attack.swing")?,
        sword_windup: frames(attack.sword_windup, "avatar.attack.sword_windup")?,
        sword_swing: frames(attack.sword_swing, "avatar.attack.sword_swing")?,
        rebound: frames(attack.rebound, "avatar.attack.rebound")?,
        combo_window: frames(attack.combo_window, "avatar.attack.combo_window")?,
    };
    if attack.windup >= attack.swing || attack.aim > attack.swing || attack.sword_windup >= attack.sword_swing {
        return Err("avatar.attack windups must end before their swings".into());
    }
    Ok(AvatarDef {
        acceleration: rate(file.acceleration, "avatar.acceleration")?,
        braking: rate(file.braking, "avatar.braking")?,
        attack_braking: rate(file.attack_braking, "avatar.attack_braking")?,
        attack,
    })
}

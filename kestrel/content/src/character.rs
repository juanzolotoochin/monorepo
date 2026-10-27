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

/// How a character decides what to do. The behaviours themselves are code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brain {
    /// Driven by a player's commands.
    Player,
    /// A threat: walks the route its wave planned to the target and breaches.
    FollowRoute,
    /// Carries goods along its haul links.
    Hauler,
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
    /// Characters that carry goods: players and haulers.
    pub carry: Option<CarryDef>,
    /// What a player character can do beyond walking and attacking. Empty
    /// for everyone else.
    pub abilities: Vec<Ability>,
    /// How a FollowRoute character hits players in passing. Without it, it
    /// never attacks. Left out of the encoding so characters without one keep
    /// their rules hash; `rules_hash` adds it when any character has one.
    #[serde(skip)]
    pub attack: Option<AttackDef>,
}

/// A threat's swing at a player, in simulation units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackDef {
    /// Player health lost per hit.
    pub damage: u32,
    /// Meters from the threat's center to the player's center.
    pub reach: Fx,
    /// Ticks from starting the swing to the hit.
    pub windup: u64,
    /// Ticks after a swing lands before the next can start.
    pub cooldown: u64,
    /// Speed the hit gives the player away from the threat, in m/s.
    pub knockback: Fx,
}

/// A move a player character can make. The behaviour is code; the tuning
/// is data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ability {
    /// A grounded jump. Replaces the jetpack.
    Jump(JumpDef),
    /// Hold to hover over the ground ahead.
    Jetpack(JetpackDef),
    /// Dig out the tile underfoot, where the scene's ground can be dug.
    Dig(DigDef),
    /// Swing the sword with the short `sword_windup` and `sword_swing` timings.
    SwiftSword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpDef {
    /// Ticks crouching before takeoff.
    pub prep: u64,
    /// Ticks recovering after touchdown.
    pub land: u64,
    /// Takeoff speed, meters per tick.
    pub velocity: Fx,
    /// Meters per tick, per tick.
    pub gravity: Fx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackDef {
    /// Height above the ground ahead, in meters.
    pub hover: Fx,
    /// Meters per tick.
    pub climb: Fx,
    pub sink: Fx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigDef {
    /// Ticks to dig out one layer, standing still.
    pub ticks: u64,
    /// Meters per tick the digger steps toward the middle of its tile.
    pub centering: Fx,
}

impl CharacterDef {
    pub fn jump(&self) -> Option<&JumpDef> {
        self.abilities.iter().find_map(|a| if let Ability::Jump(j) = a { Some(j) } else { None })
    }

    pub fn jetpack(&self) -> Option<&JetpackDef> {
        self.abilities.iter().find_map(|a| if let Ability::Jetpack(j) = a { Some(j) } else { None })
    }

    pub fn dig(&self) -> Option<&DigDef> {
        self.abilities.iter().find_map(|a| if let Ability::Dig(d) = a { Some(d) } else { None })
    }

    pub fn swift_sword(&self) -> bool {
        self.abilities.contains(&Ability::SwiftSword)
    }
}

/// How a character handles cargo. How much it carries is set where it is
/// placed, in the scenario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarryDef {
    /// Ticks to load or unload one unit.
    pub transfer: u64,
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

/// What the renderer needs to draw a character. View values are plain `f32`
/// and never reach the simulation or the rules hash.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterView {
    /// The style skin that draws it, by id. Every character but a Player has
    /// one; a Player is drawn by its `actor`.
    pub skin: Option<String>,
    /// The articulated body a Player is built from, by name (an actor file
    /// in the app, such as `actors/exosuit.ron`). Styles may re-dress its
    /// joints. Players only.
    pub actor: Option<String>,
    /// How a Player's rig poses. Its walk and run speeds are the character's
    /// `movement`, so the view can't pose other speeds than it moves at.
    pub motion: CharacterMotion,
}

/// Pose tuning for a Player's rig. The rig a style bakes may adapt it further
/// (planted feet for legged actors, longer reach for baked boots).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterMotion {
    pub motion_style: MotionStyle,
    /// Ground speeds in meters per second, from the character's `movement`.
    pub walk_speed: f32,
    pub run_speed: f32,
    pub arm_swing: f32,
    pub leg_swing: f32,
    pub torso_turn: f32,
    pub pose_response: f32,
    pub run_crouch: f32,
    pub elbow_drive: f32,
    pub step_lift: f32,
    /// Distance between alternating foot contacts in world units.
    pub step_distance: f32,
    /// Use the planted-foot IK gait for a rig with articulated hips and ankles.
    pub plant_feet: bool,
    /// Hip-to-ankle reach for that gait, in unscaled actor units.
    pub leg_reach: f32,
    /// Angular spring frequency in radians per second. Lower values keep turning momentum longer.
    pub turn_inertia: f32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum MotionStyle {
    #[default]
    Classic,
    Natural,
}

impl Default for CharacterMotion {
    fn default() -> Self {
        Self {
            motion_style: MotionStyle::Classic,
            walk_speed: 3.8,
            run_speed: 5.6,
            arm_swing: 0.23,
            leg_swing: 0.48,
            torso_turn: 0.42,
            pose_response: 19.0,
            run_crouch: 0.13,
            elbow_drive: 0.38,
            step_lift: 0.10,
            step_distance: 0.50,
            plant_feet: false,
            leg_reach: 0.48,
            turn_inertia: 8.0,
        }
    }
}

/// A character file as written: every field optional, so a `base` can fill
/// what a variant leaves out.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CharacterFile {
    #[serde(default)]
    pub base: Option<String>,
    pub sim: SimFile,
    pub view: ViewFile,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ViewFile {
    skin: Option<String>,
    actor: Option<String>,
    motion: Option<MotionFile>,
}

/// [`CharacterMotion`] without the speeds, which come from `movement`.
#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct MotionFile {
    motion_style: Option<MotionStyle>,
    arm_swing: Option<f32>,
    leg_swing: Option<f32>,
    torso_turn: Option<f32>,
    pose_response: Option<f32>,
    run_crouch: Option<f32>,
    elbow_drive: Option<f32>,
    step_lift: Option<f32>,
    step_distance: Option<f32>,
    plant_feet: Option<bool>,
    leg_reach: Option<f32>,
    turn_inertia: Option<f32>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct SimFile {
    brain: Option<Brain>,
    body: Option<BodyFile>,
    movement: Option<MovementFile>,
    health: Option<HealthFile>,
    avatar: Option<AvatarFile>,
    carry: Option<CarryFile>,
    /// Replaced as a whole, not merged.
    abilities: Option<Vec<AbilityFile>>,
    attack: Option<ThreatAttackFile>,
}

/// A FollowRoute character's `attack`: damage, meters, 60 Hz frames and m/s.
#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct ThreatAttackFile {
    damage: Option<u32>,
    reach: Option<Fx>,
    windup: Option<u64>,
    cooldown: Option<u64>,
    knockback: Option<Fx>,
}

/// Frames, meters and per-second rates.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
enum AbilityFile {
    Jump { prep: u64, land: u64, velocity: Fx, gravity: Fx },
    Jetpack { hover: Fx, climb: Fx, sink: Fx },
    Dig { time: u64, centering: Fx },
    SwiftSword,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct CarryFile {
    /// 60 Hz frames.
    transfer: Option<u64>,
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

merge_fields!(SimFile { brain, abilities } { body, movement, health, avatar, carry, attack });
merge_fields!(ThreatAttackFile { damage, reach, windup, cooldown, knockback } {});
merge_fields!(CarryFile { transfer } {});
merge_fields!(BodyFile { radius } {});
merge_fields!(MovementFile { walk, run } {});
merge_fields!(HealthFile { hp, armor, breach_damage } {});
merge_fields!(AvatarFile { acceleration, braking, attack_braking } { attack });
merge_fields!(AttackFile { windup, aim, swing, sword_windup, sword_swing, rebound, combo_window } {});
merge_fields!(ViewFile { skin, actor } { motion });
merge_fields!(MotionFile {
    motion_style, arm_swing, leg_swing, torso_turn, pose_response, run_crouch,
    elbow_drive, step_lift, step_distance, plant_feet, leg_reach, turn_inertia
} {});

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
    let carry = file
        .carry
        .map(|c| {
            let transfer = need(c.transfer, "carry.transfer")?;
            if !(3..=300).contains(&transfer) {
                return Err("carry.transfer must be 3..=300 frames".to_string());
            }
            Ok(CarryDef { transfer: ticks_from_60hz(transfer) })
        })
        .transpose()?;
    let abilities = file.abilities.unwrap_or_default().into_iter().map(ability_def).collect::<Result<Vec<_>, _>>()?;
    let kind = |a: &Ability| std::mem::discriminant(a);
    if abilities.iter().enumerate().any(|(i, a)| abilities[..i].iter().any(|b| kind(a) == kind(b))) {
        return Err("abilities lists an ability twice".into());
    }
    if abilities.iter().any(|a| matches!(a, Ability::Jump(_))) && abilities.iter().any(|a| matches!(a, Ability::Jetpack(_))) {
        return Err("a character can't have both Jump and Jetpack: both take the lift button".into());
    }
    if brain != Brain::Player && !abilities.is_empty() {
        return Err("only Player characters have abilities".into());
    }
    match brain {
        Brain::Player if avatar.is_none() || carry.is_none() || health.is_some() => {
            return Err("a Player character needs `avatar` and `carry`, and no `health` (player health is a scenario rule)".into())
        }
        Brain::FollowRoute if health.is_none() || avatar.is_some() || carry.is_some() => {
            return Err("a FollowRoute character needs `health`, and no `avatar` or `carry`".into())
        }
        Brain::Hauler if carry.is_none() || avatar.is_some() || health.is_some() => {
            return Err("a Hauler character needs `carry`, and no `avatar` or `health`".into())
        }
        _ => {}
    }
    let attack = file.attack.map(attack_def).transpose()?;
    if attack.is_some() && brain != Brain::FollowRoute {
        return Err("only FollowRoute characters have `attack`".into());
    }
    Ok(CharacterDef { brain, body: BodyDef { radius }, movement: MovementDef { walk, run }, health, avatar, carry, abilities, attack })
}

fn attack_def(file: ThreatAttackFile) -> Result<AttackDef, String> {
    let damage = need(file.damage, "attack.damage")?;
    if !(1..=1000).contains(&damage) {
        return Err("attack.damage must be in 1..=1000".into());
    }
    let reach = need(file.reach, "attack.reach")?;
    if !within(reach, Fx::from_ratio(1, 10), Fx::from_int(8)) {
        return Err("attack.reach must be in [0.1, 8] m".into());
    }
    let windup = need(file.windup, "attack.windup")?;
    if !(1..=600).contains(&windup) {
        return Err("attack.windup must be 1..=600 frames".into());
    }
    let cooldown = need(file.cooldown, "attack.cooldown")?;
    if cooldown > 600 {
        return Err("attack.cooldown must be 0..=600 frames".into());
    }
    let knockback = need(file.knockback, "attack.knockback")?;
    if !within(knockback, Fx::ZERO, Fx::from_int(40)) {
        return Err("attack.knockback must be in [0, 40] m/s".into());
    }
    Ok(AttackDef { damage, reach, windup: ticks_from_60hz(windup), cooldown: ticks_from_60hz(cooldown), knockback })
}

/// Checks a merged view against its character's definition.
pub(crate) fn character_view(file: ViewFile, def: &CharacterDef) -> Result<CharacterView, String> {
    let player = def.brain == Brain::Player;
    match &file.skin {
        Some(_) if player => return Err("a Player character is drawn by its view.actor, not view.skin".into()),
        Some(skin) if !crate::scenario::ident(skin) => return Err("view.skin must be a skin id".into()),
        None if !player => return Err("missing view.skin (here or in a base)".into()),
        _ => {}
    }
    match &file.actor {
        Some(_) if !player => return Err("only Player characters have view.actor".into()),
        Some(actor) if !crate::scenario::ident(actor) => return Err("view.actor must be an actor name".into()),
        None if player => return Err("missing view.actor (here or in a base)".into()),
        _ => {}
    }
    if file.motion.is_some() && !player {
        return Err("only Player characters have view.motion".into());
    }
    let m = file.motion.unwrap_or_default();
    let d = CharacterMotion::default();
    let motion = CharacterMotion {
        motion_style: m.motion_style.unwrap_or(d.motion_style),
        walk_speed: def.movement.walk.to_f32(),
        run_speed: def.movement.run.to_f32(),
        arm_swing: m.arm_swing.unwrap_or(d.arm_swing),
        leg_swing: m.leg_swing.unwrap_or(d.leg_swing),
        torso_turn: m.torso_turn.unwrap_or(d.torso_turn),
        pose_response: m.pose_response.unwrap_or(d.pose_response),
        run_crouch: m.run_crouch.unwrap_or(d.run_crouch),
        elbow_drive: m.elbow_drive.unwrap_or(d.elbow_drive),
        step_lift: m.step_lift.unwrap_or(d.step_lift),
        step_distance: m.step_distance.unwrap_or(d.step_distance),
        plant_feet: m.plant_feet.unwrap_or(d.plant_feet),
        leg_reach: m.leg_reach.unwrap_or(d.leg_reach),
        turn_inertia: m.turn_inertia.unwrap_or(d.turn_inertia),
    };
    let tuning = [
        motion.arm_swing, motion.leg_swing, motion.torso_turn, motion.pose_response, motion.run_crouch,
        motion.elbow_drive, motion.step_lift, motion.step_distance, motion.turn_inertia,
    ];
    if tuning.iter().any(|v| !v.is_finite() || !(0.0..=100.0).contains(v)) {
        return Err("view.motion values must be in [0, 100]".into());
    }
    if !(0.2..=2.0).contains(&motion.leg_reach) {
        return Err("view.motion.leg_reach must be in [0.2, 2]".into());
    }
    Ok(CharacterView { skin: file.skin, actor: file.actor, motion })
}

fn ability_def(file: AbilityFile) -> Result<Ability, String> {
    let frames = |value: u64, field: &str| -> Result<u64, String> {
        if !(1..=600).contains(&value) {
            return Err(format!("{field} must be 1..=600 frames"));
        }
        Ok(ticks_from_60hz(value))
    };
    // `tenths` is the lower bound in tenths, so 5 means 0.5.
    let between = |value: Fx, tenths: i64, hi: i32, field: &str| -> Result<Fx, String> {
        if !within(value, Fx::from_ratio(tenths, 10), Fx::from_int(hi)) {
            return Err(format!("{field} must be in [{}.{}, {hi}]", tenths / 10, tenths % 10));
        }
        Ok(value)
    };
    Ok(match file {
        AbilityFile::Jump { prep, land, velocity, gravity } => Ability::Jump(JumpDef {
            prep: frames(prep, "Jump.prep")?,
            land: frames(land, "Jump.land")?,
            velocity: per_tick(between(velocity, 5, 20, "Jump.velocity (m/s)")?),
            gravity: per_tick(per_tick(between(gravity, 10, 100, "Jump.gravity (m/s²)")?)),
        }),
        AbilityFile::Jetpack { hover, climb, sink } => Ability::Jetpack(JetpackDef {
            hover: between(hover, 5, 5, "Jetpack.hover (m)")?,
            climb: per_tick(between(climb, 5, 20, "Jetpack.climb (m/s)")?),
            sink: per_tick(between(sink, 5, 20, "Jetpack.sink (m/s)")?),
        }),
        AbilityFile::Dig { time, centering } => Ability::Dig(DigDef {
            ticks: frames(time, "Dig.time")?,
            centering: per_tick(between(centering, 1, 15, "Dig.centering (m/s)")?),
        }),
        AbilityFile::SwiftSword => Ability::SwiftSword,
    })
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

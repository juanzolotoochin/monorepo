//! Versioned commands used by scripts, live clients and tests.
use serde::{Deserialize, Serialize};
use voxel_assets::Mineral;

pub const VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Inspect,
    InspectBuilding {
        building: Option<String>,
    },
    SettlementTransfer {
        depot: String,
        item: String,
        units: u32,
        deposit: bool,
    },
    SettlementCancel,
    SettlementPause {
        paused: bool,
    },
    SettlementRate {
        rate: f32,
    },
    SettlementLink {
        id: String,
        enabled: bool,
    },
    SettlementMachine {
        id: String,
        enabled: bool,
    },
    SettlementDefender {
        id: String,
        enabled: bool,
    },
    SettlementStock {
        depot: String,
        item: String,
        units: u32,
    },
    /// Gives a sprite (by id) a job: a building kind (by id) to staff, or
    /// idle when `job` is missing or null.
    SettlementSpriteJob {
        sprite: String,
        #[serde(default)]
        job: Option<String>,
    },
    /// The player tops up their bundle from building `building`'s output
    /// (its index), as many units as fit, walking there first.
    SettlementPickUp {
        building: u16,
    },
    /// The player empties their bundle into a depot (by id), walking there.
    SettlementDeposit {
        depot: String,
    },
    /// The player fills their bundle from a depot (by id), walking there.
    SettlementWithdraw {
        depot: String,
    },
    /// Reports the player's interact target in the receipt's state
    /// (`settlement.interact`); with `prompt`, fails unless the target's
    /// prompt is that ("PICK UP 3 WOOD"), or there is none and it is empty.
    SettlementInteractTarget {
        #[serde(default)]
        prompt: Option<String>,
    },
    /// Interacts as E or A does, on the same target: a bundle target's units
    /// move (the elder steps in to it), and a sprite opens its job menu.
    SettlementInteract,
    /// Picks the open job menu's option for `job` (a building kind's id, or
    /// idle when missing), as the menu's own pick does.
    SettlementJobMenu {
        #[serde(default)]
        job: Option<String>,
    },
    /// The town's build key: with no `kind`, opens the build menu as B or X
    /// does; with a building kind's id, picks it from the open build menu
    /// (or, with none open, starts placing it). Its ghost then follows the
    /// player, and the receipt's `settlement.interact.ghost` says where it
    /// stands and whether it may go down there (`reason` if not).
    SettlementBuild {
        #[serde(default)]
        kind: Option<String>,
    },
    /// Puts a blueprint down. With `position` (arena x and z), a blueprint
    /// of `kind` there; without, the ghost being placed, as interact does.
    /// Fails with the sim's reason when it can't go down, such as "BLOCKED".
    SettlementPlaceBlueprint {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        position: Option<[f32; 2]>,
    },
    /// Backs out of placing a blueprint, as Escape does.
    SettlementCancelBuild,
    /// Possesses building `building` (its index), as interacting with its
    /// slot or the boarding key does: walks the elder beside it first when
    /// it is out of reach, then boards, putting out a sprite inside. Fails
    /// with the sim's reason ("Drop the bundle first (Q)"); done once the
    /// elder sits in it (`settlement.seat`).
    SettlementBoard {
        building: u16,
    },
    /// Leaves the building the elder possesses, as interacting or the
    /// boarding key does; done once it stands beside it.
    SettlementLeave,
    /// Drops the bundle on the ground at the player's feet, as Q or B does:
    /// onto a pile of its item there, or a new one. Fails with the sim's
    /// reason, such as "Nothing carried"; done once every unit has landed.
    SettlementDrop,
    /// Bursts the building the elder possesses, as R or RT does there: its
    /// Burst units go into the output at once. Fails with the sim's reason
    /// ("Burst ready in 12 s", "Output full", "Possess a building to
    /// Burst"); done once the units are in (`settlement.burst`).
    SettlementBurst,
    /// Fires the ballista the elder possesses once, as left mouse or RT
    /// does there: the reticle goes `x`, `y` meters from the ballista (cut
    /// to its range) and a bolt flies there. Fails with the sim's reason
    /// ("No ammo: bring stone", "Reloading", "Possess a ballista to
    /// fire"); done once the shot has been simulated.
    SettlementFire {
        x: f32,
        y: f32,
    },
    /// Throws the fireball of the ballista the elder possesses, as R or RB
    /// does there: the reticle goes `x`, `y` meters from the ballista (cut
    /// to its range) and the fireball flies there, for no stone. Fails with
    /// the sim's reason ("Fireball ready in 12 s", "Possess a building to
    /// Burst"); done once the elder's fireball cooldown has started
    /// (`settlement.burst`).
    SettlementFireball {
        x: f32,
        y: f32,
    },
    AssertSettlement {
        delivered: u64,
        cycles: u64,
        equipped: u64,
        kills: u64,
    },
    MemoryRecord,
    MemoryDeploy,
    MemoryClear,
    AssertTrial {
        unlocked: bool,
        collected: bool,
    },
    Jetpack {
        enabled: bool,
    },
    /// Climbs into the nearest vehicle, or out of the one driven.
    Board,
    /// Holds the movement stick this way (arena x and z, length up to 1);
    /// a driver steers their vehicle with it.
    Drive {
        direction: [f32; 2],
        #[serde(default)]
        run: bool,
    },
    /// Digs out a layer under the player, in a digging scene.
    Dig,
    /// Walks to the collector and empties the cargo into it.
    Deliver {
        #[serde(default)]
        seconds_per_unit: f32,
    },
    AssertFlight {
        airborne: bool,
        altitude: f32,
        #[serde(default = "flight_tolerance")]
        tolerance: f32,
    },
    Scene {
        scene: String,
    },
    Walk {
        position: [f32; 2],
    },
    Add {
        dispenser: u32,
        units: u32,
    },
    Grab {
        pile: u32,
        units: u32,
        #[serde(default)]
        material: Option<Mineral>,
        #[serde(default)]
        seconds_per_unit: f32,
        #[serde(default)]
        approach: Option<[f32; 2]>,
    },
    DropAll {
        position: [f32; 2],
        #[serde(default)]
        seconds_per_unit: f32,
        #[serde(default)]
        approach: Option<[f32; 2]>,
    },
    DepositAll {
        pile: u32,
        #[serde(default)]
        seconds_per_unit: f32,
        #[serde(default)]
        approach: Option<[f32; 2]>,
    },
    Strike {
        target: [f32; 2],
    },
    EquipWeapon {
        weapon: String,
    },
    Wait {
        frames: u32,
    },
    Assert {
        #[serde(default)]
        cargo_units: Option<u32>,
        #[serde(default)]
        total_units: Option<u64>,
        #[serde(default)]
        piles: Vec<PileCheck>,
    },
    Quit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PileCheck {
    pub id: u32,
    pub units: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub version: u32,
    pub scene: String,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub max_frames: u32,
    pub steps: Vec<Command>,
}
impl Script {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("Unsupported demo script version".into());
        }
        if self.scene.is_empty() || !self.scene.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err("Invalid scene id".into());
        }
        if !(10..=60).contains(&self.fps)
            || !(640..=1920).contains(&self.width)
            || !(480..=1080).contains(&self.height)
            || self.width % 2 != 0
            || self.height % 2 != 0
            || !(1..=7200).contains(&self.max_frames)
            || self.steps.is_empty()
            || self.steps.len() > 1024
            || self.max_frames > self.fps * 120
            || self.steps.iter().any(|c| matches!(c, Command::Quit))
        {
            return Err("Invalid demo dimensions, FPS, frame limit or step count".into());
        }
        Ok(())
    }
}
pub fn point(position: [f32; 2]) -> Result<bevy::prelude::Vec3, String> {
    if position.iter().any(|v| !v.is_finite() || v.abs() > 128.0) {
        return Err("Position must be finite and inside the supported world bounds".into());
    }
    Ok(bevy::prelude::Vec3::new(position[0], 0.0, position[1]))
}

fn flight_tolerance() -> f32 {
    0.05
}

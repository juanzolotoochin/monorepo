//! Versioned commands used by scripts, live clients and tests.
use serde::{Deserialize, Serialize};
use voxel_assets::Mineral;

pub const VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Inspect,
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
        scene_index(&self.scene)?;
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
pub fn scene_index(name: &str) -> Result<usize, String> {
    match name {
        "mining-deck" => Ok(0),
        "boulder-lab" => Ok(1),
        "pile-lab" => Ok(2),
        "jetpack-lab" => Ok(3),
        "surprise-lab" => Ok(4),
        "atlantis" => Ok(5),
        "forest-outpost" => Ok(6),
        _ => Err(format!("Unknown scene {name}")),
    }
}
pub fn point(position: [f32; 2]) -> Result<bevy::prelude::Vec3, String> {
    if position.iter().any(|v| !v.is_finite() || v.abs() > 24.0) {
        return Err("Position must be finite and inside the supported world bounds".into());
    }
    Ok(bevy::prelude::Vec3::new(position[0], 0.0, position[1]))
}

fn flight_tolerance() -> f32 {
    0.05
}

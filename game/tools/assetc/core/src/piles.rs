//! Authored logistics fixtures. Units are discrete; each visual chunk is one unit.
use crate::Result;
use serde::{Deserialize, Serialize};

pub const PILE_CAPACITY: u32 = 96;
pub const CARRY_CAPACITY: u32 = 12;
pub const MAX_PILES: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mineral {
    AmberOre,
    BlueCrystal,
    /// Only found by digging, deep in a mine.
    GreenMalachite,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispenserLayout {
    pub material: Mineral,
    pub position: [f32; 3],
    pub output: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PileSeed {
    pub material: Mineral,
    pub position: [f32; 3],
    pub units: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PileLayout {
    pub capacity: u32,
    pub dispensers: Vec<DispenserLayout>,
    pub piles: Vec<PileSeed>,
    /// A machine that takes any mineral delivered to it.
    #[serde(default)]
    pub collector: Option<[f32; 3]>,
}
impl PileLayout {
    pub fn validate(&self) -> Result<()> {
        if !(1..=CARRY_CAPACITY).contains(&self.capacity)
            || self.dispensers.len() > 4
            || self.piles.len() + self.dispensers.len() > MAX_PILES
        {
            return Err("invalid pile layout capacity or fixture count".into());
        }
        let position = |p: &[f32; 3]| {
            p.iter().all(|v| v.is_finite()) && p[0].abs() <= 6.5 && p[2].abs() <= 6.5 && p[1] == 0.0
        };
        if self
            .dispensers
            .iter()
            .any(|d| !position(&d.position) || !position(&d.output))
            || self
                .piles
                .iter()
                .any(|p| !position(&p.position) || !(1..=PILE_CAPACITY).contains(&p.units))
            || self.collector.is_some_and(|c| !position(&c))
        {
            return Err("pile fixtures must be on the deck and quantities within capacity".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_quantities_capacity_and_off_deck_fixtures_are_rejected() {
        let mut layout = PileLayout {
            capacity: 12,
            dispensers: vec![DispenserLayout {
                material: Mineral::AmberOre,
                position: [-3.0, 0.0, 0.0],
                output: [0.0, 0.0, 0.0],
            }],
            piles: vec![PileSeed {
                material: Mineral::BlueCrystal,
                position: [3.0, 0.0, 0.0],
                units: 24,
            }],
            collector: None,
        };
        assert!(layout.validate().is_ok());
        layout.capacity = 13;
        assert!(layout.validate().is_err());
        layout.capacity = 12;
        layout.piles[0].units = 97;
        assert!(layout.validate().is_err());
        layout.piles[0].units = 24;
        layout.dispensers[0].output[0] = f32::NAN;
        assert!(layout.validate().is_err());
        layout.dispensers[0].output = [0.0, 1.0, 0.0];
        assert!(layout.validate().is_err());
    }
}

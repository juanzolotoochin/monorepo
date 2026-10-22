//! Quantity bookkeeping without rendering, input, or frame timing.
use std::collections::BTreeMap;
use voxel_assets::{Mineral, PileLayout, MAX_PILES, PILE_CAPACITY};

pub type PileId = u32;
#[derive(Clone, Debug)]
pub struct Pile {
    pub material: Mineral,
    pub position: [f32; 2],
    pub units: u32,
    pub source: bool,
}
#[derive(Clone, Debug)]
pub struct Cargo {
    pub material: Option<Mineral>,
    pub units: u32,
    pub capacity: u32,
}
#[derive(Clone, Debug)]
pub struct Stock {
    pub piles: BTreeMap<PileId, Pile>,
    pub cargo: Cargo,
    pub created: u64,
    next_id: PileId,
}
impl Stock {
    pub fn new(layout: &PileLayout) -> Self {
        // Layouts are validated by the asset compiler and again on decoding.
        let mut stock = Self {
            piles: BTreeMap::new(),
            cargo: Cargo {
                material: None,
                units: 0,
                capacity: layout.capacity,
            },
            created: 0,
            next_id: 0,
        };
        for d in &layout.dispensers {
            stock.insert(d.material, [d.output[0], d.output[2]], 0, true);
        }
        for p in &layout.piles {
            stock.insert(p.material, [p.position[0], p.position[2]], p.units, false);
            stock.created += u64::from(p.units);
        }
        stock
    }
    fn insert(
        &mut self,
        material: Mineral,
        position: [f32; 2],
        units: u32,
        source: bool,
    ) -> PileId {
        let id = self.next_id;
        self.next_id += 1;
        self.piles.insert(
            id,
            Pile {
                material,
                position,
                units,
                source,
            },
        );
        id
    }
    pub fn add(&mut self, id: PileId, requested: u32) -> Result<u32, &'static str> {
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if !pile.source {
            return Err("Only dispensers create mineral");
        }
        let amount = requested.min(PILE_CAPACITY - pile.units);
        if amount == 0 {
            return Err("Output pile is full (96 units)");
        }
        pile.units += amount;
        self.created += u64::from(amount);
        Ok(amount)
    }
    pub fn pickup(&mut self, id: PileId) -> Result<u32, &'static str> {
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if self.cargo.material.is_some_and(|m| m != pile.material) {
            return Err("Unload before collecting a different mineral");
        }
        let amount = pile.units.min(self.cargo.capacity - self.cargo.units);
        if amount == 0 {
            return Err(if pile.units == 0 {
                "This pile is empty"
            } else {
                "Carrier is full"
            });
        }
        self.cargo.material = Some(pile.material);
        self.cargo.units += amount;
        pile.units -= amount;
        if pile.units == 0 && !pile.source {
            self.piles.remove(&id);
        }
        Ok(amount)
    }
    pub fn deposit(&mut self, id: PileId) -> Result<u32, &'static str> {
        let material = self.cargo.material.ok_or("Carrier is empty")?;
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if pile.material != material {
            return Err("Different minerals stay in separate piles");
        }
        let amount = self.cargo.units.min(PILE_CAPACITY - pile.units);
        if amount == 0 {
            return Err("Destination pile is full (96 units)");
        }
        pile.units += amount;
        self.cargo.units -= amount;
        if self.cargo.units == 0 {
            self.cargo.material = None;
        }
        Ok(amount)
    }
    pub fn drop_new(&mut self, position: [f32; 2]) -> Result<PileId, &'static str> {
        let material = self.cargo.material.ok_or("Carrier is empty")?;
        if position.iter().any(|v| !v.is_finite() || v.abs() > 6.5) {
            return Err("Drop position must be on the deck");
        }
        if self.piles.len() >= MAX_PILES {
            return Err("Pile limit reached; deposit into an existing pile");
        }
        let units = self.cargo.units;
        let id = self.insert(material, position, units, false);
        self.cargo.units = 0;
        self.cargo.material = None;
        Ok(id)
    }
    pub fn total(&self) -> u64 {
        self.piles.values().map(|p| u64::from(p.units)).sum::<u64>() + u64::from(self.cargo.units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel_assets::{DispenserLayout, PileSeed};
    fn stock() -> Stock {
        Stock::new(&PileLayout {
            capacity: 12,
            dispensers: vec![DispenserLayout {
                material: Mineral::AmberOre,
                position: [-4.0, 0.0, 0.0],
                output: [-2.0, 0.0, 0.0],
            }],
            piles: vec![PileSeed {
                material: Mineral::BlueCrystal,
                position: [3.0, 0.0, 0.0],
                units: 24,
            }],
        })
    }
    #[test]
    fn transfers_conserve_units_respect_capacity_and_keep_materials_separate() {
        let mut s = stock();
        assert_eq!(s.add(0, 20), Ok(20));
        assert_eq!(s.pickup(0), Ok(12));
        assert_eq!(s.piles[&0].units, 8);
        let before = s.total();
        assert!(s.pickup(1).is_err());
        assert!(s.deposit(1).is_err());
        assert_eq!(s.total(), before);
        let destination = s.drop_new([1.0, 2.0]).unwrap();
        assert_eq!(s.piles[&destination].units, 12);
        assert_eq!(s.cargo.units, 0);
        assert_eq!(s.pickup(0), Ok(8));
        assert_eq!(s.deposit(destination), Ok(8));
        assert_eq!(s.piles[&destination].units, 20);
        assert!(s.deposit(destination).is_err());
        assert_eq!(s.total(), s.created);
    }
    #[test]
    fn partial_transfers_and_empty_pile_removal_never_destroy_material() {
        let mut s = stock();
        s.add(0, 200).unwrap();
        assert_eq!(s.piles[&0].units, PILE_CAPACITY);
        s.pickup(0).unwrap();
        let id = s.drop_new([1.0, 2.0]).unwrap();
        s.add(0, 200).unwrap();
        s.pickup(id).unwrap();
        assert!(!s.piles.contains_key(&id));
        assert!(s.deposit(0).is_err());
        assert_eq!(s.cargo.units, 12);
        let id = s.drop_new([1.0, 2.0]).unwrap();
        s.pickup(0).unwrap();
        s.deposit(id).unwrap();
        s.pickup(id).unwrap();
        // Source has room for twelve; deposit part, then collect remaining cargo.
        s.add(0, 7).unwrap();
        assert_eq!(s.deposit(0), Ok(5));
        assert_eq!(s.cargo.units, 7);
        assert_eq!(s.total(), s.created);
    }
}

/// Stable, supported mound packing: every chunk has a chunk directly below it.
/// Filling a prefix grows the same mound instead of reshuffling it each transfer.
pub fn unit_positions() -> &'static Vec<[f32; 3]> {
    static POSITIONS: std::sync::OnceLock<Vec<[f32; 3]>> = std::sync::OnceLock::new();
    POSITIONS.get_or_init(|| {
        let mut cells: Vec<(i32, i32, i32)> = (0..6)
            .flat_map(|y| (-3_i32..=3).flat_map(move |x| (-3_i32..=3).map(move |z| (x, y, z))))
            .collect();
        cells.sort_by_key(|&(x, y, z)| (x.abs().max(z.abs()) + y, y, x * x + z * z, x, z));
        cells
            .into_iter()
            .take(PILE_CAPACITY as usize)
            .map(|(x, y, z)| [x as f32 * 0.28, y as f32 * 0.23 + 0.14, z as f32 * 0.28])
            .collect()
    })
}
pub fn pile_radius(units: u32) -> f32 {
    unit_positions()
        .iter()
        .take(units as usize)
        .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt() + 0.18)
        .fold(0.35, f32::max)
}

#[cfg(test)]
mod packing_tests {
    use super::*;
    #[test]
    fn all_visual_slots_are_unique_and_supported_in_every_prefix() {
        let positions = unit_positions();
        assert_eq!(positions.len(), PILE_CAPACITY as usize);
        let mut occupied = std::collections::BTreeSet::new();
        for p in positions {
            let cell = (
                (p[0] / 0.28).round() as i32,
                ((p[1] - 0.14) / 0.23).round() as i32,
                (p[2] / 0.28).round() as i32,
            );
            if cell.1 > 0 {
                assert!(occupied.contains(&(cell.0, cell.1 - 1, cell.2)));
            }
            assert!(occupied.insert(cell));
        }
    }
}

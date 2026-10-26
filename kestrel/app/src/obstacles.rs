//! Everything the simulation treats as a blocking prop in a scene, from the
//! scene's data and the constants below. `lockstep::build_world` turns it
//! into the world's terrain, so a world can be built before any view exists.
//! The views spawn their `Obstacle` components from the same constants.
use bevy::prelude::Vec3;
use voxel_assets::{Instance, SceneFeature, ScenePack};

use crate::navigation::Disc;

/// A scene's obstacles in arena coordinates, in a fixed order: architecture,
/// boulders, industrial props, pile fixtures, the memory trial's wall.
pub struct SceneObstacles {
    pub discs: Vec<Disc>,
    /// The memory trial's doors, which open when the trial is solved.
    pub doors: Vec<Disc>,
}

/// The industrial deck's coolant relay.
pub const REACTOR: Disc = Disc { center: Vec3::new(-5.0, 0.0, -5.3), radius: 1.45 };
/// The industrial deck's bulkheads along the back wall.
pub const BULKHEADS: [Disc; 4] = [
    Disc { center: Vec3::new(-7.7, 0.0, -7.4), radius: 0.72 },
    Disc { center: Vec3::new(-3.0, 0.0, -7.4), radius: 0.72 },
    Disc { center: Vec3::new(1.5, 0.0, -7.4), radius: 0.72 },
    Disc { center: Vec3::new(6.7, 0.0, -7.4), radius: 0.72 },
];
/// The industrial deck's supply crates.
pub const CRATES: [Disc; 3] = [
    Disc { center: Vec3::new(-6.7, 0.0, -0.9), radius: 0.66 },
    Disc { center: Vec3::new(6.5, 0.0, 0.3), radius: 0.66 },
    Disc { center: Vec3::new(-3.2, 0.0, 5.7), radius: 0.66 },
];
pub const DISPENSER_RADIUS: f32 = 0.8;
pub const COLLECTOR_RADIUS: f32 = 0.9;

/// The memory trial's wall, one block every 0.45 m along x = 2, and whether
/// each block is a door.
pub fn memory_wall() -> impl Iterator<Item = (Disc, bool)> {
    (-18..=18).map(|n| {
        let z = n as f32 * 0.45;
        (Disc { center: Vec3::new(2.0, 0.0, z), radius: 0.24 }, z.abs() < 1.15)
    })
}

/// The scene's obstacles. `collision_radius` gives an instance's model
/// radius before scaling; runtime boulders only know it once generated.
pub fn scene_obstacles(pack: &ScenePack, collision_radius: impl Fn(&Instance) -> f32) -> SceneObstacles {
    let scene = &pack.scene;
    let mut discs = Vec::new();
    let mut doors = Vec::new();
    if let Some(environment) = &pack.environment {
        discs.extend(
            environment.obstacles.iter().map(|&(position, radius)| Disc { center: Vec3::from_array(position), radius }),
        );
    }
    for instance in &scene.instances {
        discs.push(Disc {
            center: Vec3::from_array(instance.position),
            radius: collision_radius(instance) * instance.scale,
        });
    }
    if scene.industrial {
        discs.push(REACTOR);
        discs.extend(BULKHEADS);
        discs.extend(CRATES);
    }
    if let Some(piles) = &scene.piles {
        discs.extend(
            piles.dispensers.iter().map(|d| Disc { center: Vec3::from_array(d.position), radius: DISPENSER_RADIUS }),
        );
        if let Some(at) = piles.collector {
            discs.push(Disc { center: Vec3::from_array(at), radius: COLLECTOR_RADIUS });
        }
    }
    if scene.features.contains(&SceneFeature::MemoryTrial) {
        for (disc, door) in memory_wall() {
            if door {
                doors.push(disc);
            } else {
                discs.push(disc);
            }
        }
    }
    SceneObstacles { discs, doors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::SceneLibrary;

    #[test]
    fn every_scene_has_its_obstacles() {
        let library = SceneLibrary::new();
        let mut counts: Vec<_> = library
            .scenes
            .iter()
            .map(|pack| {
                let found = scene_obstacles(pack, |_| 1.0);
                (pack.scene.id.as_str(), found.discs.len(), found.doors.len())
            })
            .collect();
        counts.sort();
        assert_eq!(
            counts,
            [
                ("asset-guide", 0, 0),
                ("atlantis", 21, 0),
                ("boulder-lab", 10, 0),
                ("combat-arena", 0, 0),
                ("dig-lab", 1, 0),
                ("field", 0, 0),
                ("forest-outpost", 0, 0),
                ("jetpack-lab", 0, 0),
                ("laser-pit", 110, 0),
                ("mining-deck", 14, 0),
                ("moonlit-glade", 24, 0),
                ("pile-lab", 2, 0),
                ("surprise-lab", 32, 5),
            ]
        );
    }
}

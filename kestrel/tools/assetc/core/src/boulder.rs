use crate::{
    BoulderRecipe, Collision, CollisionLayer, MaterialPattern, MeshData, Model, Result, Statistics,
};
use std::collections::{BTreeMap, BTreeSet};

fn hash(x: i32, y: i32, z: i32) -> u32 {
    let mut n = (x as u32).wrapping_mul(374761393)
        ^ (y as u32).wrapping_mul(668265263)
        ^ (z as u32).wrapping_mul(2246822519);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}
fn quad(mesh: &mut MeshData, points: [[f32; 3]; 4], axis: usize, sign: i8) {
    let base = mesh.positions.len() as u32;
    let mut normal = [0.0; 3];
    normal[axis] = sign as f32;
    mesh.positions.extend(points);
    mesh.normals.extend([normal; 4]);
    let order = if sign > 0 {
        [0, 1, 2, 0, 2, 3]
    } else {
        [0, 2, 1, 0, 3, 2]
    };
    mesh.indices.extend(order.map(|i| base + i));
}
fn box_mesh(mesh: &mut MeshData, center: [f32; 3], size: [f32; 3]) {
    for axis in 0..3 {
        for sign in [-1_i8, 1] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut points = [center; 4];
            for (point, (du, dv)) in
                points
                    .iter_mut()
                    .zip([(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)])
            {
                point[axis] += sign as f32 * size[axis] * 0.5;
                point[u] += du * size[u] * 0.5;
                point[v] += dv * size[v] * 0.5;
            }
            quad(mesh, points, axis, sign);
        }
    }
}

pub fn generate_boulder(recipe: &BoulderRecipe) -> Result<Model> {
    recipe.validate()?;
    let step = recipe.voxel_size;
    let extent = ((recipe.radius + 0.09) / step).ceil() as i32;
    let layers = (recipe.height / step).floor() as i32;
    let mut voxels = BTreeMap::new();
    let mut collision = Collision {
        radius: 0.0,
        layers: (0..layers)
            .map(|y| CollisionLayer {
                bottom: y as f32 * step,
                top: (y + 1) as f32 * step,
                radius: 0.0,
            })
            .collect(),
    };
    for y in 0..layers {
        for x in -extent..=extent {
            for z in -extent..=extent {
                let h = hash(x.wrapping_add((recipe.seed as i32).wrapping_mul(13)), y, z);
                let ratio = y as f32 * step / recipe.height;
                let radius =
                    recipe.radius * (1.0 - ratio * ratio * recipe.taper) + (h % 3) as f32 * 0.045;
                let (px, pz) = (x as f32 * step, z as f32 * step * 1.08);
                if px * px + pz * pz > radius * radius {
                    continue;
                }
                let material = if matches!(recipe.material_pattern, MaterialPattern::Quadrants) {
                    usize::from(x >= 0) + 2 * usize::from(z >= 0)
                } else if h % recipe.ore_every == 0 {
                    3
                } else if h % 7 == 0 {
                    2
                } else if h % 4 == 0 {
                    1
                } else {
                    0
                };
                voxels.insert([x, y, z], material);
                let dx = (x as f32).abs() * step + step * 0.5;
                let dz = (z as f32).abs() * step + step * 0.5;
                let radial = (dx * dx + dz * dz).sqrt();
                collision.radius = collision.radius.max(radial);
                collision.layers[y as usize].radius =
                    collision.layers[y as usize].radius.max(radial);
            }
        }
    }
    if voxels.is_empty() {
        return Err("recipe generated no occupied cells".into());
    }
    // Group exposed cells by plane, outward normal and material. Internal faces
    // disappear even where adjacent cells have different materials.
    let mut faces: BTreeMap<(usize, i8, i32, usize), BTreeSet<(i32, i32)>> = BTreeMap::new();
    let mut exposed = 0;
    for (&cell, &material) in &voxels {
        for axis in 0..3 {
            for sign in [-1_i8, 1] {
                let mut neighbor = cell;
                neighbor[axis] += sign as i32;
                if voxels.contains_key(&neighbor) {
                    continue;
                }
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                faces
                    .entry((axis, sign, cell[axis] * 2 + sign as i32, material))
                    .or_default()
                    .insert((cell[u], cell[v]));
                exposed += 1;
            }
        }
    }
    let mut meshes: Vec<_> = (0..4)
        .map(|material| MeshData {
            material,
            ..Default::default()
        })
        .collect();
    // Greedy rectangles merge coplanar faces without crossing material boundaries.
    for ((axis, sign, plane, material), mut cells) in faces {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        while let Some(&(x, y)) = cells.first() {
            let mut width = 1;
            while cells.contains(&(x + width, y)) {
                width += 1;
            }
            let mut height = 1;
            while (0..width).all(|i| cells.contains(&(x + i, y + height))) {
                height += 1;
            }
            for j in 0..height {
                for i in 0..width {
                    cells.remove(&(x + i, y + j));
                }
            }
            let mut points = [[0.0; 3]; 4];
            for (point, (du, dv)) in
                points
                    .iter_mut()
                    .zip([(0, 0), (width, 0), (width, height), (0, height)])
            {
                point[axis] = plane as f32 * 0.5 * step;
                point[u] = (x as f32 - 0.5 + du as f32) * step;
                point[v] = (y as f32 - 0.5 + dv as f32) * step;
                point[1] += step * 0.5;
            }
            quad(&mut meshes[material], points, axis, sign);
        }
    }
    const DIRECTIONS: [[f32; 2]; 8] = [
        [1.0, 0.0],
        [-0.737394, 0.675463],
        [0.087499, -0.996165],
        [0.608351, 0.793668],
        [-0.984688, -0.174327],
        [0.843854, -0.536573],
        [-0.259817, 0.965658],
        [-0.460679, -0.887567],
    ];
    for i in 0..recipe.debris as usize {
        let dir = DIRECTIONS[i % 8];
        let distance = recipe.radius + 0.30 + (i % 3) as f32 * 0.12 + (i / 8) as f32 * 0.20;
        box_mesh(
            &mut meshes[1],
            [dir[0] * distance, 0.075, dir[1] * distance],
            [0.20, 0.15, 0.20],
        );
    }
    let quads = meshes.iter().map(|m| m.indices.len() / 6).sum();
    let statistics = Statistics {
        voxels: voxels.len(),
        exposed_faces: exposed + recipe.debris as usize * 6,
        quads,
    };
    meshes.retain(|m| !m.positions.is_empty());
    Ok(Model {
        meshes,
        collision,
        statistics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_optimized_mesh_has_outward_winding_and_matching_collision() {
        let recipe = BoulderRecipe::default();
        let a = generate_boulder(&recipe).unwrap();
        let b = generate_boulder(&recipe).unwrap();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        assert!(a.statistics.quads < a.statistics.exposed_faces);
        assert!(a.statistics.exposed_faces < a.statistics.voxels * 6 + recipe.debris as usize * 6);
        assert!(a.collision.radius > a.collision.strike_radius_at(0.9));
        assert_eq!(a.collision.strike_radius_at(20.0), 0.0);
        assert_eq!(a.collision.strike_radius_at(-1.0), 0.0);
        for mesh in &a.meshes {
            for tri in mesh.indices.chunks_exact(3) {
                let p = mesh.positions[tri[0] as usize];
                let q = mesh.positions[tri[1] as usize];
                let r = mesh.positions[tri[2] as usize];
                let (u, v) = (
                    [q[0] - p[0], q[1] - p[1], q[2] - p[2]],
                    [r[0] - p[0], r[1] - p[1], r[2] - p[2]],
                );
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let n = mesh.normals[tri[0] as usize];
                assert!(cross.iter().zip(n).map(|(a, b)| a * b).sum::<f32>() > 0.0);
            }
        }
        let changed = generate_boulder(&BoulderRecipe { seed: 99, ..recipe }).unwrap();
        assert_ne!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&changed).unwrap()
        );
    }
    #[test]
    fn invalid_or_excessive_recipes_fail_before_allocating() {
        for recipe in [
            BoulderRecipe {
                radius: f32::NAN,
                ..Default::default()
            },
            BoulderRecipe {
                voxel_size: 0.0,
                ..Default::default()
            },
            BoulderRecipe {
                radius: 16.0,
                height: 16.0,
                voxel_size: 0.05,
                ..Default::default()
            },
            BoulderRecipe {
                ore_every: 0,
                ..Default::default()
            },
        ] {
            assert!(generate_boulder(&recipe).is_err());
        }
    }
}

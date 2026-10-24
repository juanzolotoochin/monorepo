//! Style-independent architectural recipes compiled onto a profile's physical voxel grid.
use crate::{Material, MeshData, Result, Statistics};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename = "Style", deny_unknown_fields)]
pub struct StyleProfile {
    pub id: String,
    pub voxel_size: f32,
    pub actor_voxel_size: Option<f32>,
    pub weapon_name: String,
    pub weapon_skins: Vec<WeaponSkin>,
    pub actor: Vec<ActorRecipe>,
    pub actor_scale: f32,
    pub skins: Vec<SkinRecipe>,
    pub materials: Vec<Material>,
    pub background: [f32; 3],
    pub ambient: [f32; 3],
    pub ambient_brightness: f32,
    pub sunlight: [f32; 3],
    pub sun_shadows: bool,
    pub illuminance: f32,
    pub accents: [[f32; 3]; 3],
    pub bloom: f32,
    pub fog_density: f32,
    pub viewport_height: f32,
    pub exposure_ev100: f32,
    pub shadow_depth_bias: f32,
    pub shadow_normal_bias: f32,
    pub shadow_map_size: u32,
    pub ao_thickness: f32,
}
impl Default for StyleProfile {
    fn default() -> Self {
        Self {
            id: "kestrel-industrial".into(),
            voxel_size: 0.3,
            actor_voxel_size: None,
            weapon_name: "MK. IV / INDUSTRIAL CLEAVER".into(),
            weapon_skins: vec![],
            actor: vec![],
            actor_scale: 1.0,
            skins: vec![],
            materials: vec![
                material("stone", [0.17, 0.22, 0.28], 0.0, 0.3),
                material("shadow", [0.065, 0.095, 0.14], 0.0, 0.35),
                material("metal", [0.31, 0.39, 0.43], 0.0, 0.7),
                material("magic", [0.1, 0.78, 1.0], 4.0, 0.0),
                material("water", [0.05, 0.2, 0.3], 0.3, 0.5),
                material("trim", [0.93, 0.63, 0.2], 0.0, 0.5),
            ],
            background: [0.018, 0.025, 0.045],
            ambient: [0.57, 0.7, 1.0],
            ambient_brightness: 180.0,
            sunlight: [1.0, 0.79, 0.63],
            sun_shadows: true,
            illuminance: 10500.0,
            accents: [[0.05, 0.65, 1.0], [1.0, 0.3, 0.08], [0.32, 0.58, 1.0]],
            bloom: 0.18,
            fog_density: 0.0,
            viewport_height: 21.5,
            exposure_ev100: 9.7,
            shadow_depth_bias: 0.08,
            shadow_normal_bias: 0.6,
            shadow_map_size: 1024,
            ao_thickness: 0.25,
        }
    }
}
fn material(name: &str, color: [f32; 3], glow: f32, metallic: f32) -> Material {
    Material {
        name: name.into(),
        color,
        emissive: color.map(|x| x * glow),
        metallic,
        roughness: 0.65,
        impact: match name {
            "metal" | "trim" => crate::ImpactResponse::Metal,
            "magic" => crate::ImpactResponse::Crystal,
            "water" => crate::ImpactResponse::Polymer,
            _ => crate::ImpactResponse::Stone,
        },
    }
}
impl StyleProfile {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 64
            || !self.voxel_size.is_finite()
            || !(0.05..=0.5).contains(&self.voxel_size)
            || self.materials.is_empty()
            || self.materials.len() > 32
        {
            return Err("invalid style identity, density or palette size".into());
        }
        if self.actor.len() > 8
            || self.weapon_name.trim().is_empty()
            || self.weapon_name.len() > 80
            || self
                .actor_voxel_size
                .is_some_and(|v| !v.is_finite() || !(0.05..=0.5).contains(&v))
        {
            return Err("invalid style actor density or weapon label".into());
        }
        if self.weapon_skins.len() > 8
            || self
                .weapon_skins
                .iter()
                .map(|w| w.weapon)
                .collect::<BTreeSet<_>>()
                .len()
                != self.weapon_skins.len()
            || self.weapon_skins.iter().any(|w| {
                !self.skins.iter().any(|s| s.id == w.skin)
                    || w.muzzle.iter().any(|v| !v.is_finite() || v.abs() > 4.0)
            })
        {
            return Err("invalid weapon skin mapping".into());
        }
        let mut parts = BTreeSet::new();
        if self.actor.iter().any(|a| !parts.insert(a.part)) {
            return Err("duplicate styled actor part".into());
        }
        if !self.shadow_map_size.is_power_of_two() || !(512..=4096).contains(&self.shadow_map_size)
        {
            return Err("invalid shadow map resolution".into());
        }
        let mut names = BTreeSet::new();
        for m in &self.materials {
            if !names.insert(&m.name)
                || m.name.is_empty()
                || m.color
                    .iter()
                    .chain(m.emissive.iter())
                    .any(|x| !x.is_finite() || *x < 0.0)
                || !m.metallic.is_finite()
                || !(0.0..=1.0).contains(&m.metallic)
                || !m.roughness.is_finite()
                || !(0.0..=1.0).contains(&m.roughness)
            {
                return Err("invalid style material".into());
            }
        }
        if self
            .background
            .iter()
            .chain(self.ambient.iter())
            .chain(self.sunlight.iter())
            .chain(self.accents.iter().flatten())
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
            || [
                (self.ambient_brightness, 0.0, 1000.0),
                (self.illuminance, 0.0, 100000.0),
                (self.bloom, 0.0, 1.0),
                (self.fog_density, 0.0, 0.1),
                (self.viewport_height, 12.5, 26.0),
                (self.exposure_ev100, 5.0, 16.0),
                (self.shadow_depth_bias, 0.0, 0.2),
                (self.shadow_normal_bias, 0.0, 2.0),
                (self.ao_thickness, 0.025, 1.0),
                (self.actor_scale, 0.25, 2.0),
            ]
            .iter()
            .any(|&(v, min, max)| !v.is_finite() || v < min || v > max)
        {
            return Err("invalid style lighting or camera settings".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ShapeKind {
    Box,
    Cylinder,
    Octahedron,
    Cone,
    CylinderX,
    RingX,
    Ellipsoid,
    RingFloor,
    Ring,
    Gear,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoxelShape {
    pub kind: ShapeKind,
    #[serde(default)]
    pub walkable: bool,
    pub position: [f32; 3],
    pub size: [f32; 3],
    pub material: String,
    #[serde(default = "once")]
    pub repeat: [u32; 3],
    #[serde(default)]
    pub spacing: [f32; 3],
    #[serde(default)]
    pub solid_radius: Option<f32>,
    #[serde(default)]
    pub light: Option<VoxelLight>,
}
fn once() -> [u32; 3] {
    [1; 3]
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalkableSurface {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub height: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentMesh {
    pub meshes: Vec<MeshData>,
    pub statistics: Statistics,
    pub obstacles: Vec<([f32; 3], f32)>,
    pub lights: Vec<BakedLight>,
    #[serde(default)]
    pub walkable: Vec<WalkableSurface>,
}
fn inside(kind: ShapeKind, p: [f32; 3]) -> bool {
    match kind {
        ShapeKind::Box => true,
        ShapeKind::Cone => p[0] * p[0] + p[2] * p[2] <= ((1.0 - p[1]) * 0.5).powi(2),
        ShapeKind::CylinderX => p[1] * p[1] + p[2] * p[2] <= 1.0,
        ShapeKind::RingX => (0.64..=1.0).contains(&(p[1] * p[1] + p[2] * p[2])),
        ShapeKind::Ellipsoid => p.iter().map(|x| x * x).sum::<f32>() <= 1.0,
        ShapeKind::RingFloor => (0.86..=1.0).contains(&(p[0] * p[0] + p[2] * p[2])),
        ShapeKind::Cylinder => p[0] * p[0] + p[2] * p[2] <= 1.0,
        ShapeKind::Octahedron => p.iter().map(|x| x.abs()).sum::<f32>() <= 1.0,
        ShapeKind::Ring => {
            let r = p[0] * p[0] + p[1] * p[1];
            (0.64..=1.0).contains(&r)
        }
        ShapeKind::Gear => {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let angle = p[1].atan2(p[0]);
            r >= 0.45
                && r <= if (angle * 12.0).cos() > 0.0 {
                    1.0
                } else {
                    0.84
                }
        }
    }
}
pub fn compile_environment(style: &StyleProfile, shapes: &[VoxelShape]) -> Result<EnvironmentMesh> {
    style.validate()?;
    if shapes.len() > 512 {
        return Err("too many architecture shapes".into());
    }
    let step = style.voxel_size;
    let mut voxels = BTreeMap::<[i32; 3], usize>::new();
    let mut obstacles = vec![];
    let mut walkable = vec![];
    let mut lights = vec![];
    let mut work = 0u64;
    let mut estimated_work = 0u64;
    let mut instances = 0u64;
    for shape in shapes {
        if shape.walkable && !matches!(shape.kind, ShapeKind::Box) {
            return Err("walkable surfaces must be boxes".into());
        }
        let material = style
            .materials
            .iter()
            .position(|m| m.name == shape.material)
            .ok_or_else(|| format!("unknown style material {}", shape.material))?;
        if shape
            .position
            .iter()
            .chain(shape.size.iter())
            .chain(shape.spacing.iter())
            .any(|x| !x.is_finite())
            || shape.size.iter().any(|&x| x <= 0.0 || x > 32.0)
            || shape.position.iter().any(|x| x.abs() > 32.0)
            || shape.repeat.iter().any(|&r| r == 0 || r > 32)
            || shape
                .solid_radius
                .is_some_and(|r| !r.is_finite() || r <= 0.0 || r > 8.0)
        {
            return Err("invalid architecture dimensions, repeat or collider".into());
        }
        let repeats = shape.repeat.iter().map(|&r| r as u64).product::<u64>();
        instances += repeats;
        estimated_work += shape
            .size
            .iter()
            .map(|v| (v / step).ceil() as u64 + 2)
            .product::<u64>()
            * repeats;
        if instances > 4096 || estimated_work > 4_000_000 {
            return Err("architecture exceeds instance or voxel work budget".into());
        }
        for ix in 0..shape.repeat[0] {
            for iy in 0..shape.repeat[1] {
                for iz in 0..shape.repeat[2] {
                    let repeats = [ix, iy, iz];
                    let center = std::array::from_fn::<_, 3, _>(|i| {
                        shape.position[i] + repeats[i] as f32 * shape.spacing[i]
                    });
                    if center.iter().any(|x| x.abs() > 32.0) {
                        return Err("architecture repeat exceeds bounds".into());
                    }
                    let min = std::array::from_fn::<_, 3, _>(|i| {
                        ((center[i] - shape.size[i] * 0.5) / step).floor() as i32
                    });
                    let max = std::array::from_fn::<_, 3, _>(|i| {
                        ((center[i] + shape.size[i] * 0.5) / step).ceil() as i32
                    });
                    work += (0..3).map(|i| (max[i] - min[i]) as u64).product::<u64>();
                    if work > 4_000_000 {
                        return Err("architecture exceeds voxel work budget".into());
                    }
                    if shape.walkable {
                        let lo = std::array::from_fn::<_, 3, _>(|i| {
                            ((center[i] - shape.size[i] * 0.5) / step - 0.5 - 0.00001).ceil() as i32
                        });
                        let hi = std::array::from_fn::<_, 3, _>(|i| {
                            ((center[i] + shape.size[i] * 0.5) / step - 0.5 + 0.00001).floor()
                                as i32
                        });
                        if (0..3).all(|i| hi[i] >= lo[i]) {
                            walkable.push(WalkableSurface {
                                min: [lo[0] as f32 * step, lo[2] as f32 * step],
                                max: [(hi[0] + 1) as f32 * step, (hi[2] + 1) as f32 * step],
                                height: (hi[1] + 1) as f32 * step,
                            });
                        }
                    }
                    if let Some(light) = &shape.light {
                        let position = std::array::from_fn(|i| center[i] + light.offset[i]);
                        let lamp = BakedLight {
                            position,
                            color: style.materials[material].color,
                            intensity: light.intensity,
                            range: light.range,
                            radius: light.radius,
                            shadows: light.shadows,
                            kind: light.kind.clone(),
                        };
                        lamp.validate()?;
                        if !style.materials[material].emissive.iter().any(|&x| x > 0.0)
                            || light.offset.iter().any(|v| !v.is_finite() || v.abs() > 4.0)
                        {
                            return Err(
                                "lights require an emissive source material and a bounded offset"
                                    .into(),
                            );
                        }
                        lights.push(lamp);
                        if lights.len() > 64 {
                            return Err("architecture exceeds 64 local lights".into());
                        }
                    }
                    if let Some(radius) = shape.solid_radius {
                        obstacles.push((center, radius));
                    }
                    for x in min[0]..max[0] {
                        for y in min[1]..max[1] {
                            for z in min[2]..max[2] {
                                let cell = [x, y, z];
                                let p = std::array::from_fn(|i| {
                                    (((cell[i] as f32 + 0.5) * step) - center[i])
                                        / (shape.size[i] * 0.5)
                                });
                                if p.iter().all(|v| v.abs() <= 1.00001) && inside(shape.kind, p) {
                                    voxels.insert(cell, material);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut faces: BTreeMap<(usize, i8, i32, usize), BTreeSet<(i32, i32)>> = BTreeMap::new();
    let mut exposed = 0;
    for (&cell, &material) in &voxels {
        for axis in 0..3 {
            for sign in [-1i8, 1] {
                let mut neighbor = cell;
                neighbor[axis] += sign as i32;
                if voxels.contains_key(&neighbor) {
                    continue;
                }
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                faces
                    .entry((axis, sign, cell[axis] * 2 + 1 + sign as i32, material))
                    .or_default()
                    .insert((cell[u], cell[v]));
                exposed += 1;
            }
        }
    }
    let mut meshes: Vec<_> = (0..style.materials.len())
        .map(|material| MeshData {
            material,
            ..Default::default()
        })
        .collect();
    for ((axis, sign, plane, material), mut cells) in faces {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        while let Some(&(x, y)) = cells.first() {
            let mut w = 1;
            while cells.contains(&(x + w, y)) {
                w += 1;
            }
            let mut h = 1;
            while (0..w).all(|i| cells.contains(&(x + i, y + h))) {
                h += 1;
            }
            for j in 0..h {
                for i in 0..w {
                    cells.remove(&(x + i, y + j));
                }
            }
            let mesh = &mut meshes[material];
            let base = mesh.positions.len() as u32;
            for (du, dv) in [(0, 0), (w, 0), (w, h), (0, h)] {
                let mut p = [0.0; 3];
                p[axis] = plane as f32 * 0.5 * step;
                p[u] = (x + du) as f32 * step;
                p[v] = (y + dv) as f32 * step;
                mesh.positions.push(p);
                let mut n = [0.0; 3];
                n[axis] = sign as f32;
                mesh.normals.push(n);
            }
            let indices = if sign > 0 {
                [0, 1, 2, 0, 2, 3]
            } else {
                [0, 2, 1, 0, 3, 2]
            };
            mesh.indices.extend(indices.map(|i| base + i));
        }
    }
    meshes.retain(|m| !m.positions.is_empty());
    let quads = meshes.iter().map(|m| m.indices.len() / 6).sum();
    Ok(EnvironmentMesh {
        meshes,
        statistics: Statistics {
            voxels: voxels.len(),
            exposed_faces: exposed,
            quads,
        },
        obstacles,
        lights,
        walkable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shape() -> VoxelShape {
        VoxelShape {
            walkable: false,
            kind: ShapeKind::Gear,
            position: [0.0, 1.0, 0.0],
            size: [2.0, 2.0, 0.6],
            material: "metal".into(),
            repeat: [1; 3],
            spacing: [0.0; 3],
            solid_radius: Some(1.0),
            light: None,
        }
    }
    #[test]
    fn density_changes_geometry_without_scaling_world_or_colliders() {
        let mut style = StyleProfile::default();
        style.voxel_size = 0.3;
        let coarse = compile_environment(&style, &[shape()]).unwrap();
        style.voxel_size = 0.1;
        let fine = compile_environment(&style, &[shape()]).unwrap();
        assert!(fine.statistics.voxels > coarse.statistics.voxels * 10);
        assert!(fine.statistics.quads < fine.statistics.exposed_faces);
        assert_eq!(fine.obstacles, coarse.obstacles);
        for mesh in &fine.meshes {
            for p in &mesh.positions {
                assert!(p[0].abs() <= 1.01 && p[1] >= -0.01 && p[1] <= 2.01);
            }
        }
        assert_eq!(
            serde_json::to_vec(&fine).unwrap(),
            serde_json::to_vec(&compile_environment(&style, &[shape()]).unwrap()).unwrap()
        );
        for mesh in &fine.meshes {
            for t in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [
                    mesh.positions[t[0] as usize],
                    mesh.positions[t[1] as usize],
                    mesh.positions[t[2] as usize],
                ];
                let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
                let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                assert!(
                    (0..3)
                        .map(|i| cross[i] * mesh.normals[t[0] as usize][i])
                        .sum::<f32>()
                        > 0.0
                );
            }
        }
    }
    #[test]
    fn style_and_recipe_validation_bound_work_and_reject_bad_roles() {
        let mut style = StyleProfile::default();
        style.voxel_size = f32::NAN;
        assert!(style.validate().is_err());
        style = StyleProfile::default();
        style.materials[0].name = style.materials[1].name.clone();
        assert!(style.validate().is_err());
        style = StyleProfile::default();
        let mut s = shape();
        s.material = "missing".into();
        assert!(compile_environment(&style, &[s.clone()]).is_err());
        s.material = "metal".into();
        s.repeat = [32; 3];
        assert!(compile_environment(&style, &[s.clone()]).is_err());
        s.repeat = [1; 3];
        s.size = [f32::NAN; 3];
        assert!(compile_environment(&style, &[s]).is_err());
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActorPart {
    Body,
    LeftLeg,
    RightLeg,
    Weapon,
    Head,
    WoundPlug,
    Armor,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponSkin {
    pub weapon: content::scenario::Weapon,
    pub skin: String,
    #[serde(default)]
    pub muzzle: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorRecipe {
    pub part: ActorPart,
    #[serde(default)]
    pub pivot: Option<[f32; 3]>,
    pub shapes: Vec<VoxelShape>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActorMesh {
    pub part: ActorPart,
    pub pivot: Option<[f32; 3]>,
    pub meshes: Vec<MeshData>,
    pub statistics: Statistics,
    pub lights: Vec<BakedLight>,
}
pub fn compile_actor(profile: &StyleProfile) -> Result<Vec<ActorMesh>> {
    profile.validate()?;
    let mut style = profile.clone();
    style.voxel_size = profile.actor_voxel_size.unwrap_or(profile.voxel_size);
    style.actor.clear();
    profile
        .actor
        .iter()
        .map(|recipe| {
            let env = compile_environment(&style, &recipe.shapes)?;
            if env.meshes.is_empty() || !env.obstacles.is_empty() || !env.walkable.is_empty() {
                return Err("actor parts require visible geometry without world colliders".into());
            }
            Ok(ActorMesh {
                part: recipe.part,
                pivot: recipe.pivot,
                meshes: env.meshes,
                statistics: env.statistics,
                lights: env.lights,
            })
        })
        .collect()
}

#[cfg(test)]
mod actor_tests {
    use super::*;
    #[test]
    fn actor_profiles_bake_finer_meshes_and_reject_missing_or_duplicate_parts() {
        let shape = VoxelShape {
            walkable: false,
            kind: ShapeKind::Cylinder,
            position: [0.0; 3],
            size: [0.5; 3],
            material: "metal".into(),
            repeat: [1; 3],
            spacing: [0.0; 3],
            solid_radius: None,
            light: None,
        };
        let mut profile = StyleProfile {
            actor: vec![ActorRecipe {
                part: ActorPart::Body,
                pivot: None,
                shapes: vec![shape],
            }],
            actor_voxel_size: Some(0.05),
            ..Default::default()
        };
        let fine = compile_actor(&profile).unwrap();
        profile.actor_voxel_size = Some(0.25);
        let coarse = compile_actor(&profile).unwrap();
        assert!(fine[0].statistics.voxels > coarse[0].statistics.voxels * 20);
        profile.actor.push(profile.actor[0].clone());
        assert!(profile.validate().is_err());
        profile.actor.pop();
        profile.actor[0].shapes.clear();
        assert!(compile_actor(&profile).is_err());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoxelLight {
    pub intensity: f32,
    pub range: f32,
    pub radius: f32,
    #[serde(default)]
    pub kind: LightKind,
    #[serde(default)]
    pub shadows: bool,
    #[serde(default)]
    pub offset: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BakedLight {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub radius: f32,
    pub shadows: bool,
    pub kind: LightKind,
}
impl BakedLight {
    pub fn validate(&self) -> Result<()> {
        if self
            .position
            .iter()
            .any(|x| !x.is_finite() || x.abs() > 40.0)
            || self
                .color
                .iter()
                .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
            || [
                (self.intensity, 1.0, 100_000.0),
                (self.range, 0.1, 20.0),
                (self.radius, 0.01, 2.0),
            ]
            .iter()
            .any(|&(v, a, b)| !v.is_finite() || v < a || v > b)
        {
            return Err("invalid baked light".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod lighting_tests {
    use super::*;
    #[test]
    fn repeated_fixture_lights_share_geometry_positions_and_require_emission() {
        let style = StyleProfile::default();
        let mut shape = VoxelShape {
            walkable: false,
            kind: ShapeKind::Octahedron,
            position: [0.0, 2.0, 0.0],
            size: [0.3; 3],
            material: "magic".into(),
            repeat: [2, 1, 1],
            spacing: [3.0, 0.0, 0.0],
            solid_radius: None,
            light: Some(VoxelLight {
                intensity: 2000.0,
                range: 4.0,
                radius: 0.2,
                shadows: true,
                kind: LightKind::Point,
                offset: [0.0, 0.1, 0.0],
            }),
        };
        let result = compile_environment(&style, &[shape.clone()]).unwrap();
        assert_eq!(result.lights.len(), 2);
        assert_eq!(result.lights[0].position, [0.0, 2.1, 0.0]);
        assert_eq!(result.lights[1].position, [3.0, 2.1, 0.0]);
        assert!(result.lights.iter().all(|l| l.shadows));
        shape.material = "stone".into();
        assert!(compile_environment(&style, &[shape.clone()]).is_err());
        shape.material = "magic".into();
        shape.light.as_mut().unwrap().intensity = f32::NAN;
        assert!(compile_environment(&style, &[shape]).is_err());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum LightKind {
    #[default]
    Point,
    Spot {
        direction: [f32; 3],
        inner: f32,
        outer: f32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum Motion {
    Biped,
    Quadruped,
    Vehicle {
        wheel_radius: f32,
    },
    Machine {
        axis: [f32; 3],
    },
    Turret,
    #[default]
    Static,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkinRecipe {
    pub id: String,
    pub scale: f32,
    #[serde(default)]
    pub motion: Motion,
    #[serde(default)]
    pub use_actor: bool,
    #[serde(default)]
    pub voxel_size: Option<f32>,
    #[serde(default)]
    pub material_map: BTreeMap<String, String>,
    #[serde(default)]
    pub parts: Vec<ActorRecipe>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkinMesh {
    pub id: String,
    pub scale: f32,
    pub motion: Motion,
    pub parts: Vec<ActorMesh>,
}
pub fn compile_skins(profile: &StyleProfile) -> Result<Vec<SkinMesh>> {
    if profile.skins.len() > 32 {
        return Err("too many actor skins".into());
    }
    let mut ids = BTreeSet::new();
    let mut result = vec![];
    for recipe in &profile.skins {
        if recipe.id.is_empty()
            || !ids.insert(&recipe.id)
            || !recipe.scale.is_finite()
            || !(0.02..=4.0).contains(&recipe.scale)
        {
            return Err("invalid actor skin identity or scale".into());
        }
        match recipe.motion {
            Motion::Vehicle { wheel_radius }
                if !wheel_radius.is_finite() || wheel_radius <= 0.01 =>
            {
                return Err("invalid wheel radius".into())
            }
            Motion::Machine { axis }
                if axis.iter().any(|v| !v.is_finite())
                    || axis.iter().map(|v| v * v).sum::<f32>() < 0.01 =>
            {
                return Err("invalid machine axis".into())
            }
            _ => {}
        }
        let mut p = profile.clone();
        p.skins.clear();
        p.weapon_skins.clear();
        p.actor = if recipe.use_actor {
            profile.actor.clone()
        } else {
            recipe.parts.clone()
        };
        p.actor_voxel_size = Some(recipe.voxel_size.unwrap_or(if recipe.use_actor {
            profile.actor_voxel_size.unwrap_or(profile.voxel_size)
        } else {
            profile.voxel_size
        }));
        for part in &mut p.actor {
            if part
                .pivot
                .is_some_and(|p| p.iter().any(|v| !v.is_finite() || v.abs() > 16.0))
            {
                return Err("invalid rig pivot".into());
            }
            for shape in &mut part.shapes {
                if let Some(material) = recipe.material_map.get(&shape.material) {
                    shape.material = material.clone();
                }
            }
        }
        let parts = compile_actor(&p)?;
        if parts.is_empty() {
            return Err("skin must contain geometry".into());
        }
        result.push(SkinMesh {
            id: recipe.id.clone(),
            scale: recipe.scale,
            motion: recipe.motion.clone(),
            parts,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod surface_tests {
    use super::*;
    #[test]
    fn walkable_metadata_matches_baked_top_faces_at_different_voxel_densities() {
        for step in [0.1, 0.15, 0.3] {
            let style = StyleProfile {
                voxel_size: step,
                ..Default::default()
            };
            let shape = VoxelShape {
                kind: ShapeKind::Box,
                walkable: true,
                position: [0., 0., 0.],
                size: [3., 0.3, 2.],
                material: "stone".into(),
                repeat: [1; 3],
                spacing: [0.; 3],
                solid_radius: None,
                light: None,
            };
            let mesh = compile_environment(&style, &[shape]).unwrap();
            let top = mesh
                .meshes
                .iter()
                .flat_map(|m| &m.positions)
                .map(|p| p[1])
                .max_by(f32::total_cmp)
                .unwrap();
            assert_eq!(mesh.walkable.len(), 1);
            assert!(
                (mesh.walkable[0].height - top).abs() < 1e-5,
                "{step}: {} != {top}",
                mesh.walkable[0].height
            );
        }
    }
}

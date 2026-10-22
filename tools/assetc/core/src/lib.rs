//! Renderer-independent prefab recipes, meshing and versioned asset packs.
mod boulder;
mod piles;
pub use boulder::generate_boulder;
pub use piles::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const VERSION: u32 = 3;
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename = "Boulder", deny_unknown_fields)]
pub struct BoulderRecipe {
    pub seed: u32,
    pub radius: f32,
    pub height: f32,
    pub voxel_size: f32,
    pub taper: f32,
    pub ore_every: u32,
    pub debris: u32,
    pub material_pattern: MaterialPattern,
}
impl Default for BoulderRecipe {
    fn default() -> Self {
        Self {
            seed: 0,
            radius: 0.85,
            height: 1.25,
            voxel_size: 0.31,
            taper: 0.65,
            ore_every: 17,
            debris: 8,
            material_pattern: MaterialPattern::Speckled,
        }
    }
}
impl BoulderRecipe {
    pub fn validate(&self) -> Result<()> {
        if ![self.radius, self.height, self.voxel_size, self.taper]
            .iter()
            .all(|x| x.is_finite())
            || !(0.1..=16.0).contains(&self.radius)
            || !(0.1..=16.0).contains(&self.height)
            || !(0.05..=2.0).contains(&self.voxel_size)
            || !(0.0..=0.95).contains(&self.taper)
            || self.height < self.voxel_size
            || self.ore_every == 0
            || self.debris > 64
        {
            return Err("invalid boulder dimensions, taper, ore frequency or debris count".into());
        }
        let side = 2.0 * ((self.radius + 0.09) / self.voxel_size).ceil() + 1.0;
        if side * side * (self.height / self.voxel_size).floor() > 262144.0 {
            return Err("boulder exceeds the 262144-cell generation budget".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoulderParameters {
    pub seed: Option<u32>,
    pub radius: Option<f32>,
    pub height: Option<f32>,
}
impl BoulderParameters {
    pub fn apply(&self, base: &BoulderRecipe) -> Result<BoulderRecipe> {
        let recipe = BoulderRecipe {
            seed: self.seed.unwrap_or(base.seed),
            radius: self.radius.unwrap_or(base.radius),
            height: self.height.unwrap_or(base.height),
            ..base.clone()
        };
        recipe.validate()?;
        Ok(recipe)
    }
}

/// Assign palette slots either as natural inclusions or four readable surface regions.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum MaterialPattern {
    #[default]
    Speckled,
    Quadrants,
}

/// Shared material semantics; the renderer maps these to visual and audio feedback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactResponse {
    #[default]
    Stone,
    Metal,
    Crystal,
    Polymer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub name: String,
    pub color: [f32; 3],
    pub emissive: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    #[serde(default)]
    pub impact: ImpactResponse,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename = "Palette", deny_unknown_fields)]
pub struct Palette {
    pub materials: Vec<Material>,
}
impl Palette {
    pub fn validate(&self) -> Result<()> {
        if self.materials.len() != 4 {
            return Err("boulders require four materials: base, dark, light, ore".into());
        }
        for m in &self.materials {
            if m.name.is_empty()
                || !m
                    .color
                    .iter()
                    .chain(m.emissive.iter())
                    .all(|x| x.is_finite() && *x >= 0.0)
                || !m.metallic.is_finite()
                || !m.roughness.is_finite()
                || !(0.0..=1.0).contains(&m.metallic)
                || !(0.0..=1.0).contains(&m.roughness)
            {
                return Err(format!("invalid material {}", m.name));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MeshData {
    pub material: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Collision {
    pub radius: f32,
    pub layers: Vec<CollisionLayer>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct CollisionLayer {
    pub bottom: f32,
    pub top: f32,
    pub radius: f32,
}
impl Collision {
    pub fn strike_radius_at(&self, height: f32) -> f32 {
        self.layers
            .iter()
            .filter(|layer| height >= layer.bottom && height <= layer.top)
            .map(|layer| layer.radius)
            .fold(0.0, f32::max)
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Statistics {
    pub voxels: usize,
    pub exposed_faces: usize,
    pub quads: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub meshes: Vec<MeshData>,
    pub collision: Collision,
    pub statistics: Statistics,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Geometry {
    Baked(Model),
    Runtime(BoulderRecipe),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prefab {
    pub version: u32,
    pub id: String,
    pub fingerprint: String,
    pub geometry_key: String,
    pub palette: Palette,
    pub geometry: Geometry,
}
impl Prefab {
    pub fn new(id: String, palette: Palette, geometry: Geometry) -> Result<Self> {
        let fingerprint = content_key(&(&palette, &geometry));
        let geometry_key = content_key(&geometry);
        let prefab = Self {
            version: VERSION,
            id,
            fingerprint,
            geometry_key,
            palette,
            geometry,
        };
        prefab.validate()?;
        Ok(prefab)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub prefab: String,
    pub position: [f32; 3],
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub parameters: Option<BoulderParameters>,
    #[serde(default)]
    pub label: Option<String>,
}
fn one() -> f32 {
    1.0
}
fn industrial() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename = "Scene", deny_unknown_fields)]
pub struct Scene {
    pub name: String,
    pub spawn: [f32; 3],
    #[serde(default = "industrial")]
    pub industrial: bool,
    pub instances: Vec<Instance>,
    #[serde(default)]
    pub piles: Option<PileLayout>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenePack {
    pub version: u32,
    pub scene: Scene,
    pub prefabs: Vec<Prefab>,
}

impl Prefab {
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(format!("unsupported prefab version {}", self.version));
        }
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("invalid prefab ID".into());
        }
        self.palette.validate()?;
        match &self.geometry {
            Geometry::Runtime(r) => r.validate(),
            Geometry::Baked(model) => {
                if model.meshes.is_empty() || model.meshes.len() > 4 {
                    return Err("invalid submesh count".into());
                }
                if !model.collision.radius.is_finite()
                    || model.collision.radius <= 0.0
                    || model.collision.layers.is_empty()
                    || model.collision.layers.iter().any(|l| {
                        !l.bottom.is_finite()
                            || !l.top.is_finite()
                            || !l.radius.is_finite()
                            || l.top <= l.bottom
                            || l.radius <= 0.0
                    })
                {
                    return Err("invalid collision metadata".into());
                }
                for m in &model.meshes {
                    if m.material >= 4
                        || m.positions.len() != m.normals.len()
                        || m.positions.is_empty()
                        || m.positions.len() > 2000000
                        || m.indices.len() % 3 != 0
                        || m.positions
                            .iter()
                            .chain(m.normals.iter())
                            .flatten()
                            .any(|x| !x.is_finite())
                        || m.indices.iter().any(|&i| i as usize >= m.positions.len())
                    {
                        return Err("invalid mesh buffers".into());
                    }
                }
                Ok(())
            }
        }
    }
}
impl ScenePack {
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(format!("unsupported scene version {}", self.version));
        }
        if self.scene.name.trim().is_empty()
            || self.scene.instances.len() > 512
            || !self.scene.spawn.iter().all(|v| v.is_finite())
        {
            return Err("invalid scene header or instance count".into());
        }
        if let Some(piles) = &self.scene.piles {
            piles.validate()?;
        }
        let mut ids = BTreeSet::new();
        for prefab in &self.prefabs {
            prefab.validate()?;
            if !ids.insert(&prefab.id) {
                return Err(format!("duplicate prefab {}", prefab.id));
            }
        }
        for instance in &self.scene.instances {
            if !instance.position.iter().all(|v| v.is_finite())
                || !instance.scale.is_finite()
                || !(0.1..=8.0).contains(&instance.scale)
                || !instance.yaw.is_finite()
            {
                return Err("invalid instance transform".into());
            }
            let prefab = self
                .prefabs
                .iter()
                .find(|p| p.id == instance.prefab)
                .ok_or_else(|| format!("unknown prefab {}", instance.prefab))?;
            if let Some(parameters) = &instance.parameters {
                if let Geometry::Runtime(recipe) = &prefab.geometry {
                    parameters.apply(recipe)?;
                } else {
                    return Err(format!(
                        "parameters on baked prefab {}; use runtime mode",
                        prefab.id
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("asset pack exceeds 32 MiB".into());
        }
        let pack: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        pack.validate()?;
        Ok(pack)
    }
}
/// Stable content key shared by the tool and runtime cache.
pub fn content_key(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("validated asset serializes");
    let hash = bytes.iter().fold(14695981039346656037_u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(1099511628211)
    });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn palette() -> Palette {
        Palette {
            materials: (0..4)
                .map(|i| Material {
                    name: format!("material_{i}"),
                    color: [0.3; 3],
                    emissive: [0.0; 3],
                    metallic: 0.0,
                    roughness: 0.8,
                    impact: ImpactResponse::Stone,
                })
                .collect(),
        }
    }
    #[test]
    fn baked_and_runtime_recipes_produce_the_same_geometry() {
        let recipe = BoulderRecipe::default();
        let baked = Prefab::new(
            "rock".into(),
            palette(),
            Geometry::Baked(generate_boulder(&recipe).unwrap()),
        )
        .unwrap();
        let runtime = Prefab::new("dynamic".into(), palette(), Geometry::Runtime(recipe)).unwrap();
        let Geometry::Baked(model) = baked.geometry else {
            unreachable!()
        };
        let Geometry::Runtime(recipe) = runtime.geometry else {
            unreachable!()
        };
        assert_eq!(
            content_key(&model),
            content_key(&generate_boulder(&recipe).unwrap())
        );
        let changed = BoulderParameters {
            radius: Some(1.3),
            ..Default::default()
        }
        .apply(&recipe)
        .unwrap();
        assert_ne!(
            content_key(&model),
            content_key(&generate_boulder(&changed).unwrap())
        );
    }
    #[test]
    fn material_variants_share_geometry_but_not_prefab_identity() {
        let geometry = Geometry::Baked(generate_boulder(&Default::default()).unwrap());
        let stone = Prefab::new("stone".into(), palette(), geometry.clone()).unwrap();
        let mut metal_palette = palette();
        for material in &mut metal_palette.materials {
            material.impact = ImpactResponse::Metal;
        }
        let metal = Prefab::new("metal".into(), metal_palette, geometry).unwrap();
        assert_eq!(stone.geometry_key, metal.geometry_key);
        assert_ne!(stone.fingerprint, metal.fingerprint);
    }

    #[test]
    fn pack_validation_rejects_bad_versions_transforms_and_baked_parameters() {
        let prefab = Prefab::new(
            "rock".into(),
            palette(),
            Geometry::Baked(generate_boulder(&Default::default()).unwrap()),
        )
        .unwrap();
        let mut pack = ScenePack {
            version: VERSION,
            scene: Scene {
                name: "test".into(),
                spawn: [0.0; 3],
                industrial: true,
                piles: None,
                instances: vec![Instance {
                    prefab: "rock".into(),
                    position: [0.0; 3],
                    scale: 1.0,
                    yaw: 0.0,
                    parameters: None,
                    label: None,
                }],
            },
            prefabs: vec![prefab],
        };
        assert!(pack.validate().is_ok());
        pack.version = 99;
        assert!(pack.validate().is_err());
        pack.version = VERSION;
        pack.scene.instances[0].scale = -1.0;
        assert!(pack.validate().is_err());
        pack.scene.instances[0].scale = 1.0;
        pack.scene.instances[0].parameters = Some(BoulderParameters::default());
        assert!(pack.validate().is_err());
    }
}

//! Bevy adapter for baked prefabs and the shared runtime boulder generator.
use crate::{
    effects::Feedback,
    gameplay::{Controls, Obstacle},
    impact::{self, ImpactSlot, ImpactTarget, Surface},
    navigation::Disc,
};
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};
use std::{collections::HashMap, sync::Arc};
use voxel_assets::{
    content_key, generate_boulder, BoulderRecipe, Collision, Geometry, Instance, Material, MeshData, Model, Prefab,
    ScenePack,
};
use service::WorldScoped;

#[derive(Component)]
pub struct BoulderInstance;
#[derive(Component)]
pub struct SampleCaption(Vec3);
/// Every scene the game ships, decoded once.
#[derive(Resource)]
pub struct SceneLibrary {
    pub scenes: Vec<Arc<ScenePack>>,
}
impl SceneLibrary {
    pub fn new() -> Self {
        let scenes: Vec<_> = [
            ScenePack::decode(mining_deck::PACK_BYTES).expect("validated mining deck asset pack"),
            ScenePack::decode(boulder_lab::PACK_BYTES).expect("validated boulder lab asset pack"),
            ScenePack::decode(pile_lab::PACK_BYTES).expect("validated pile lab asset pack"),
            ScenePack::decode(jetpack_lab::PACK_BYTES).expect("validated jetpack lab asset pack"),
            ScenePack::decode(surprise_lab::PACK_BYTES).expect("validated field trial asset pack"),
            ScenePack::decode(atlantis::PACK_BYTES).expect("validated Atlantis asset pack"),
            ScenePack::decode(forest_outpost::PACK_BYTES).expect("validated forest asset pack"),
            ScenePack::decode(combat_arena::PACK_BYTES).expect("validated combat asset pack"),
            ScenePack::decode(dig_lab::PACK_BYTES).expect("validated dig lab asset pack"),
            ScenePack::decode(field::PACK_BYTES).expect("validated field asset pack"),
        ]
        .into_iter()
        .map(Arc::new)
        .collect();
        let mut ids = std::collections::HashSet::new();
        for pack in &scenes {
            assert!(ids.insert(pack.scene.id.as_str()), "duplicate scene id {}", pack.scene.id);
        }
        Self { scenes }
    }
    pub fn index(&self, id: &str) -> Result<usize, String> {
        self.scenes.iter().position(|pack| pack.scene.id == id)
            .ok_or_else(|| format!("Unknown scene {id}"))
    }
}
/// A scene's half-width in meters.
pub fn extent(pack: &ScenePack) -> f32 {
    if let Some(mine) = &pack.scene.mine {
        return mine.extent;
    }
    pack.scene.scenario.as_ref().map_or(8., |s| s.extent.to_f32())
}
#[derive(Clone)]
struct CachedModel {
    meshes: Vec<(Handle<Mesh>, usize)>,
    collision: Collision,
    surfaces: Arc<Vec<Surface>>,
}
#[derive(Resource, Default)]
pub struct ModelCache {
    models: HashMap<String, CachedModel>,
    pub runtime_generations: usize,
}
impl ModelCache {
    /// Generates or decodes, and uploads, every model a scene's instances use.
    pub fn load(&mut self, pack: &ScenePack, meshes: &mut Assets<Mesh>) {
        for instance in &pack.scene.instances {
            let prefab = pack.prefabs.iter().find(|p| p.id == instance.prefab).expect("validated prefab reference");
            let (key, recipe) = model_key(prefab, instance);
            if self.models.contains_key(&key) {
                continue;
            }
            let model = if let Some(recipe) = recipe {
                self.runtime_generations += 1;
                generate_boulder(&recipe).expect("validated runtime recipe")
            } else if let Geometry::Baked(model) = &prefab.geometry {
                model.clone()
            } else {
                unreachable!()
            };
            self.models.insert(key, upload(model, meshes));
        }
    }
    /// The collision radius of an instance's model, before scaling. The model
    /// is cached once the scene has been loaded (`load`).
    pub fn collision_radius(&self, pack: &ScenePack, instance: &Instance) -> f32 {
        let prefab = pack.prefabs.iter().find(|p| p.id == instance.prefab).expect("validated prefab reference");
        self.models[&model_key(prefab, instance).0].collision.radius
    }
}
/// The cache key of an instance's model, and the recipe to generate it if
/// it is a runtime boulder.
fn model_key(prefab: &Prefab, instance: &Instance) -> (String, Option<BoulderRecipe>) {
    match &prefab.geometry {
        Geometry::Baked(_) => (format!("baked:{}", prefab.geometry_key), None),
        Geometry::Runtime(recipe) => {
            let recipe = instance
                .parameters
                .as_ref()
                .map_or_else(|| Ok(recipe.clone()), |p| p.apply(recipe))
                .expect("validated runtime parameters");
            (format!("runtime:{}", content_key(&recipe)), Some(recipe))
        }
    }
}
pub(crate) fn standard(material: &Material) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(material.color[0], material.color[1], material.color[2]),
        emissive: LinearRgba::rgb(
            material.emissive[0],
            material.emissive[1],
            material.emissive[2],
        ),
        metallic: material.metallic,
        perceptual_roughness: material.roughness,
        ..default()
    }
}
pub(crate) fn mesh(data: &MeshData) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; data.positions.len()])
    .with_inserted_indices(Indices::U32(data.indices.clone()))
}
fn upload(model: Model, meshes: &mut Assets<Mesh>) -> CachedModel {
    CachedModel {
        meshes: model
            .meshes
            .iter()
            .map(|m| (meshes.add(mesh(m)), m.material))
            .collect(),
        surfaces: impact::surfaces(&model),
        collision: model.collision,
    }
}

/// Spawns a scene's boulders and their captions for a world (see
/// `SceneView`), and resets the player's controls and feedback.
pub fn spawn_scene(world: &mut World, pack: &Arc<ScenePack>) {
    world.run_system_cached_with(spawn_boulders, pack.clone()).expect("spawning a scene's boulders");
}

/// Frees the materials of the world's boulders, which have their own so only
/// the struck one flashes. The boulders go with the world.
pub fn release_scene(world: &mut World) {
    let handles: Vec<_> = world
        .query_filtered::<&ImpactTarget, With<BoulderInstance>>()
        .iter(world)
        .flat_map(|target| target.slots.iter().map(|slot| slot.handle.clone()))
        .collect();
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    for handle in handles {
        materials.remove(&handle);
    }
}

fn spawn_boulders(
    In(scene): In<Arc<ScenePack>>,
    mut commands: Commands,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut controls: ResMut<Controls>,
    mut feedback: ResMut<Feedback>,
) {
    *controls = Controls::default();
    *feedback = Feedback::default();
    cache.load(&scene, &mut meshes);
    for instance in &scene.scene.instances {
        let prefab = scene
            .prefabs
            .iter()
            .find(|p| p.id == instance.prefab)
            .expect("validated prefab reference");
        let model = cache.models[&model_key(prefab, instance).0].clone();
        // Geometry is shared. Materials belong to each target so only the struck
        // slot flashes, including when another instance uses the same prefab.
        let slots: Vec<_> = prefab
            .palette
            .materials
            .iter()
            .map(|m| {
                let material = standard(m);
                ImpactSlot {
                    rest_emissive: material.emissive,
                    handle: materials.add(material),
                    kind: m.impact,
                    flash: 0.0,
                }
            })
            .collect();
        let handles: Vec<_> = slots.iter().map(|s| s.handle.clone()).collect();
        let center = Vec3::from_array(instance.position);
        let radius = model.collision.radius * instance.scale;
        let contact_radius = model
            .collision
            .strike_radius_at((0.9 - center.y) / instance.scale)
            * instance.scale;
        let transform = Transform::from_translation(center)
            .with_scale(Vec3::splat(instance.scale))
            .with_rotation(Quat::from_rotation_y(instance.yaw));
        commands
            .spawn((
                BoulderInstance,
                WorldScoped::default(),
                transform,
                Visibility::default(),
                Obstacle(Disc { center, radius }),
                ImpactTarget {
                    base: center,
                    contact_radius,
                    kick: Vec3::ZERO,
                    slots,
                    surfaces: model.surfaces,
                    local_from_world: transform.compute_affine().inverse(),
                },
            ))
            .with_children(|parent| {
                for (mesh, material) in model.meshes {
                    parent.spawn((Mesh3d(mesh), MeshMaterial3d(handles[material].clone())));
                }
            });
        if let Some(label) = &instance.label {
            commands.spawn((
                SampleCaption(center + Vec3::Y * 2.1),
                WorldScoped::default(),
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.65, 0.9, 1.0)),
                TextLayout::justify(Justify::Center),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(200.0),
                    ..default()
                },
            ));
        }
    }
    info!(
        "Loaded scene {}: {} instances, {} cached models, {} runtime generations",
        scene.scene.name,
        scene.scene.instances.len(),
        cache.models.len(),
        cache.runtime_generations
    );
}

/// Project sample captions into the UI without requiring a second sprite renderer.
pub fn update_captions(
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut captions: Query<(&SampleCaption, &mut Node, &mut Visibility)>,
) {
    for (caption, mut node, mut visibility) in &mut captions {
        if let Ok(point) = camera.0.world_to_viewport(camera.1, caption.0) {
            node.left = px(point.x - 100.0);
            node.top = px(point.y);
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel_assets::SceneFeature;
    #[test]
    fn scene_switching_reuses_geometry_and_resets_gameplay() {
        let mut world = World::new();
        world.insert_resource(SceneLibrary::new());
        world.init_resource::<ModelCache>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Controls>();
        world.init_resource::<Feedback>();
        let baseline_materials = world.resource::<Assets<StandardMaterial>>().len();
        // Materials each scene's boulders allocate, by scene.
        let mut allocated = [None, None];
        for (step, index) in [0, 1, 0, 1].into_iter().enumerate() {
            world.resource_mut::<Controls>().impacts = 5;
            world.resource_mut::<Controls>().last_material = Some(voxel_assets::ImpactResponse::Metal);
            world.resource_mut::<Controls>().velocity = Vec3::X;
            let pack = world.resource::<SceneLibrary>().scenes[index].clone();
            spawn_scene(&mut world, &pack);
            // The mining deck's boulders are baked; the boulder lab generates
            // two models once and reuses them after.
            let generations = if step == 0 { 0 } else { 2 };
            assert_eq!(world.resource::<ModelCache>().runtime_generations, generations);
            assert_eq!(world.resource::<Controls>().impacts, 0);
            assert!(world.resource::<Controls>().last_material.is_none());
            assert_eq!(world.resource::<Controls>().velocity, Vec3::ZERO);
            assert_eq!(world.query::<&SampleCaption>().iter(&world).count(), if index == 0 { 0 } else { 4 });
            assert_eq!(world.query::<&BoulderInstance>().iter(&world).count(), if index == 0 { 6 } else { 10 });
            let added = world.resource::<Assets<StandardMaterial>>().len() - baseline_materials;
            assert_eq!(*allocated[index].get_or_insert(added), added, "a revisit allocates the same");
            // The world stops: its boulders' materials go, then its entities.
            release_scene(&mut world);
            let scoped: Vec<_> = world.query_filtered::<Entity, With<WorldScoped>>().iter(&world).collect();
            for entity in scoped {
                world.despawn(entity);
            }
            assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), baseline_materials);
        }
        assert_eq!(allocated[1].unwrap() - allocated[0].unwrap(), 16);
    }

    #[test]
    fn embedded_scenes_validate_and_demonstrate_shared_and_runtime_assets() {
        let library = SceneLibrary::new();
        let mining = &library.scenes[0];
        let lab = &library.scenes[1];
        assert_eq!(library.index("mining-deck"), Ok(0));
        assert_eq!(library.index("combat-arena"), Ok(7));
        assert_eq!(library.index("dig-lab"), Ok(8));
        assert_eq!(library.index("field"), Ok(9));
        assert!(library.scenes[8].scene.features.contains(&SceneFeature::Digging));
        assert!(library.index("missing").is_err());
        assert!(library.scenes[4].scene.features.contains(&SceneFeature::MemoryTrial));
        assert!(library.scenes[3].scene.features.contains(&SceneFeature::FlightHud));
        assert_ne!(mining.scene.name, lab.scene.name);
        assert!(mining
            .prefabs
            .iter()
            .all(|p| matches!(p.geometry, Geometry::Baked(_))));
        assert!(lab
            .prefabs
            .iter()
            .any(|p| matches!(p.geometry, Geometry::Runtime(_))));
        let key = |scene: &ScenePack| {
            scene
                .prefabs
                .iter()
                .find(|p| p.id == "ore_small")
                .unwrap()
                .fingerprint
                .clone()
        };
        assert_eq!(key(mining), key(lab));
        let dynamic = lab.prefabs.iter().find(|p| p.id == "dynamic_ore").unwrap();
        let Geometry::Runtime(base) = &dynamic.geometry else {
            panic!("runtime prefab")
        };
        let instances: Vec<_> = lab
            .scene
            .instances
            .iter()
            .filter(|i| i.prefab == "dynamic_ore")
            .collect();
        let recipes: Vec<_> = instances
            .iter()
            .map(|i| i.parameters.as_ref().unwrap().apply(base).unwrap())
            .collect();
        assert_ne!(content_key(&recipes[0]), content_key(&recipes[1]));
        assert_eq!(content_key(&recipes[1]), content_key(&recipes[2]));
    }
}

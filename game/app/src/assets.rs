//! Bevy adapter for baked prefabs and the shared runtime boulder generator.
use crate::{
    effects::Feedback,
    gameplay::{Controls, Obstacle, Player},
    impact::{self, ImpactSlot, ImpactTarget, Surface},
    navigation::Disc,
};
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};
use std::{collections::HashMap, sync::Arc};
use voxel_assets::{
    content_key, generate_boulder, Collision, Geometry, Material, MeshData, Model, SceneFeature, ScenePack,
};

#[derive(Component)]
pub struct BoulderInstance;
#[derive(Component)]
pub struct SampleCaption(Vec3);
#[derive(Component)]
pub struct IndustrialProp(pub Disc);
#[derive(Resource)]
pub struct SceneLibrary {
    pub scenes: Vec<ScenePack>,
    pub current: usize,
    pub initial: usize,
    pub requested: Option<usize>,
    /// Networked games stay on their first scene: every peer builds the
    /// scene's view, and only the host's world is real.
    pub locked: bool,
}
impl SceneLibrary {
    pub fn new() -> Self {
        let scenes = vec![
                ScenePack::decode(mining_deck::PACK_BYTES)
                    .expect("validated mining deck asset pack"),
                ScenePack::decode(boulder_lab::PACK_BYTES)
                    .expect("validated boulder lab asset pack"),
                ScenePack::decode(pile_lab::PACK_BYTES).expect("validated pile lab asset pack"),
                ScenePack::decode(jetpack_lab::PACK_BYTES)
                    .expect("validated jetpack lab asset pack"),
                ScenePack::decode(surprise_lab::PACK_BYTES)
                    .expect("validated field trial asset pack"),
                ScenePack::decode(atlantis::PACK_BYTES).expect("validated Atlantis asset pack"),
                ScenePack::decode(forest_outpost::PACK_BYTES).expect("validated forest asset pack"),
                ScenePack::decode(combat_arena::PACK_BYTES).expect("validated combat asset pack"),
                ScenePack::decode(dig_lab::PACK_BYTES).expect("validated dig lab asset pack"),
                ScenePack::decode(field::PACK_BYTES).expect("validated field asset pack"),
            ];
        let mut ids = std::collections::HashSet::new();
        for pack in &scenes {
            assert!(ids.insert(pack.scene.id.as_str()), "duplicate scene id {}", pack.scene.id);
        }
        Self {
            scenes,
            current: usize::MAX,
            initial: 0,
            requested: None,
            locked: false,
        }
    }
    pub fn index(&self, id: &str) -> Result<usize, String> {
        self.scenes.iter().position(|pack| pack.scene.id == id)
            .ok_or_else(|| {
                let available = self.scenes.iter()
                    .map(|pack| pack.scene.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Unknown scene {id}. Available scenes: {available}")
            })
    }
    pub fn has_feature(&self, feature: SceneFeature) -> bool {
        self.scenes.get(self.current).is_some_and(|pack| pack.scene.features.contains(&feature))
    }
    pub fn extent(&self) -> f32 {
        let Some(scene) = self.scenes.get(self.current).map(|s| &s.scene) else { return 8. };
        if let Some(mine) = &scene.mine {
            return mine.extent;
        }
        scene.scenario.as_ref().map_or(8., |s| s.extent.to_f32())
    }
    pub fn title(&self) -> &str {
        self.scenes
            .get(self.current)
            .map_or("LOADING", |s| s.scene.name.as_str())
    }
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

pub fn switch_scene(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut library: ResMut<SceneLibrary>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<(Entity, &ImpactTarget), With<BoulderInstance>>,
    captions: Query<Entity, With<SampleCaption>>,
    mut industrial: Query<(Entity, &IndustrialProp, &mut Visibility)>,
    mut player: Single<&mut Transform, With<Player>>,
    mut controls: ResMut<Controls>,
    mut feedback: ResMut<Feedback>,
) {
    let requested = if let Some(index) = library.requested.take() {
        Some(index)
    } else if library.locked && library.current != usize::MAX {
        None
    } else if keys.just_pressed(KeyCode::Digit1) {
        Some(0)
    } else if keys.just_pressed(KeyCode::Digit2) {
        Some(1)
    } else if keys.just_pressed(KeyCode::Digit7) {
        Some(6)
    } else if keys.just_pressed(KeyCode::Digit8) {
        Some(7)
    } else if keys.just_pressed(KeyCode::Digit9) {
        Some(8)
    } else if keys.just_pressed(KeyCode::Digit0) {
        Some(9)
    } else if keys.just_pressed(KeyCode::Digit6) {
        Some(5)
    } else if keys.just_pressed(KeyCode::Digit5) {
        Some(4)
    } else if keys.just_pressed(KeyCode::Digit4) {
        Some(3)
    } else if keys.just_pressed(KeyCode::Digit3) {
        Some(2)
    } else if library.current == usize::MAX {
        Some(library.initial)
    } else {
        None
    };
    let Some(index) = requested else {
        return;
    };
    if index == library.current {
        return;
    }
    for (entity, rock) in &old {
        for slot in &rock.slots {
            materials.remove(&slot.handle);
        }
        commands.entity(entity).despawn();
    }
    for caption in &captions {
        commands.entity(caption).despawn();
    }
    library.current = index;
    let scene = &library.scenes[index];
    for (entity, prop, mut visibility) in &mut industrial {
        if scene.scene.industrial {
            *visibility = Visibility::Inherited;
            commands.entity(entity).insert(Obstacle(prop.0));
        } else {
            *visibility = Visibility::Hidden;
            commands.entity(entity).remove::<Obstacle>();
        }
    }
    player.translation = Vec3::from_array(scene.scene.spawn);
    player.rotation = Quat::from_rotation_y(std::f32::consts::PI);
    *controls = Controls::default();
    *feedback = Feedback::default();
    for instance in &scene.scene.instances {
        let prefab = scene
            .prefabs
            .iter()
            .find(|p| p.id == instance.prefab)
            .expect("validated prefab reference");
        let (key, recipe) = match &prefab.geometry {
            Geometry::Baked(_) => (format!("baked:{}", prefab.geometry_key), None),
            Geometry::Runtime(recipe) => {
                let recipe = instance
                    .parameters
                    .as_ref()
                    .map_or_else(|| Ok(recipe.clone()), |p| p.apply(recipe))
                    .expect("validated runtime parameters");
                (format!("runtime:{}", content_key(&recipe)), Some(recipe))
            }
        };
        if !cache.models.contains_key(&key) {
            let model = if let Some(recipe) = recipe {
                cache.runtime_generations += 1;
                generate_boulder(&recipe).expect("validated runtime recipe")
            } else if let Geometry::Baked(model) = &prefab.geometry {
                model.clone()
            } else {
                unreachable!()
            };
            cache.models.insert(key.clone(), upload(model, &mut meshes));
        }
        let model = cache.models[&key].clone();
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
    #[test]
    fn scene_switching_reuses_geometry_and_resets_gameplay() {
        let mut app = App::new();
        app.insert_resource(SceneLibrary::new())
            .init_resource::<ModelCache>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .init_resource::<Feedback>()
            .add_systems(Update, switch_scene);
        app.world_mut().spawn((Player, Transform::default()));
        let disc = Disc {
            center: Vec3::new(-5.0, 0.0, -5.0),
            radius: 1.0,
        };
        let prop = app
            .world_mut()
            .spawn((IndustrialProp(disc), Obstacle(disc), Visibility::Inherited))
            .id();
        app.update();
        assert_eq!(app.world().resource::<SceneLibrary>().current, 0);
        assert_eq!(app.world().resource::<ModelCache>().runtime_generations, 0);
        assert_eq!(
            app.world_mut()
                .query::<&BoulderInstance>()
                .iter(app.world())
                .count(),
            6
        );
        let baseline_materials = app.world().resource::<Assets<StandardMaterial>>().len();
        for index in [1, 0, 1] {
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.reset_all();
                keys.press(if index == 0 {
                    KeyCode::Digit1
                } else {
                    KeyCode::Digit2
                });
            }
            app.world_mut().resource_mut::<Controls>().impacts = 5;
            app.world_mut().resource_mut::<Controls>().last_material =
                Some(voxel_assets::ImpactResponse::Metal);
            app.world_mut().resource_mut::<Controls>().velocity = Vec3::X;
            app.update();
            assert_eq!(app.world().resource::<SceneLibrary>().current, index);
            assert_eq!(app.world().resource::<ModelCache>().runtime_generations, 2);
            assert_eq!(app.world().resource::<Controls>().impacts, 0);
            assert!(app.world().resource::<Controls>().last_material.is_none());
            assert_eq!(
                app.world_mut()
                    .query::<&SampleCaption>()
                    .iter(app.world())
                    .count(),
                if index == 0 { 0 } else { 4 }
            );
            assert_eq!(app.world().resource::<Controls>().velocity, Vec3::ZERO);
            assert_eq!(
                app.world_mut()
                    .query::<&BoulderInstance>()
                    .iter(app.world())
                    .count(),
                if index == 0 { 6 } else { 10 }
            );
            assert_eq!(
                app.world().resource::<Assets<StandardMaterial>>().len(),
                baseline_materials + if index == 0 { 0 } else { 16 }
            );
            assert_eq!(app.world().get::<Obstacle>(prop).is_some(), index == 0);
        }
        assert_eq!(
            *app.world().get::<Visibility>(prop).unwrap(),
            Visibility::Hidden
        );
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
        assert_eq!(
            library.index("missing"),
            Err("Unknown scene missing. Available scenes: mining-deck, boulder-lab, pile-lab, jetpack-lab, surprise-lab, atlantis, forest-outpost, combat-arena, dig-lab, field".into())
        );
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

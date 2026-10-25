//! Bevy adapter for baked prefabs and the shared runtime boulder generator.
use crate::{
    character_catalog::{CharacterCatalog, CharacterEntry},
    effects::Feedback,
    combat::Obstacle, gameplay::Controls,
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
    /// Every character Character Studio can show, found in the scenes.
    pub characters: CharacterCatalog,
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
            ScenePack::decode(moonlit_glade::PACK_BYTES).expect("validated moonlit glade asset pack"),
            ScenePack::decode(laser_pit::PACK_BYTES).expect("validated laser pit asset pack"),
            ScenePack::decode(asset_guide::PACK_BYTES).expect("validated asset guide pack"),
        ]
        .into_iter()
        .map(Arc::new)
        .collect();
        let mut ids = std::collections::HashSet::new();
        for pack in &scenes {
            assert!(ids.insert(pack.scene.id.as_str()), "duplicate scene id {}", pack.scene.id);
        }
        let characters = CharacterCatalog::discover(&scenes);
        Self { scenes, characters }
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
    pub fn character(&self, scene_index: usize) -> Option<&CharacterEntry> {
        self.characters.for_scene(scene_index).or_else(|| {
            self.scenes.get(scene_index)
                .filter(|pack| pack.scene.style.is_none() && pack.actor.is_empty())
                .and_then(|_| self.characters.built_in())
        })
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
    controls.swift_sword = crate::lockstep::builtin_character(&scene.scene.player).swift_sword();
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
        let discovered: Vec<_> = library.scenes.iter().enumerate()
            .filter(|(_, pack)| pack.scene.style.is_some() && !pack.actor.is_empty())
            .map(|(index, _)| index)
            .collect();
        assert!(!discovered.is_empty());
        for entry in library.characters.iter() {
            assert_eq!(library.characters.index_for_id(&entry.id).map(|index| library.characters[index].scene_index), Ok(entry.scene_index));
        }
        let distinct_styles: std::collections::HashSet<_> = discovered.iter()
            .map(|&index| library.scenes[index].scene.style.as_ref().unwrap().id.as_str())
            .collect();
        assert!(library.characters.len() >= distinct_styles.len());
        for id in ["skin:ox", "skin:ghoul", "skin:ghoul_brute"] {
            assert!(library.characters.index_for_id(id).is_ok(), "missing {id}");
        }
        let exosuit = library.characters.index_for_id("builtin:exosuit").unwrap();
        let field = library.index("field").unwrap();
        assert_eq!(library.character(field).unwrap().id, library.characters[exosuit].id);
        assert!(library.characters[exosuit].can_locomote);
        assert!(!library.characters[exosuit].can_jump, "the exosuit flies");
        let warden = library.characters.iter().find(|c| c.name == "STARWELL WARDEN").expect("the glade's actor");
        assert!(warden.can_jump, "the glade exosuit jumps");
        let mining = &library.scenes[0];
        let lab = &library.scenes[1];
        assert_eq!(library.index("mining-deck"), Ok(0));
        assert_eq!(library.index("combat-arena"), Ok(7));
        assert_eq!(library.index("dig-lab"), Ok(8));
        assert_eq!(library.index("field"), Ok(9));
        assert_eq!(library.index("moonlit-glade"), Ok(10));
        assert_eq!(library.index("laser-pit"), Ok(11));
        let laser = &library.scenes[11];
        assert_eq!(laser.scene.scenario.as_ref().unwrap().player_health, 6);
        assert_eq!(laser.scene.scenario.as_ref().unwrap().weapons, [content::scenario::Weapon::Laser]);
        assert!(laser.scene.terrain.is_some());
        assert!(laser.environment.as_ref().is_some_and(|env| env.obstacles.len() >= 7));
        let glade = &library.scenes[10];
        let blade_tip = glade
            .actor
            .iter()
            .find(|part| part.part == voxel_assets::ActorPart::Weapon)
            .unwrap()
            .meshes
            .iter()
            .flat_map(|mesh| mesh.positions.iter().map(|position| position[2]))
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(blade_tip > 0.8, "voxel sword blade was lost during baking");
        assert_eq!(glade.scene.player, "exosuit_moonlit");
        for pack in &library.scenes {
            let motion = pack.scene.style.as_ref().map(|s| s.character_motion).unwrap_or_default();
            let player = crate::lockstep::builtin_character(&pack.scene.player);
            let close = |view: f32, sim: fx::Fx| (view - crate::lockstep::view_f32(sim)).abs() < 1e-3;
            assert!(
                close(motion.walk_speed, player.movement.walk) && close(motion.run_speed, player.movement.run),
                "{}: the style poses other speeds than its player moves at",
                pack.scene.id
            );
        }
        assert_eq!(glade.scene.instances.len(), 7);
        assert!(glade.environment.as_ref().is_some_and(|environment| environment.walkable.len() >= 14));
        let Geometry::Baked(rock) = &glade.prefabs[0].geometry else { panic!("glade rock should be baked"); };
        assert!(rock.collision.radius > 0.5);
        assert_eq!(glade.prefabs[0].palette.materials[3].impact, voxel_assets::ImpactResponse::Crystal);
        assert!(library.scenes[8].scene.features.contains(&SceneFeature::Digging));
        assert_eq!(
            library.index("missing"),
            Err("Unknown scene missing. Available scenes: mining-deck, boulder-lab, pile-lab, jetpack-lab, surprise-lab, atlantis, forest-outpost, combat-arena, dig-lab, field, moonlit-glade, laser-pit, asset-guide".into())
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

    #[test]
    fn laser_pit_starts_with_a_healthy_armed_player() {
        let options = crate::control::Options { simulation: true, ..default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let library = SceneLibrary::new();
        let scene = library.index("laser-pit").unwrap();
        let mut app = crate::test_app(options, library, hub, crate::flow::Launch::single_player(scene));
        crate::start_world(&mut app);
        let platform_visuals = app.world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<crate::jetpack::TerrainVisual>>()
            .iter(app.world())
            .count();
        assert_eq!(platform_visuals, 5, "authored floor must replace terrain top tiles");
        let service = app.world().resource::<crate::lockstep::SessionService>();
        let settlement = service.settlement().unwrap();
        let player = &settlement.agents[settlement.player().unwrap() as usize];
        assert_eq!(player.avatar.health, 6);
        assert_eq!(player.avatar.weapon, content::scenario::Weapon::Laser);
        assert_eq!(app.world().resource::<crate::gameplay::Controls>().weapon, content::scenario::Weapon::Laser);
        let terrain = service.world().unwrap().terrain();
        assert!(terrain.mine.is_none());
        assert!(terrain.obstacles.len() >= 7, "arena cover must block movement and fire");
        let at = |x, z| service.to_sim(Vec3::new(x, 0.0, z));
        assert!(!terrain.clear(at(0.0, 11.5), at(0.0, -11.5), fx::Fx::from_ratio(1, 50)));
        assert!(terrain.clear(at(2.3, 11.5), at(2.3, -11.5), fx::Fx::from_ratio(1, 50)));
        assert!(!terrain.clear(at(0.0, 13.0), at(0.0, 14.8), fx::Fx::from_ratio(8, 25)),
            "the low front rim must block movement");
        assert!(!terrain.clear(at(-13.0, 10.0), at(-14.8, 10.0), fx::Fx::from_ratio(8, 25)),
            "the side wall must block movement");
        let route = sim::nav::path(terrain, at(0.0, 11.5), at(0.0, -11.5), fx::Fx::from_ratio(8, 25));
        assert!(route.len() > 1, "fighters must be able to flank central cover");
    }
}

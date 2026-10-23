//! Data-driven visual styles. Compiled geometry and material handles survive scene switches.
use crate::{assets::SceneLibrary, gameplay::Obstacle, navigation::Disc};
use bevy::{
    pbr::{DistanceFog, FogFalloff},
    post_process::bloom::Bloom,
    prelude::*,
};
use std::collections::HashMap;
use voxel_assets::content_key;
#[derive(Resource, Default)]
pub struct ActiveStyle {
    current: Option<usize>,
    cache: HashMap<String, Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    palettes: HashMap<String, Vec<Handle<StandardMaterial>>>,
}
#[derive(Resource)]
pub struct MaterialBindings(pub Vec<(Handle<StandardMaterial>, String, StandardMaterial)>);

#[derive(Component)]
struct Environment;
#[derive(Component)]
struct StyledActor;
#[derive(Component)]
pub struct WeaponLabel;
#[derive(Component)]
pub struct AccentLight {
    pub role: usize,
    pub intensity: f32,
}
#[derive(Component)]
struct StyleLabel;
fn color(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}
pub fn apply(world: &mut World) {
    let index = world.resource::<SceneLibrary>().current;
    if world.resource::<ActiveStyle>().current == Some(index) {
        return;
    }
    let scene = &world.resource::<SceneLibrary>().scenes[index];
    let styled = scene.scene.style.is_some();
    let profile = scene.scene.style.clone().unwrap_or_default();
    let environment = scene.environment.clone();
    let actor = scene.actor.clone();
    let fixture_lighting = environment.as_ref().is_some_and(|e| !e.lights.is_empty());
    let hide_deck = environment.is_some() || scene.scene.terrain.is_some();
    let old_actors: Vec<_> = world
        .query_filtered::<Entity, With<StyledActor>>()
        .iter(world)
        .collect();
    for e in old_actors {
        world.despawn(e);
    }
    let old: Vec<_> = world
        .query_filtered::<Entity, With<Environment>>()
        .iter(world)
        .collect();
    for e in old {
        world.despawn(e);
    }
    for mut visibility in world
        .query_filtered::<&mut Visibility, With<crate::jetpack::Deck>>()
        .iter_mut(world)
    {
        *visibility = if hide_deck {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    if let Some(bindings) = world.get_resource::<MaterialBindings>() {
        let updates: Vec<_> = bindings
            .0
            .iter()
            .map(|(handle, role, original)| {
                let material = if styled {
                    profile
                        .materials
                        .iter()
                        .find(|m| m.name == *role)
                        .map(crate::assets::standard)
                        .unwrap_or_else(|| original.clone())
                } else {
                    original.clone()
                };
                (handle.clone(), material)
            })
            .collect();
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        for (handle, material) in updates {
            if let Some(target) = materials.get_mut(&handle) {
                *target = material;
            }
        }
    }
    world.resource_mut::<ClearColor>().0 = color(profile.background);
    let mut ambient = world.resource_mut::<GlobalAmbientLight>();
    ambient.color = color(profile.ambient);
    ambient.brightness = profile.ambient_brightness;
    for mut light in world.query::<&mut DirectionalLight>().iter_mut(world) {
        light.color = color(profile.sunlight);
        light.illuminance = profile.illuminance;
        light.shadows_enabled = profile.sun_shadows;
    }
    // Authored key lights keep their positions and intensities; profiles supply their color roles.
    for (role, mut light) in world
        .query::<(&AccentLight, &mut PointLight)>()
        .iter_mut(world)
    {
        light.color = color(profile.accents[role.role]);
        light.intensity = if fixture_lighting {
            0.0
        } else {
            role.intensity
        };
    }
    world.insert_resource(bevy::light::PointLightShadowMap {
        size: profile.shadow_map_size as usize,
    });
    let cameras: Vec<_> = world
        .query_filtered::<Entity, With<Camera3d>>()
        .iter(world)
        .collect();
    for e in cameras {
        if let Some(mut ao) = world.get_mut::<bevy::pbr::ScreenSpaceAmbientOcclusion>(e) {
            ao.constant_object_thickness = profile.ao_thickness;
        }
        world.entity_mut(e).insert(bevy::camera::Exposure {
            ev100: profile.exposure_ev100,
        });
        world.entity_mut(e).insert(DistanceFog {
            color: color(profile.background),
            falloff: FogFalloff::Exponential {
                density: profile.fog_density,
            },
            ..default()
        });
        if let Some(mut bloom) = world.get_mut::<Bloom>(e) {
            bloom.intensity = profile.bloom;
        }
        if let Some(mut rig) = world.get_mut::<crate::effects::CameraRig>(e) {
            rig.zoom = profile.viewport_height;
        }
    }
    if let Some(environment) = environment {
        let key = content_key(&(&profile, &environment));
        let handles = upload(world, key, &environment.meshes, &profile);
        for (mesh, material) in handles {
            let emitting = world
                .resource::<Assets<StandardMaterial>>()
                .get(&material)
                .is_some_and(|m| m.emissive.red + m.emissive.green + m.emissive.blue > 0.0);
            let e = world
                .spawn((
                    Environment,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::default(),
                ))
                .id();
            if emitting {
                world.entity_mut(e).insert(bevy::light::NotShadowCaster);
            }
        }
        for light in &environment.lights {
            spawn_light(world, light, None, &profile);
        }
        for (position, radius) in environment.obstacles {
            world.spawn((
                Environment,
                Obstacle(Disc {
                    center: Vec3::from_array(position),
                    radius,
                }),
            ));
        }
    }
    use voxel_assets::ActorPart;
    let parts: HashMap<_, _> = world
        .query::<(Entity, &crate::gameplay::Part)>()
        .iter(world)
        .map(|(e, p)| {
            let kind = match p.kind {
                crate::gameplay::PartKind::Body => ActorPart::Body,
                crate::gameplay::PartKind::Leg(side) => {
                    if side < 0.0 {
                        ActorPart::LeftLeg
                    } else {
                        ActorPart::RightLeg
                    }
                }
                crate::gameplay::PartKind::Sword => ActorPart::Weapon,
            };
            (e, kind)
        })
        .collect();
    let player = world
        .query_filtered::<Entity, With<crate::gameplay::Player>>()
        .single(world)
        .unwrap();
    world.get_mut::<Transform>(player).unwrap().scale = Vec3::splat(profile.actor_scale);
    for (parent,mut visibility,exhaust) in world.query_filtered::<(&ChildOf,&mut Visibility,Option<&crate::jetpack::Exhaust>),With<Mesh3d>>().iter_mut(world) {
        let hide=parts.get(&parent.parent()).is_some_and(|part|actor.iter().any(|a|a.part==*part)) || (parent.parent()==player && exhaust.is_none() && actor.iter().any(|a|a.part==ActorPart::Body));
        *visibility=if hide {Visibility::Hidden} else {Visibility::Inherited};
    }
    for a in &actor {
        let key = content_key(&(&profile, a));
        let handles = upload(world, key, &a.meshes, &profile);
        for (&parent, part) in &parts {
            if *part == a.part {
                for light in &a.lights {
                    spawn_light(world, light, Some(parent), &profile);
                }
                for (mesh, material) in &handles {
                    let e = world
                        .spawn((
                            StyledActor,
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::default(),
                        ))
                        .id();
                    let material = world.get::<MeshMaterial3d<StandardMaterial>>(e).unwrap();
                    let emitting = world
                        .resource::<Assets<StandardMaterial>>()
                        .get(&material.0)
                        .is_some_and(|m| m.emissive.red + m.emissive.green + m.emissive.blue > 0.0);
                    if emitting {
                        world.entity_mut(e).insert(bevy::light::NotShadowCaster);
                    }
                    world.entity_mut(parent).add_child(e);
                }
            }
        }
    }
    for mut label in world
        .query_filtered::<&mut Text, With<WeaponLabel>>()
        .iter_mut(world)
    {
        label.0 = profile.weapon_name.clone();
    }
    let labels: Vec<_> = world
        .query_filtered::<Entity, With<StyleLabel>>()
        .iter(world)
        .collect();
    for e in labels {
        world.despawn(e);
    }
    world.spawn((
        StyleLabel,
        Text::new(format!(
            "STYLE {}  /  {:.0} VOXELS PER METER",
            profile.id.to_uppercase(),
            1.0 / profile.voxel_size
        )),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(color(profile.accents[0])),
        Node {
            position_type: PositionType::Absolute,
            left: px(36),
            top: px(112),
            ..default()
        },
    ));
    world.resource_mut::<ActiveStyle>().current = Some(index);
}
fn spawn_light(
    world: &mut World,
    light: &voxel_assets::BakedLight,
    parent: Option<Entity>,
    profile: &voxel_assets::StyleProfile,
) {
    let transform = Transform::from_translation(Vec3::from_array(light.position));
    let e = match &light.kind {
        voxel_assets::LightKind::Point => world
            .spawn((
                PointLight {
                    color: color(light.color),
                    intensity: light.intensity,
                    range: light.range,
                    radius: light.radius,
                    shadows_enabled: light.shadows,
                    shadow_depth_bias: profile.shadow_depth_bias,
                    shadow_normal_bias: profile.shadow_normal_bias,
                    shadow_map_near_z: 0.05,
                    ..default()
                },
                transform,
            ))
            .id(),
        voxel_assets::LightKind::Spot {
            direction,
            inner,
            outer,
        } => world
            .spawn((
                SpotLight {
                    color: color(light.color),
                    intensity: light.intensity,
                    range: light.range,
                    radius: light.radius,
                    shadows_enabled: light.shadows,
                    shadow_depth_bias: profile.shadow_depth_bias,
                    shadow_normal_bias: profile.shadow_normal_bias,
                    shadow_map_near_z: 0.05,
                    inner_angle: *inner,
                    outer_angle: *outer,
                    ..default()
                },
                transform.looking_to(Vec3::from_array(*direction), Vec3::Y),
            ))
            .id(),
    };
    if let Some(parent) = parent {
        world.entity_mut(e).insert(StyledActor);
        world.entity_mut(parent).add_child(e);
    } else {
        world.entity_mut(e).insert(Environment);
    }
}
pub(crate) fn upload(
    world: &mut World,
    key: String,
    meshes: &[voxel_assets::MeshData],
    profile: &voxel_assets::StyleProfile,
) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
    if let Some(handles) = world.resource::<ActiveStyle>().cache.get(&key) {
        return handles.clone();
    }
    let palette_key = content_key(&profile.materials);
    let materials =
        if let Some(materials) = world.resource::<ActiveStyle>().palettes.get(&palette_key) {
            materials.clone()
        } else {
            let materials: Vec<_> = profile
                .materials
                .iter()
                .map(|m| {
                    world
                        .resource_mut::<Assets<StandardMaterial>>()
                        .add(crate::assets::standard(m))
                })
                .collect();
            world
                .resource_mut::<ActiveStyle>()
                .palettes
                .insert(palette_key, materials.clone());
            materials
        };
    let handles: Vec<_> = meshes
        .iter()
        .map(|m| {
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(crate::assets::mesh(m));
            (mesh, materials[m.material].clone())
        })
        .collect();
    world
        .resource_mut::<ActiveStyle>()
        .cache
        .insert(key, handles.clone());
    handles
}
fn world_position(world: &World, mut entity: Entity) -> Vec3 {
    let mut p = Vec3::ZERO;
    for _ in 0..16 {
        if let Some(t) = world.get::<Transform>(entity) {
            p = t.compute_affine().transform_point3(p);
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            break;
        };
        entity = parent.parent();
    }
    p
}
pub fn snapshot(world: &mut World) -> serde_json::Value {
    let library = world.resource::<SceneLibrary>();
    let pack = &library.scenes[library.current];
    let p = pack.scene.style.clone().unwrap_or_default();
    let mut info = serde_json::json!({"id":p.id,"voxel_size":p.voxel_size,"voxels_per_meter":1.0/p.voxel_size,"palette":p.materials.iter().map(|m| &m.name).collect::<Vec<_>>(),"environment_statistics":pack.environment.as_ref().map(|e|e.statistics),"baked":pack.environment.is_some(),"actor_voxel_size":p.actor_voxel_size,"actor_parts":pack.actor.iter().map(|a|a.part).collect::<Vec<_>>(),"weapon":p.weapon_name,"fixture_lights":pack.environment.as_ref().map(|e|&e.lights),"sun_shadows":p.sun_shadows,"exposure_ev100":p.exposure_ev100});
    let moving:Vec<_>=world.query_filtered::<Entity,With<StyledActor>>().iter(world).filter_map(|e| {
        let (kind,intensity,range,shadows)=if let Some(l)=world.get::<PointLight>(e) {("point",l.intensity,l.range,l.shadows_enabled)} else if let Some(l)=world.get::<SpotLight>(e) {("spot",l.intensity,l.range,l.shadows_enabled)} else {return None;};
        Some(serde_json::json!({"kind":kind,"position":world_position(world,e).to_array(),"intensity":intensity,"range":range,"shadows":shadows}))
    }).collect();
    info["actor_lights"] = serde_json::json!(moving);
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scene_styles_restore_materials_and_lights_and_reuse_uploaded_assets() {
        let options = crate::control::Options {
            simulation: true,
            ..default()
        };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::build_app(options, SceneLibrary::new(), hub);
        app.update();
        let originals: Vec<_> = app
            .world()
            .resource::<MaterialBindings>()
            .0
            .iter()
            .map(|(h, _, m)| (h.clone(), m.base_color, m.emissive))
            .collect();
        let mut counts = None;
        for index in [5, 0, 5, 0, 5] {
            app.world_mut().resource_mut::<SceneLibrary>().requested = Some(index);
            app.update();
            let world = app.world_mut();
            let info = snapshot(world);
            assert_eq!(
                info["id"],
                if index == 5 {
                    "atlantean-aetherworks"
                } else {
                    "kestrel-industrial"
                }
            );
            assert_eq!(info["baked"], index == 5);
            let actor_count = world
                .query_filtered::<Entity, With<StyledActor>>()
                .iter(world)
                .count();
            assert_eq!(actor_count > 0, index == 5);
            let label = world
                .query_filtered::<&Text, With<WeaponLabel>>()
                .single(world)
                .unwrap();
            assert_eq!(
                label.0,
                if index == 5 {
                    "TIDEBINDER / AETHER TRIDENT"
                } else {
                    "MK. IV / INDUSTRIAL CLEAVER"
                }
            );
            let environment = world
                .query_filtered::<Entity, With<Environment>>()
                .iter(world)
                .count();
            assert_eq!(environment > 0, index == 5);
            for visibility in world
                .query_filtered::<&Visibility, With<crate::jetpack::Deck>>()
                .iter(world)
            {
                assert_eq!(*visibility == Visibility::Hidden, index == 5);
            }
            if index == 5 {
                assert!((info["voxel_size"].as_f64().unwrap() - 0.1).abs() < 0.0001);
                assert_eq!(
                    world
                        .resource::<crate::assets::ModelCache>()
                        .runtime_generations,
                    0
                );
                assert_eq!(info["actor_lights"].as_array().unwrap().len(), 2);
                let helmet = info["actor_lights"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|l| l["kind"] == "spot")
                    .unwrap();
                assert_eq!(helmet["shadows"], true);
                for light in world.query::<&SpotLight>().iter(world) {
                    assert!(light.shadows_enabled);
                    assert!((light.shadow_depth_bias - 0.008).abs() < 0.0001);
                }
                for part in &world.resource::<SceneLibrary>().scenes[index].actor {
                    if matches!(
                        part.part,
                        voxel_assets::ActorPart::LeftLeg | voxel_assets::ActorPart::RightLeg
                    ) {
                        let sole = part
                            .meshes
                            .iter()
                            .flat_map(|m| m.positions.iter())
                            .map(|p| p[1])
                            .fold(f32::INFINITY, f32::min)
                            + 0.58;
                        assert!(
                            (-0.05..=0.005).contains(&sole),
                            "sole must contact floor: {sole}"
                        );
                    }
                }

                assert_eq!(
                    info["fixture_lights"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|l| l["shadows"] == true)
                        .count(),
                    2
                );
                for light in world.query::<&DirectionalLight>().iter(world) {
                    assert_eq!(light.illuminance, 0.0);
                    assert!(!light.shadows_enabled);
                }
                for (_, light) in world.query::<(&AccentLight, &PointLight)>().iter(world) {
                    assert_eq!(light.intensity, 0.0);
                }
                let before = info["actor_lights"][0]["position"][0].as_f64().unwrap();
                let player = world
                    .query_filtered::<Entity, With<crate::gameplay::Player>>()
                    .single(world)
                    .unwrap();
                world.get_mut::<Transform>(player).unwrap().translation.x += 1.0;
                let moved = snapshot(world);
                assert!(
                    (moved["actor_lights"][0]["position"][0].as_f64().unwrap() - before - 1.0)
                        .abs()
                        < 0.001
                );
                world.get_mut::<Transform>(player).unwrap().translation.x -= 1.0;
                let now = (
                    world.resource::<Assets<Mesh>>().len(),
                    world.resource::<Assets<StandardMaterial>>().len(),
                );
                if let Some(counts) = counts {
                    assert_eq!(now, counts);
                } else {
                    counts = Some(now);
                }
            } else {
                let materials = world.resource::<Assets<StandardMaterial>>();
                for (h, color, emissive) in &originals {
                    let m = materials.get(h).unwrap();
                    assert_eq!(m.base_color, *color);
                    assert_eq!(m.emissive, *emissive);
                }
                assert_eq!(world.resource::<GlobalAmbientLight>().brightness, 180.0);
                for light in world.query::<&DirectionalLight>().iter(world) {
                    assert_eq!(light.illuminance, 10500.0);
                    assert!(light.shadows_enabled);
                }
            }
        }
        assert_eq!(app.world().resource::<ActiveStyle>().cache.len(), 5);
    }
}

pub fn cache_metrics(world: &World) -> (usize, usize) {
    let a = world.resource::<ActiveStyle>();
    (a.cache.len(), a.palettes.len())
}

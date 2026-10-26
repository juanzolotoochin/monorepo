//! Data-driven visual styles. Compiled geometry and material handles survive scene switches.
use crate::{combat::Obstacle, navigation::Disc};
use service::WorldScoped;
use bevy::{
    pbr::{DistanceFog, FogFalloff},
    post_process::bloom::Bloom,
    prelude::*,
};
use std::collections::HashMap;
use voxel_assets::content_key;
/// Uploaded style geometry and palettes, kept across worlds.
#[derive(Resource, Default)]
pub struct ActiveStyle {
    cache: HashMap<String, Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    palettes: HashMap<String, Vec<Handle<StandardMaterial>>>,
}
#[derive(Resource)]
pub struct MaterialBindings(pub Vec<(Handle<StandardMaterial>, String, StandardMaterial)>);

#[derive(Component)]
struct Environment;
#[derive(Component)]
pub struct DestructibleView {
    pub index: usize,
    pub last_health: u16,
    pub max_health: u16,
}
#[derive(Component)]
pub struct DamageMaterial(pub Color);
#[derive(Component)]
struct StyledActor;
#[derive(Component)]
pub struct WeaponVisual(pub content::scenario::Weapon);
#[derive(Component, Clone, Copy)]
pub struct WeaponMuzzle(pub content::scenario::Weapon);
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
/// Resolve authored root-space joint origins into local bind transforms.
/// Unspecified joints inherit their original offsets, so older skins retain
/// the scene rig and descendants follow a moved shoulder or hip naturally.
fn apply_actor_pivots(
    world: &mut World,
    parts: &HashMap<Entity, voxel_assets::ActorPart>,
    actor: &[voxel_assets::ActorMesh],
) {
    fn position(
        entity: Entity,
        binds: &HashMap<Entity, (Vec3, Option<Entity>)>,
        parts: &HashMap<Entity, voxel_assets::ActorPart>,
        actor: &[voxel_assets::ActorMesh],
        resolved: &mut HashMap<Entity, Vec3>,
    ) -> Vec3 {
        if let Some(value) = resolved.get(&entity) {
            return *value;
        }
        let (bind, parent) = binds[&entity];
        let parent_position = parent
            .filter(|parent| binds.contains_key(parent))
            .map_or(Vec3::ZERO, |parent| position(parent, binds, parts, actor, resolved));
        // The weapon's authored pivot describes its model, while WeaponGrip
        // must always remain attached to the hand.
        let pivot = parts.get(&entity).filter(|part| **part != voxel_assets::ActorPart::Weapon)
            .and_then(|part| actor.iter().find(|mesh| mesh.part == *part))
            .and_then(|mesh| mesh.pivot)
            .map(Vec3::from_array);
        let value = pivot.unwrap_or(parent_position + bind);
        resolved.insert(entity, value);
        value
    }

    let binds: HashMap<_, _> = world
        .query::<(Entity, &crate::character::Part, Option<&ChildOf>)>()
        .iter(world)
        .map(|(entity, part, parent)| {
            (entity, (part.bind.translation, parent.map(ChildOf::parent)))
        })
        .collect();
    let mut resolved = HashMap::new();
    for &entity in binds.keys() {
        position(entity, &binds, parts, actor, &mut resolved);
    }
    for (&entity, &global) in &resolved {
        let parent = binds[&entity].1;
        let parent_global = parent.and_then(|parent| resolved.get(&parent).copied()).unwrap_or(Vec3::ZERO);
        let local = global - parent_global;
        let rest = {
            let mut part = world.get_mut::<crate::character::Part>(entity).unwrap();
            part.rest = part.bind;
            part.rest.translation = local;
            part.rest
        };
        *world.get_mut::<Transform>(entity).unwrap() = rest;
    }
}
/// Dresses a world in its scene's style: materials, lights, fog, the
/// environment and the player's styled parts. Runs when the world starts
/// (see `SceneView`), after the player's body exists. Character Studio
/// (`studio`) dresses only the character, on its own stage.
pub fn apply(world: &mut World, scene: &voxel_assets::ScenePack, studio: bool) {
    let styled = scene.scene.style.is_some();
    let profile = scene.scene.style.clone().unwrap_or_default();
    let environment = if studio { None } else { scene.environment.clone() };
    let actor = scene.actor.clone();
    let skins = scene.skins.clone();
    let available_weapons = scene
        .scene
        .scenario
        .as_ref()
        .map_or(vec![], |s| s.weapons.clone());
    let fixture_lighting = environment.as_ref().is_some_and(|e| !e.lights.is_empty());

    let settlement = scene.scene.scenario.is_some();
    let jumping = crate::lockstep::builtin_character(&scene.scene.player).jump().is_some();
    world.insert_resource(crate::character::ActorFootContacts::from_actor(&actor));
    for mut visible in world
        .query_filtered::<&mut Visibility, Or<(
            With<crate::scene::CombatPanel>,
            With<crate::scene::ContactPanel>,
        )>>()
        .iter_mut(world)
    {
        *visible = if settlement {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for mut help in world
        .query_filtered::<&mut Text, With<crate::scene::ControlHelp>>()
        .iter_mut(world)
    {
        help.0 = if available_weapons == [content::scenario::Weapon::Laser] {
            "RMB MOVE / WASD RUN / SHIFT+LMB FIRE LASER"
        } else if jumping {
            "RMB MOVE / WASD RUN / SPACE JUMP / SHIFT+LMB ATTACK"
        } else if !available_weapons.is_empty() {
            "RMB MOVE / SHIFT+LMB ATTACK / Z SWORD / X SPEAR / C BOW / V ARC"
        } else if settlement {
            "LMB STOCK / CTRL DEPOSIT / E MACHINE / SHIFT+LMB ATTACK"
        } else {
            "RMB MOVE / SHIFT+LMB STRIKE"
        }
        .into();
    }
    for mut help in world
        .query_filtered::<&mut Text, With<crate::scene::SceneShortcutHelp>>()
        .iter_mut(world)
    {
        help.0 = if jumping {
            "1 - 9 / 0  SCENES    F4  NEXT SCENE    SPACE  JUMP    WASD  STEER    SHIFT  RUN    WHEEL  ZOOM    M  AUDIO"
        } else {
            "1 - 9 / 0  SCENES    F4  NEXT SCENE    SPACE  JETPACK    WASD  STEER    SHIFT  RUN    WHEEL  ZOOM    M  AUDIO"
        }
        .into();
    }
    if studio {
        // Character Studio has its own stage instead of the built-in deck.
        for mut visibility in world
            .query_filtered::<&mut Visibility, With<crate::jetpack::Deck>>()
            .iter_mut(world)
        {
            *visibility = Visibility::Hidden;
        }
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
            if let Some(mut target) = materials.get_mut(&handle) {
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
        light.shadow_maps_enabled = profile.sun_shadows;
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
                    WorldScoped::default(),
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
                WorldScoped::default(),
                Obstacle(Disc {
                    center: Vec3::from_array(position),
                    radius,
                }),
            ));
        }
    }
    if !studio {
        let initial_health: Vec<u16> = world.resource::<crate::lockstep::SessionService>().world()
            .map(|sim| sim.terrain().destructibles.iter().map(|piece| piece.health).collect())
            .unwrap_or_default();
        for (index, piece) in scene.destructibles.iter().enumerate() {
            let health = initial_health.get(index).copied().unwrap_or(piece.health);
            let root = world.spawn((Environment, WorldScoped::default(), DestructibleView {
                index, last_health: health, max_health: piece.health,
            },
                Transform::from_translation(Vec3::from_array(piece.center)),
                if health > 0 { Visibility::Inherited } else { Visibility::Hidden })).id();
            if health > 0 {
                world.entity_mut(root).insert(Obstacle(Disc { center: Vec3::from_array(piece.center), radius: piece.radius }));
            }
            let key = content_key(&(&profile, &piece.meshes));
            for (mesh, material) in upload(world, key, &piece.meshes, &profile) {
                let original = world.resource::<Assets<StandardMaterial>>().get(&material).expect("uploaded material").clone();
                let mut appearance = original.clone();
                let base = original.base_color.to_srgba();
                let factor = 0.55 + 0.45 * f32::from(health) / f32::from(piece.health);
                appearance.base_color = Color::srgba(base.red * factor, base.green * factor, base.blue * factor, base.alpha);
                let unique = world.resource_mut::<Assets<StandardMaterial>>().add(appearance);
                let child = world.spawn((Mesh3d(mesh), MeshMaterial3d(unique), DamageMaterial(original.base_color),
                    Transform::from_translation(-Vec3::from_array(piece.center)))).id();
                world.entity_mut(root).add_child(child);
            }
        }
    }
    use voxel_assets::ActorPart;
    let kinds: HashMap<_, _> = world
        .query::<(Entity, &crate::character::Part)>()
        .iter(world)
        .map(|(e, p)| (e, p.kind))
        .collect();
    let parts: HashMap<_, _> = kinds
        .iter()
        .filter_map(|(&e, kind)| {
            let part = match kind {
                crate::character::PartKind::Body => Some(ActorPart::Body),
                crate::character::PartKind::Chest => Some(ActorPart::Chest),
                crate::character::PartKind::Head => Some(ActorPart::Head),
                crate::character::PartKind::Leg(side) => Some({
                    if *side < 0.0 {
                        ActorPart::LeftLeg
                    } else {
                        ActorPart::RightLeg
                    }
                }),
                crate::character::PartKind::Knee(side) => Some(if *side < 0.0 {
                    ActorPart::LeftKnee
                } else {
                    ActorPart::RightKnee
                }),
                crate::character::PartKind::Arm(side) => Some(if *side < 0.0 {
                    ActorPart::LeftArm
                } else {
                    ActorPart::RightArm
                }),
                crate::character::PartKind::Forearm(side) => Some(if *side < 0.0 {
                    ActorPart::LeftForearm
                } else {
                    ActorPart::RightForearm
                }),
                crate::character::PartKind::Hand(side) => Some(if *side < 0.0 {
                    ActorPart::LeftHand
                } else {
                    ActorPart::RightHand
                }),
                crate::character::PartKind::Shin(side) => Some(if *side < 0.0 {
                    ActorPart::LeftShin
                } else {
                    ActorPart::RightShin
                }),
                crate::character::PartKind::Foot(side) => Some(if *side < 0.0 {
                    ActorPart::LeftFoot
                } else {
                    ActorPart::RightFoot
                }),
                crate::character::PartKind::WeaponGrip => Some(ActorPart::Weapon),
            };
            part.map(|part| (e, part))
        })
        .collect();
    apply_actor_pivots(world, &parts, &actor);
    let player = world
        .query_filtered::<Entity, With<crate::character::Player>>()
        .single(world)
        .unwrap();
    world.get_mut::<Transform>(player).unwrap().scale = Vec3::splat(profile.actor_scale);
    for (parent,mut visibility,exhaust) in world.query_filtered::<(&ChildOf,&mut Visibility,Option<&crate::jetpack::Exhaust>),With<Mesh3d>>().iter_mut(world) {
        let overridden = kinds.get(&parent.parent()).is_some_and(|kind| {
            let exact = parts.get(&parent.parent()).copied();
            let fallback = match kind {
                crate::character::PartKind::Head | crate::character::PartKind::Arm(_) => Some(ActorPart::Body),
                crate::character::PartKind::Chest => Some(ActorPart::Body),
                crate::character::PartKind::Forearm(side) | crate::character::PartKind::Hand(side) => {
                    let arm = if *side < 0.0 { ActorPart::LeftArm } else { ActorPart::RightArm };
                    if actor.iter().any(|a| a.part == arm) { Some(arm) } else { Some(ActorPart::Body) }
                }
                crate::character::PartKind::Shin(side) => Some(if *side < 0.0 {
                    ActorPart::LeftLeg
                } else {
                    ActorPart::RightLeg
                }),
                crate::character::PartKind::Foot(side) => Some(if *side < 0.0 {
                    ActorPart::LeftShin
                } else {
                    ActorPart::RightShin
                }),
                _ => None,
            };
            actor.iter().any(|a| Some(a.part) == exact || Some(a.part) == fallback)
        });
        // Exhaust starts hidden and is revealed only by the flight system.
        // Applying a style must not turn it on during ground movement.
        let hide=exhaust.is_some() || overridden || (parent.parent()==player && actor.iter().any(|a|a.part==ActorPart::Body));
        *visibility=if hide {Visibility::Hidden} else {Visibility::Inherited};
    }
    for a in &actor {
        let key = content_key(&(&profile, a));
        let handles = upload(world, key, &a.meshes, &profile);
        for (&parent, part) in &parts {
            if *part == a.part {
                let mut mesh_root = None;
                for light in &a.lights {
                    spawn_light(world, light, Some(parent), &profile);
                }
                for (mesh, material) in &handles {
                    let e = world
                        .spawn((
                            StyledActor,
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            if a.part == ActorPart::Weapon && profile.actor_weapon == Some(content::scenario::Weapon::Laser) {
                                Transform::from_rotation(Quat::from_rotation_x(-0.75))
                            } else {
                                Transform::default()
                            },
                        ))
                        .id();
                    mesh_root.get_or_insert(e);
                    if a.part == ActorPart::Weapon && !available_weapons.is_empty() {
                        world
                            .entity_mut(e)
                            .insert(WeaponVisual(profile.actor_weapon.unwrap_or(content::scenario::Weapon::Sword)));
                    }
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
                if a.part == ActorPart::Weapon {
                    if let (Some(root), Some(weapon), Some(muzzle)) = (
                        mesh_root,
                        profile.actor_weapon,
                        profile.actor.iter().find(|recipe| recipe.part == ActorPart::Weapon).and_then(|recipe| recipe.muzzle),
                    ) {
                        let marker = world.spawn((WeaponMuzzle(weapon), Transform::from_translation(Vec3::from_array(muzzle)))).id();
                        world.entity_mut(root).add_child(marker);
                    }
                }
            }
        }
    }
    if !available_weapons.is_empty() {
        for appearance in &profile.weapon_skins {
            if !available_weapons.contains(&appearance.weapon) {
                continue;
            }
            if let Some(skin) = skins.iter().find(|s| s.id == appearance.skin) {
                for part in &skin.parts {
                    let handles = upload(
                        world,
                        content_key(&(&profile, part)),
                        &part.meshes,
                        &profile,
                    );
                    for (&parent, kind) in &parts {
                        if *kind != ActorPart::Weapon {
                            continue;
                        }
                        for (mesh, material) in &handles {
                            let e = world
                                .spawn((
                                    StyledActor,
                                    WeaponVisual(appearance.weapon),
                                    Mesh3d(mesh.clone()),
                                    MeshMaterial3d(material.clone()),
                                    Transform::default(),
                                    Visibility::Hidden,
                                ))
                                .id();
                            world.entity_mut(parent).add_child(e);
                        }
                    }
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
    if !studio {
        world.spawn((
            StyleLabel,
            WorldScoped::default(),
            Text::new(format!(
                "STYLE {}  /  {:.0} VOXELS PER METER",
                profile.id.to_uppercase(),
                1.0 / profile.voxel_size
            )),
            TextFont {
                font_size: FontSize::Px(11.0),
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
    }
}

pub fn select_weapon(
    controls: Res<crate::gameplay::Controls>,
    mut visuals: Query<(&WeaponVisual, &mut Visibility)>,
) {
    if !controls.is_changed() {
        return;
    }
    for (weapon, mut visible) in &mut visuals {
        *visible = if weapon.0 == controls.weapon {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
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
                    shadow_maps_enabled: light.shadows,
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
                    shadow_maps_enabled: light.shadows,
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
        world.entity_mut(e).insert((Environment, WorldScoped::default()));
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
    // Nothing is shown before the first world starts.
    let Some(pack) = world.get_resource::<crate::scene_view::SceneView>().map(|view| view.pack.clone()) else {
        return serde_json::Value::Null;
    };
    let p = pack.scene.style.clone().unwrap_or_default();
    let mut info = serde_json::json!({"id":p.id,"voxel_size":p.voxel_size,"voxels_per_meter":1.0/p.voxel_size,"palette":p.materials.iter().map(|m| &m.name).collect::<Vec<_>>(),"environment_statistics":pack.environment.as_ref().map(|e|e.statistics),"baked":pack.environment.is_some(),"actor_voxel_size":p.actor_voxel_size,"actor_parts":pack.actor.iter().map(|a|a.part).collect::<Vec<_>>(),"weapon":p.weapon_name,"fixture_lights":pack.environment.as_ref().map(|e|&e.lights),"sun_shadows":p.sun_shadows,"exposure_ev100":p.exposure_ev100});
    let moving:Vec<_>=world.query_filtered::<Entity,With<StyledActor>>().iter(world).filter_map(|e| {
        let (kind,intensity,range,shadows)=if let Some(l)=world.get::<PointLight>(e) {("point",l.intensity,l.range,l.shadow_maps_enabled)} else if let Some(l)=world.get::<SpotLight>(e) {("spot",l.intensity,l.range,l.shadow_maps_enabled)} else {return None;};
        Some(serde_json::json!({"kind":kind,"position":world_position(world,e).to_array(),"intensity":intensity,"range":range,"shadows":shadows}))
    }).collect();
    info["actor_lights"] = serde_json::json!(moving);
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::SceneLibrary;
    #[test]
    fn authored_joint_pivots_change_proportions_and_restore_on_scene_switch() {
        let options = crate::control::Options {
            simulation: true,
            ..default()
        };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let scenes = SceneLibrary::new();
        let moonlit = scenes.index("moonlit-glade").unwrap();
        let mut app = crate::test_app(options, scenes, hub, crate::flow::Launch::single_player(0));
        crate::start_world(&mut app);
        crate::switch_world(&mut app, moonlit);
        let shoulder_x = |world: &mut World| {
            world
                .query::<&crate::character::Part>()
                .iter(world)
                .find(|part| matches!(part.kind, crate::character::PartKind::Arm(1.0)))
                .unwrap()
                .rest
                .translation
                .x
        };
        assert!((shoulder_x(app.world_mut()) - 0.28).abs() < 0.001);
        crate::switch_world(&mut app, 0);
        assert!((shoulder_x(app.world_mut()) - 0.43).abs() < 0.001);
    }

    #[test]
    fn scene_styles_restore_materials_and_lights_and_reuse_uploaded_assets() {
        let options = crate::control::Options {
            simulation: true,
            ..default()
        };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, SceneLibrary::new(), hub, crate::flow::Launch::single_player(0));
        crate::start_world(&mut app);
        let originals: Vec<_> = app
            .world()
            .resource::<MaterialBindings>()
            .0
            .iter()
            .map(|(h, _, m)| (h.clone(), m.base_color, m.emissive))
            .collect();
        let mut counts = None;
        for index in [5, 0, 5, 0, 5] {
            crate::switch_world(&mut app, index);
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
                    assert!(light.shadow_maps_enabled);
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
                    assert!(!light.shadow_maps_enabled);
                }
                for (_, light) in world.query::<(&AccentLight, &PointLight)>().iter(world) {
                    assert_eq!(light.intensity, 0.0);
                }
                let before = info["actor_lights"][0]["position"][0].as_f64().unwrap();
                let player = world
                    .query_filtered::<Entity, With<crate::character::Player>>()
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
                    assert!(light.shadow_maps_enabled);
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

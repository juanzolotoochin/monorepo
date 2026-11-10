//! The sun and the moon (Sacred Tree day and night, N5 and N6): in a scene
//! with a day cycle of light (`Scene::day_cycle`), the scene's one
//! `DirectionalLight` follows the sky's arc on the sim's clock, by day the
//! sun and by night a dim moon, and its color, the ambient light, the clear
//! color and the fog blend between the cycle's keys. The cycle wins over the
//! style's fixed light. Other scenes keep the style's light and the light's
//! fixed place, as before.
//!
//! By night the scene's own lights come up (N7): its glow (the Sacred
//! Tree's) and its lanterns, lit by `day_cycle::night_glow` at the same
//! hour. In a scene with a day cycle the built-in deck's accent lights are
//! off: the night's lights replace them.
use bevy::{
    light::{CascadeShadowConfig, CascadeShadowConfigBuilder},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
};
use service::WorldScoped;
use voxel_assets::{day_cycle, ScenePack};

/// Where the light stands in a scene without a day cycle (the built-in
/// deck's key light).
pub fn fixed_transform() -> Transform {
    Transform::from_xyz(-8.0, 12.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// The shadow cascades in a scene with a day cycle: the town's camera
/// looks from about 35 m away with an orthographic view, so nothing it
/// shows is nearer than 10 m or further than about 60 m (zoomed out, the
/// view rotated, the ground below the hill). Three cascades over 10 to
/// 80 m spend the shadow map there (the default's first cascade, 0.1 to
/// 10 m, is empty air, and its last runs to 150 m). Long dawn and dusk
/// shadows come from casters on the same light ray as the ground they
/// darken, so they stay inside a cascade's light-space box.
pub fn cycle_cascades() -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: 3,
        minimum_distance: 10.0,
        first_cascade_far_bound: 32.0,
        maximum_distance: 80.0,
        overlap_proportion: 0.2,
    }
    .build()
}

/// The light a day cycle gives at `time` (in ticks, fractional): the
/// blended key and where the light stands. None without a cycle or a clock.
pub fn light_at(pack: &ScenePack, time: f64) -> Option<day_cycle::Light> {
    let cycle = pack.scene.day_cycle.as_ref()?;
    let clock = sim::settlement::Clock::of(pack.scene.scenario.as_ref()?)?;
    Some(cycle.at(day_cycle::hour(&clock, time)))
}

/// How lit a scene's night lights are at `time` (in ticks, fractional): by
/// the day cycle's hour, or always full in a scene without one.
pub fn glow_at(pack: &ScenePack, time: f64) -> f32 {
    let clock = pack.scene.scenario.as_ref().and_then(sim::settlement::Clock::of);
    match (&pack.scene.day_cycle, clock) {
        (Some(_), Some(clock)) => day_cycle::night_glow(day_cycle::hour(&clock, time)),
        _ => 1.0,
    }
}

/// A point light lit by night: its intensity at full glow, in lumens.
#[derive(Component)]
pub struct NightLight {
    pub full: f32,
}

/// A lantern's glowing head: its material, whose emissive follows the glow.
#[derive(Component)]
pub struct LanternHead;

/// A lantern's light, in lumens at full glow, its range and its head's
/// emissive (linear, HDR) at full glow.
pub const LANTERN_LUMENS: f32 = 250000.0;
pub const LANTERN_RANGE: f32 = 10.0;
const LANTERN_EMISSIVE: [f32; 3] = [10.0, 5.0, 1.5];
/// The head's light, above the lantern's foot.
const LANTERN_HEIGHT: f32 = 1.75;

/// Spawns the scene's night lights (its glow and lanterns) for the world,
/// unlit until `follow` or `dress` sets their glow. Shadows off.
pub fn spawn_night_lights(world: &mut World, pack: &ScenePack) {
    let scene = &pack.scene;
    if let Some(glow) = &scene.glow {
        world.spawn((
            WorldScoped::default(),
            NightLight { full: glow.intensity },
            PointLight {
                color: color(glow.color),
                intensity: 0.0,
                range: glow.range,
                radius: 0.6,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(Vec3::from_array(glow.position)),
        ));
    }
    if scene.lanterns.is_empty() {
        return;
    }
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
    let (wood, head) = {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let wood = materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.13, 0.09),
            perceptual_roughness: 0.92,
            ..default()
        });
        let head = materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.72, 0.42),
            perceptual_roughness: 0.6,
            ..default()
        });
        (wood, head)
    };
    for lantern in &scene.lanterns {
        let foot = Vec3::from_array(lantern.position);
        let blocks = [
            (Vec3::new(0.0, 0.06, 0.0), Vec3::new(0.32, 0.12, 0.32), &wood),
            (Vec3::new(0.0, 0.8, 0.0), Vec3::new(0.13, 1.6, 0.13), &wood),
            (Vec3::new(0.0, LANTERN_HEIGHT, 0.0), Vec3::new(0.26, 0.3, 0.26), &head),
            (Vec3::new(0.0, LANTERN_HEIGHT + 0.19, 0.0), Vec3::new(0.36, 0.08, 0.36), &wood),
        ];
        for (offset, size, material) in blocks {
            let e = world
                .spawn((
                    WorldScoped::default(),
                    crate::cutaway::Outdoor,
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(foot + offset).with_scale(size),
                ))
                .id();
            if material == &head {
                world.entity_mut(e).insert((LanternHead, bevy::light::NotShadowCaster));
            }
        }
        world.spawn((
            WorldScoped::default(),
            NightLight { full: LANTERN_LUMENS },
            PointLight {
                color: Color::srgb(1.0, 0.66, 0.34),
                intensity: 0.0,
                range: LANTERN_RANGE,
                radius: 0.15,
                shadow_maps_enabled: false,
                ..default()
            },
            // Just under the head, so the post doesn't hide it.
            Transform::from_translation(foot + Vec3::new(0.0, LANTERN_HEIGHT - 0.1, 0.0)),
        ));
    }
}

/// Lights the night's lights at `glow` (0 to 1): their point lights and
/// the lanterns' heads.
fn light_up(world: &mut World, glow: f32) {
    for (light, mut point) in world.query::<(&NightLight, &mut PointLight)>().iter_mut(world) {
        point.intensity = light.full * glow;
    }
    // In the town's fog a head is drawn by a fogged copy that follows its
    // material (`fog_dressing::Fogged`).
    let heads: Vec<Handle<StandardMaterial>> = world
        .query_filtered::<AnyOf<(&MeshMaterial3d<StandardMaterial>, &crate::fog_dressing::Fogged)>, With<LanternHead>>()
        .iter(world)
        .filter_map(|(m, f)| m.map(|m| m.0.clone()).or_else(|| f.map(|f| f.0.clone())))
        .collect();
    let emissive = LinearRgba::rgb(LANTERN_EMISSIVE[0] * glow, LANTERN_EMISSIVE[1] * glow, LANTERN_EMISSIVE[2] * glow);
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    let mut done = Vec::new();
    for handle in heads {
        if done.contains(&handle.id()) {
            continue;
        }
        done.push(handle.id());
        // Only when it changes, so a still hour doesn't re-upload it.
        if materials.get(&handle).is_some_and(|m| m.emissive != emissive) {
            if let Some(mut m) = materials.get_mut(&handle) {
                m.emissive = emissive;
            }
        }
    }
}

/// The sim's time now, in ticks, eased between the last two.
fn now(world: &World) -> f64 {
    world
        .get_resource::<crate::lockstep::WorldMirror>()
        .map_or(0.0, |m| m.tick as f64 + f64::from(m.alpha))
}

fn color(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

/// The light's rotation shining from `toward` (unit, towards the sky).
fn shining_from(toward: [f32; 3]) -> Transform {
    Transform::from_translation(Vec3::from_array(toward) * 30.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Writes the light a world starts with, after the style's (from
/// `style::apply`): the cycle's at the sim's time, its cascades, or, with
/// no cycle, the fixed light's place and the default cascades.
pub fn dress(world: &mut World, pack: &ScenePack) {
    let light = light_at(pack, now(world));
    let lights: Vec<Entity> = world.query_filtered::<Entity, With<DirectionalLight>>().iter(world).collect();
    for e in lights {
        let (transform, cascades) = match &light {
            Some(l) => (shining_from(l.toward), cycle_cascades()),
            None => (fixed_transform(), CascadeShadowConfig::default()),
        };
        world.entity_mut(e).insert((transform, cascades));
    }
    if let Some(light) = light {
        write(world, &light);
        // The night's lights replace the deck's accent lights here.
        for mut accent in world
            .query_filtered::<&mut PointLight, With<crate::style::AccentLight>>()
            .iter_mut(world)
        {
            accent.intensity = 0.0;
        }
    }
    light_up(world, glow_at(pack, now(world)));
}

/// Writes one sampled light over the world's.
fn write(world: &mut World, light: &day_cycle::Light) {
    let key = light.key;
    world.resource_mut::<ClearColor>().0 = color(key.sky);
    {
        let mut ambient = world.resource_mut::<GlobalAmbientLight>();
        ambient.color = color(key.ambient);
        ambient.brightness = key.ambient_brightness;
    }
    for (mut sun, mut transform) in world.query::<(&mut DirectionalLight, &mut Transform)>().iter_mut(world) {
        sun.color = color(key.sunlight);
        sun.illuminance = key.illuminance;
        sun.shadow_maps_enabled = true;
        transform.rotation = shining_from(light.toward).rotation;
    }
    for mut fog in world.query_filtered::<&mut DistanceFog, With<Camera3d>>().iter_mut(world) {
        fog.color = color(key.sky);
        fog.falloff = FogFalloff::Exponential { density: key.fog_density };
    }
}

/// Every frame: the cycle's light at the sim's time, eased between ticks.
pub fn follow(world: &mut World) {
    let Some(pack) = world.get_resource::<crate::scene_view::SceneView>().map(|v| v.pack.clone()) else {
        return;
    };
    let time = now(world);
    if let Some(light) = light_at(&pack, time) {
        write(world, &light);
        light_up(world, glow_at(&pack, time));
    }
}

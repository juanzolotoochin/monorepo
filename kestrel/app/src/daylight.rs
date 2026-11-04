//! The sun and the moon (Sacred Tree day and night, N5 and N6): in a scene
//! with a day cycle of light (`Scene::day_cycle`), the scene's one
//! `DirectionalLight` follows the sky's arc on the sim's clock, by day the
//! sun and by night a dim moon, and its color, the ambient light, the clear
//! color and the fog blend between the cycle's keys. The cycle wins over the
//! style's fixed light. Other scenes keep the style's light and the light's
//! fixed place, as before.
use bevy::{
    light::{CascadeShadowConfig, CascadeShadowConfigBuilder},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
};
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
    }
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
    if let Some(light) = light_at(&pack, now(world)) {
        write(world, &light);
    }
}

//! The cutaway (VERTICALITY.md V7): in a volume scene the ground round the
//! player's line of sight is not drawn, so the player stays visible
//! underground, and the inside of solid ground seen through the hole is
//! drawn flat dark (a section).
//!
//! The shape: a cylinder round the ray from the middle of the body toward
//! the camera (the orthographic camera's backward axis: it looks down from
//! +x +z), capped by a half sphere round the body. Inside it the ground is
//! cut over the head plane (`BODY_HEIGHT` plus `MARGIN` over the feet), so
//! ceilings and the hill between the camera and the player go but the far
//! walls stand. On the camera's side of the body (the ray's half) a
//! narrower cylinder (`LOW_RADIUS`) is also cut down to `LOW` over the
//! feet, so a tunnel's near wall or a trench's side hiding the body goes
//! down to the shins. Floors at the feet are never cut (the tunnel floor,
//! a bridge underfoot). The two planes meet in a plane through the body
//! square to the line of sight, seen edge on. Over the head the radius is
//! `RADIUS` while the player is under a ceiling, else `LOW_RADIUS` (see
//! `follow`), so in the open terraces beside the player that hide nothing
//! keep their shape. The last `EDGE` meters of the radius are dithered (an
//! ordered 4 x 4 discard), so the ground stays opaque. Back faces are the
//! section colour. Shadows use the uncut ground (see
//! `shaders/cutaway_prepass.wesl`). Other scenes keep `digging`'s plain
//! ground material.
use crate::character::Player;
use crate::effects::CameraRig;
use bevy::{
    asset::uuid::Uuid,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, Face, ShaderType},
    shader::ShaderRef,
};
use std::marker::PhantomData;

const SHADER: Handle<Shader> = Handle::Uuid(Uuid::from_u128(0x3c2d8f61_7a0e_4b5c_9e1d_5f4a8b2c6d01), PhantomData);
const PREPASS_SHADER: Handle<Shader> = Handle::Uuid(Uuid::from_u128(0x3c2d8f61_7a0e_4b5c_9e1d_5f4a8b2c6d02), PhantomData);

/// Meters over the head (feet plus `BODY_HEIGHT`) the cut starts behind
/// the body.
pub const MARGIN: f32 = 0.25;
/// Meters over the feet the cut starts on the camera's side of the body:
/// a layer, so a one-layer step in front stays.
pub const LOW: f32 = 0.5;
/// The hole's radius round the line of sight over the head, in meters,
/// under a ceiling (see `follow`).
pub const RADIUS: f32 = 4.5;
/// The radius under the head plane (on the camera's side): enough to show
/// the body (fully cut within 1.25 m of the line through its middle), so
/// a terrace or wall that doesn't hide the player keeps its shape.
pub const LOW_RADIUS: f32 = 2.25;
/// The dithered band inside the radius, in meters.
pub const EDGE: f32 = 1.0;
/// The section colour, sRGB: the inside of solid ground.
const SECTION: [f32; 3] = [0.11, 0.085, 0.075];

pub type CutawayMaterial = ExtendedMaterial<StandardMaterial, CutawayExt>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CutawayExt {
    #[uniform(100)]
    pub cutaway: CutawayUniform,
}

/// The shaders' `Cutaway` struct.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct CutawayUniform {
    /// xyz: the middle of the body; w: the radius.
    pub center: Vec4,
    /// xyz: unit vector toward the camera; w: the dithered edge.
    pub toward: Vec4,
    /// x: the low plane's height (camera side); y: the head plane's; z:
    /// the radius under the head plane.
    pub planes: Vec4,
    /// rgb: the section colour (linear); w: 1 when the cut is on.
    pub section: Vec4,
}

impl MaterialExtension for CutawayExt {
    fn fragment_shader() -> ShaderRef { SHADER.clone().into() }
    fn prepass_fragment_shader() -> ShaderRef { PREPASS_SHADER.clone().into() }
}

pub fn register_shader(app: &mut App) {
    let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
    shaders
        .insert(SHADER.id(), Shader::from_wesl(include_str!("../shaders/cutaway.wesl"), "cutaway.wesl"))
        .expect("static cutaway shader handle");
    shaders
        .insert(PREPASS_SHADER.id(), Shader::from_wesl(include_str!("../shaders/cutaway_prepass.wesl"), "cutaway_prepass.wesl"))
        .expect("static cutaway prepass shader handle");
}

/// The volume ground's material: `base` drawn from both sides (back faces
/// are the section), with the cut off until `follow` places it.
pub fn material(base: StandardMaterial) -> CutawayMaterial {
    let section = Color::srgb(SECTION[0], SECTION[1], SECTION[2]).to_linear();
    ExtendedMaterial {
        base: StandardMaterial { cull_mode: None::<Face>, double_sided: true, ..base },
        extension: CutawayExt {
            cutaway: CutawayUniform {
                center: Vec4::new(0.0, 0.0, 0.0, RADIUS),
                toward: Vec4::new(0.0, 1.0, 0.0, EDGE),
                planes: Vec4::ZERO,
                section: Vec4::new(section.red, section.green, section.blue, 0.0),
            },
        },
    }
}

/// The cut for feet at `feet` and a camera looking along `-toward`.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    pub center: Vec3,
    pub toward: Vec3,
    pub low: f32,
    pub head: f32,
}

impl Cut {
    pub fn new(feet: Vec3, toward: Vec3) -> Self {
        let body = sim::volume::BODY_HEIGHT.to_f32();
        Cut { center: feet + Vec3::Y * body * 0.5, toward: toward.normalize(), low: feet.y + LOW, head: feet.y + body + MARGIN }
    }

    /// How much of the ground at `p` is cut (the shaders' `cut`): 0 drawn,
    /// 1 gone, between in the dithered edge.
    #[cfg_attr(not(test), allow(dead_code))] // Only the shaders cut; this mirrors them for tests.
    pub fn amount(&self, p: Vec3, radius: f32, low_radius: f32, edge: f32) -> f32 {
        let v = p - self.center;
        let along = v.dot(self.toward);
        let plane = if along > 0.0 { self.low } else { self.head };
        if p.y <= plane {
            return 0.0;
        }
        let radius = if p.y > self.head { radius } else { low_radius };
        let off = (v - self.toward * along.max(0.0)).length();
        ((radius - off) / edge.max(0.001)).clamp(0.0, 1.0)
    }
}

/// The volume ground's material, when the scene has a volume.
#[derive(Resource)]
pub struct CutawayGround(pub Handle<CutawayMaterial>);

/// Places the cut on the drawn player each frame, along the camera's line
/// of sight. Over the head the hole opens to `RADIUS` only while the player
/// is under a ceiling (`Terrain::ceiling_at`), easing in and out; in the
/// open (a trench, beside a hill, on a bridge) it stays `LOW_RADIUS`, which
/// still clears whatever hides the body.
pub fn follow(
    ground: Option<Res<CutawayGround>>,
    service: Option<Res<crate::lockstep::SessionService>>,
    time: Res<Time>,
    player: Option<Single<&GlobalTransform, With<Player>>>,
    camera: Option<Single<&GlobalTransform, With<CameraRig>>>,
    mut materials: ResMut<Assets<CutawayMaterial>>,
    mut radius: Local<Option<f32>>,
) {
    let Some(ground) = ground else { return };
    let (Some(player), Some(camera)) = (player, camera) else { return };
    let feet = player.translation();
    let covered = service.as_ref().and_then(|service| {
        let terrain = service.world()?.terrain();
        let z = fx::Fx::from_ratio(((feet.y + 0.05) * 1000.0).round() as i64, 1000);
        terrain.ceiling_at(service.to_sim(feet), z, fx::Fx::from_ratio(3, 10))
    });
    let target = if covered.is_some() { RADIUS } else { LOW_RADIUS };
    let blend = 1.0 - (-6.0 * time.delta_secs()).exp();
    let radius = radius.get_or_insert(target);
    *radius += (target - *radius) * blend;
    let cut = Cut::new(feet, camera.back().as_vec3());
    let Some(mut material) = materials.get_mut(&ground.0) else { return };
    let cutaway = &mut material.extension.cutaway;
    cutaway.center = cut.center.extend(*radius);
    cutaway.toward = cut.toward.extend(EDGE);
    cutaway.planes = Vec4::new(cut.low, cut.head, LOW_RADIUS.min(*radius), 0.0);
    cutaway.section.w = 1.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toward() -> Vec3 { Vec3::ONE.normalize() }
    /// Square to the line of sight, level.
    fn side() -> Vec3 { Vec3::new(1.0, 0.0, -1.0).normalize() }
    fn amount(cut: &Cut, p: Vec3) -> f32 { cut.amount(p, RADIUS, LOW_RADIUS, EDGE) }

    #[test]
    fn ground_over_the_head_or_toward_the_camera_is_cut() {
        let feet = Vec3::new(12.0, 0.0, 0.0);
        let cut = Cut::new(feet, toward());
        // The tunnel roof right over the player, and behind them.
        assert_eq!(amount(&cut, feet + Vec3::Y * 3.0), 1.0);
        assert_eq!(amount(&cut, feet + Vec3::new(-1.0, 3.0, -1.0)), 1.0);
        // The mountain's top between the player and the camera.
        assert_eq!(amount(&cut, cut.center + toward() * 9.0), 1.0);
        assert_eq!(amount(&cut, cut.center + toward() * 9.0 + side()), 1.0);
        // The near wall of a tunnel, or a trench's side, down to the shins.
        assert_eq!(amount(&cut, feet + Vec3::new(1.5, 1.0, 0.0)), 1.0);
        assert_eq!(amount(&cut, feet + Vec3::new(0.0, 0.6, 1.0)), 1.0);
    }

    #[test]
    fn floors_and_far_walls_are_not_cut() {
        let feet = Vec3::new(-11.0, 5.0, 13.0);
        let cut = Cut::new(feet, toward());
        // A bridge under the feet, near and far.
        assert_eq!(amount(&cut, feet), 0.0);
        assert_eq!(amount(&cut, feet + Vec3::new(1.5, 0.0, 1.5)), 0.0);
        assert_eq!(amount(&cut, feet + Vec3::new(-1.5, 0.0, -1.5)), 0.0);
        // A step a layer up toward the camera.
        assert_eq!(amount(&cut, feet + Vec3::new(1.0, LOW, 0.0)), 0.0);
        // The wall behind the player, up to the head.
        assert_eq!(amount(&cut, feet + Vec3::new(-1.5, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn under_the_head_only_a_narrow_hole_is_cut() {
        let cut = Cut::new(Vec3::ZERO, toward());
        // A terrace 1.5 m up beside the line of sight, 2.4 m off it: kept;
        // the same point over the head plane would be cut.
        let p = cut.center + toward() * 1.0 + side() * 2.4;
        assert!(p.y < cut.head);
        assert_eq!(amount(&cut, p), 0.0);
        assert_eq!(amount(&cut, Vec3::new(p.x, cut.head + 0.5, p.z)), 1.0);
    }

    #[test]
    fn ground_outside_the_radius_is_not_cut() {
        let cut = Cut::new(Vec3::ZERO, toward());
        assert_eq!(amount(&cut, cut.center + toward() * 5.0 + side() * (RADIUS + 0.1)), 0.0);
        // Behind the body only the half sphere round it is cut.
        assert_eq!(amount(&cut, Vec3::new(-4.0, 3.0, -4.0)), 0.0);
        assert_eq!(amount(&cut, Vec3::new(-2.0, 3.0, -2.0)), 1.0);
    }

    #[test]
    fn the_edge_fades_across_its_band() {
        let cut = Cut::new(Vec3::ZERO, toward());
        let at = |off: f32| amount(&cut, cut.center + toward() * 4.0 + side() * off);
        assert_eq!(at(RADIUS - EDGE - 0.01), 1.0);
        assert!((at(RADIUS - EDGE / 2.0) - 0.5).abs() < 1e-4);
        assert!(at(RADIUS - EDGE * 0.25) < at(RADIUS - EDGE * 0.75));
        assert_eq!(at(RADIUS + 0.01), 0.0);
    }
}

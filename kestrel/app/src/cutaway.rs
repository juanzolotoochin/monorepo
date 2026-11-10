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
//! section colour. Under a ceiling a fill light over the head (`FillLight`)
//! lifts the shadowed floors. Shadows use the uncut ground (see
//! `shaders/cutaway_prepass.wesl`). Other scenes keep `digging`'s plain
//! ground material.
//!
//! Line of sight (the default, `ViewMode::Sight`; F7 switches to the
//! cylinder above and back): you only see the inside when you're inside.
//! In the open nothing is cut. Under a ceiling (`Terrain::ceiling_at`) an
//! inside factor eases to 1 over `sight::FADE`; with it, ground over the
//! ceiling's height (less `HIDE_MARGIN`) is discarded over the whole view
//! (dithered by the factor, so the cut fades in), walls under it stand
//! (in the `NEAR_TILES` tiles from the player toward the camera only
//! `NEAR_WALL` high, so a near wall doesn't hide the body), and
//! `sight`'s per-tile visibility from the head (an R8 texture over the
//! window) lights the ground: in sight it falls off with distance from the
//! player (`NEAR`, `FAR`, `FAR_LIGHT`) with an inside light lifting the
//! mountain's shadow (`INSIDE_LIGHT`), out of sight it is dimmed to `DIM`
//! (the section to `SECTION_DIM`) and partly desaturated (`GREY`).
//! Everything blends by the factor, so the outside looks as it always did.
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
/// The section colour, sRGB: the inside of solid ground. Lighter than
/// the shadowed floors round it (V.8), so the cut reads as a cut and the
/// body in it stands out.
const SECTION: [f32; 3] = [0.3, 0.25, 0.21];
/// The fill light that follows the player under a ceiling (V.8): the
/// mountain's shadow leaves tunnel and cave floors nearly black. A point
/// light over the head, no shadow map of its own (the sun's shadows stay
/// as they are), warm, its brightness easing in under a ceiling and out
/// in the open.
pub const FILL_LUMENS: f32 = 200000.0;
/// How far the fill light reaches, in meters.
pub const FILL_RANGE: f32 = 8.0;
/// How far over the feet it hangs: under a 2.5 m tunnel's roof.
pub const FILL_HEIGHT: f32 = 1.8;

/// Line of sight: how far under the ceiling over the head the cut starts,
/// meters, so the ceiling's own faces go and the walls under it stand.
pub const HIDE_MARGIN: f32 = 0.05;
/// Line of sight: on the camera's side of the player, inside, ground is
/// also cut this far over the feet, so a near wall doesn't hide the body
/// (behind the player walls stand to the ceiling).
pub const NEAR_WALL: f32 = 1.0;
/// ...in the tiles from the player's to this many tiles toward the camera
/// along x and z (a blocky box on the camera's side).
pub const NEAR_TILES: f32 = 4.0;
/// In sight, full light within `NEAR` meters of the player...
pub const NEAR: f32 = 6.0;
/// ...easing down to `FAR_LIGHT` by `FAR` meters.
pub const FAR: f32 = 20.0;
pub const FAR_LIGHT: f32 = 0.4;
/// Out of sight, the brightness kept...
pub const DIM: f32 = 0.25;
/// The section (cut tops) out of sight keeps less: darker than dimmed
/// ground, so the floors in sight read against it.
pub const SECTION_DIM: f32 = 0.12;
/// In sight the mountain's shadow still lies on the floors (shadows use
/// the uncut ground): this share of the albedo is added as an inside
/// light, falling off with distance as the rest.
pub const INSIDE_LIGHT: f32 = 0.35;
/// ...and how far toward grey it goes (0 none, 1 grey).
pub const GREY: f32 = 0.35;
/// How far round the body the inside test looks for a ceiling, meters
/// (the body's radius is 0.3).
pub const INSIDE_REACH: f32 = 0.7;
/// The key that switches between line of sight and the V.6 cylinder.
pub const TOGGLE: KeyCode = KeyCode::F7;
/// How long the view mode's label shows after a switch, seconds.
const LABEL_SECONDS: f32 = 2.5;

/// How a volume scene's ground is cut round the player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// You only see the inside when you're inside (the default).
    #[default]
    Sight,
    /// V.6's cylinder along the line of sight.
    Cutaway,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            ViewMode::Sight => "VIEW: LINE OF SIGHT (F7)",
            ViewMode::Cutaway => "VIEW: CUTAWAY (F7)",
        }
    }
}

/// What the line of sight view shows this frame, for bodies to fade by.
#[derive(Resource, Default)]
pub struct SightView {
    pub mode: ViewMode,
    /// The inside factor as drawn (0 in the open, 1 inside).
    pub inside: f32,
    /// Its eased value before smoothing.
    factor: f32,
    /// The height ground is hidden over while inside.
    pub hide: f32,
    /// The visibility round the player's level, in world tiles.
    pub sight: Option<crate::sight::Sight>,
    /// The terrain center in tiles, to turn arena meters into tiles.
    pub center: (i32, i32),
}

impl SightView {
    /// A new world: nothing seen yet, outside (the mode stays).
    pub fn reset(&mut self) {
        *self = SightView { mode: self.mode, ..default() };
    }

    /// How visible a body with its feet at arena point `feet` is: 1 in
    /// the open or in the cutaway mode; inside, its tile's visibility (0
    /// over the hidden height).
    pub fn body_alpha(&self, feet: Vec3) -> f32 {
        if self.mode != ViewMode::Sight || self.inside <= 0.0 {
            return 1.0;
        }
        let tile = ((feet.x + 0.5).floor() as i32 + self.center.0, (feet.z + 0.5).floor() as i32 + self.center.1);
        let seen = if feet.y > self.hide { 0.0 } else { self.sight.as_ref().map_or(1.0, |s| s.at(tile)) };
        1.0 - self.inside * (1.0 - seen)
    }
}

/// The view mode's label, shown a moment after a switch.
#[derive(Component)]
pub struct ModeLabel(f32);

/// The fill light (see [`FILL_LUMENS`]).
#[derive(Component)]
pub struct FillLight;

pub type CutawayMaterial = ExtendedMaterial<StandardMaterial, CutawayExt>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CutawayExt {
    #[uniform(100)]
    pub cutaway: CutawayUniform,
    /// Line of sight: per-tile visibility (R8) over the window.
    #[texture(101)]
    #[sampler(102)]
    pub sight: Handle<Image>,
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
    /// Line of sight. x: the inside factor; y: the height ground is hidden
    /// over; z: 1 in the line of sight mode; w: `GREY`.
    pub sight: Vec4,
    /// xy: the visibility texture's lowest corner (arena x, z); z: its
    /// side in meters; w: `NEAR_TILES`.
    pub window: Vec4,
    /// xyz: the player's feet (arena); w: `DIM`.
    pub eye: Vec4,
    /// x: `NEAR`, y: `FAR`, z: `FAR_LIGHT`, w: `NEAR_WALL`.
    pub light: Vec4,
    /// x: `INSIDE_LIGHT`, y: `SECTION_DIM`.
    pub inside: Vec4,
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

/// An empty visibility texture for [`material`] (`follow` fills it).
pub fn sight_image() -> Image {
    let size = crate::sight::WINDOW as u32;
    let mut image = Image::new_fill(
        bevy::render::render_resource::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        &[0],
        bevy::render::render_resource::TextureFormat::R8Unorm,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

/// The volume ground's material: `base` drawn from both sides (back faces
/// are the section), with the cut off until `follow` places it.
pub fn material(base: StandardMaterial, sight: Handle<Image>) -> CutawayMaterial {
    let section = Color::srgb(SECTION[0], SECTION[1], SECTION[2]).to_linear();
    ExtendedMaterial {
        base: StandardMaterial { cull_mode: None::<Face>, double_sided: true, ..base },
        extension: CutawayExt {
            cutaway: CutawayUniform {
                center: Vec4::new(0.0, 0.0, 0.0, RADIUS),
                toward: Vec4::new(0.0, 1.0, 0.0, EDGE),
                planes: Vec4::ZERO,
                section: Vec4::new(section.red, section.green, section.blue, 0.0),
                sight: Vec4::new(0.0, 0.0, 0.0, GREY),
                window: Vec4::new(0.0, 0.0, crate::sight::WINDOW as f32, NEAR_TILES),
                eye: Vec4::new(0.0, 0.0, 0.0, DIM),
                light: Vec4::new(NEAR, FAR, FAR_LIGHT, NEAR_WALL),
                inside: Vec4::new(INSIDE_LIGHT, SECTION_DIM, 0.0, 0.0),
            },
            sight,
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

/// Places the cut on the drawn player each frame. In the line of sight
/// mode: the inside factor, the hidden height and the visibility texture
/// (recomputed when the player's tile, floor or the terrain changes). In
/// the cutaway mode the V.6 cylinder along the camera's line of sight:
/// over the head the hole opens to `RADIUS` only while the player is under
/// a ceiling (`Terrain::ceiling_at`), easing in and out; in the open (a
/// trench, beside a hill, on a bridge) it stays `LOW_RADIUS`, which still
/// clears whatever hides the body. `TOGGLE` (F7) switches.
#[allow(clippy::too_many_arguments)]
pub fn follow(
    mut commands: Commands,
    mut fill: Query<(&mut PointLight, &mut Transform), With<FillLight>>,
    mut labels: Query<(Entity, &mut ModeLabel)>,
    mut glow: Local<f32>,
    ground: Option<Res<CutawayGround>>,
    service: Option<Res<crate::lockstep::SessionService>>,
    input: Option<Res<debug_ui::GameInput>>,
    time: Res<Time>,
    player: Option<Single<&GlobalTransform, With<Player>>>,
    camera: Option<Single<&GlobalTransform, With<CameraRig>>>,
    mut materials: ResMut<Assets<CutawayMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut view: ResMut<SightView>,
    mut radius: Local<Option<f32>>,
) {
    let Some(ground) = ground else { return };
    for (entity, mut label) in &mut labels {
        label.0 -= time.delta_secs();
        if label.0 <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    if input.as_ref().is_some_and(|input| input.keys().just_pressed(TOGGLE)) {
        view.mode = match view.mode {
            ViewMode::Sight => ViewMode::Cutaway,
            ViewMode::Cutaway => ViewMode::Sight,
        };
        for (entity, _) in &labels {
            commands.entity(entity).despawn();
        }
        commands.spawn((
            ModeLabel(LABEL_SECONDS),
            service::WorldScoped::default(),
            Text::new(view.mode.label()),
            TextFont { font_size: FontSize::Px(18.0), ..default() },
            TextColor(Color::srgb(0.9, 0.83, 0.63)),
            Node { position_type: PositionType::Absolute, left: px(24), bottom: px(24), ..default() },
        ));
    }
    let (Some(player), Some(camera)) = (player, camera) else { return };
    let feet = player.translation();
    let world = service.as_ref().and_then(|service| service.world());
    let at = service.as_ref().map(|service| service.to_sim(feet));
    let z = fx::Fx::from_ratio(((feet.y + 0.05) * 1000.0).round() as i64, 1000);
    let covered = world.zip(at).and_then(|(world, at)| world.terrain().ceiling_at(at, z, fx::Fx::from_ratio(3, 10)));
    // Line of sight starts a little before the body is under the roof
    // (`INSIDE_REACH`), so the roof is going as the body goes under it.
    let reach = fx::Fx::from_ratio((INSIDE_REACH * 100.0) as i64, 100);
    let under = world.zip(at).and_then(|(world, at)| world.terrain().ceiling_at(at, z, reach));
    let blend = 1.0 - (-6.0 * time.delta_secs()).exp();
    *glow += (if covered.is_some() { 1.0 } else { 0.0 } - *glow) * blend;
    let light_at = Transform::from_translation(feet + Vec3::Y * FILL_HEIGHT);
    match fill.single_mut() {
        Ok((mut light, mut transform)) => {
            light.intensity = FILL_LUMENS * *glow;
            *transform = light_at;
        }
        Err(_) => {
            commands.spawn((
                service::WorldScoped::default(),
                FillLight,
                PointLight {
                    color: Color::srgb(1.0, 0.88, 0.72),
                    intensity: FILL_LUMENS * *glow,
                    range: FILL_RANGE,
                    radius: 0.2,
                    shadow_maps_enabled: false,
                    ..default()
                },
                light_at,
            ));
        }
    }
    // Line of sight: the inside factor, the hidden height, the visibility.
    let sight_mode = view.mode == ViewMode::Sight;
    view.factor = crate::sight::ease(view.factor, under.is_some() && sight_mode, time.delta_secs());
    view.inside = crate::sight::shown(view.factor);
    if let Some(ceiling) = under {
        view.hide = ceiling.to_f32() - HIDE_MARGIN;
    }
    if let (Some(world), Some(at)) = (world, at) {
        let terrain = world.terrain();
        let center = sim::terrain::Terrain::pit_tile(terrain.center);
        view.center = center;
        let floor = terrain.volume.as_ref().and_then(|volume| volume.floor_at(at, z)).map(|(floor, _)| floor);
        if let (Some(volume), Some(floor), true) = (&terrain.volume, floor, sight_mode) {
            let key = crate::sight::Key { tile: sim::terrain::Terrain::pit_tile(at), floor, revision: terrain.revision };
            if crate::sight::Sight::stale(view.sight.as_ref(), key) {
                let sight = crate::sight::compute(key, crate::sight::WINDOW, |t| crate::sight::blocks(volume, t, floor));
                let handle = materials.get(&ground.0).map(|m| m.extension.sight.clone());
                if let Some(mut image) = handle.as_ref().and_then(|h| images.get_mut(h)) {
                    image.data = Some(sight.bytes());
                }
                view.sight = Some(sight);
            }
        }
    }
    let target = if covered.is_some() { RADIUS } else { LOW_RADIUS };
    let radius = radius.get_or_insert(target);
    *radius += (target - *radius) * blend;
    let cut = Cut::new(feet, camera.back().as_vec3());
    let corner = view.sight.as_ref().map(|sight| {
        // The texture's lowest corner: tile `origin`'s low edge, in arena meters.
        Vec2::new((sight.origin.0 - view.center.0) as f32 - 0.5, (sight.origin.1 - view.center.1) as f32 - 0.5)
    });
    let Some(mut material) = materials.get_mut(&ground.0) else { return };
    let cutaway = &mut material.extension.cutaway;
    cutaway.center = cut.center.extend(*radius);
    cutaway.toward = cut.toward.extend(EDGE);
    cutaway.planes = Vec4::new(cut.low, cut.head, LOW_RADIUS.min(*radius), 0.0);
    cutaway.section.w = if sight_mode { 0.0 } else { 1.0 };
    cutaway.sight = Vec4::new(view.inside, view.hide, if sight_mode { 1.0 } else { 0.0 }, GREY);
    if let Some(corner) = corner {
        cutaway.window = Vec4::new(corner.x, corner.y, crate::sight::WINDOW as f32, NEAR_TILES);
    }
    cutaway.eye = feet.extend(DIM);
    cutaway.light = Vec4::new(NEAR, FAR, FAR_LIGHT, NEAR_WALL);
    cutaway.inside = Vec4::new(INSIDE_LIGHT, SECTION_DIM, 0.0, 0.0);
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

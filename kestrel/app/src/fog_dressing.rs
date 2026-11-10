//! The fog of war on the town's dressing (M9.3 of the Sacred Tree fog,
//! `SACRED_TREE_FOG.md`): the baked environment (`style::Environment`: the
//! pines, the plaza's grass and dirt, bushes) and the dressing with no place
//! of its own (`cutaway::Outdoor`: lamp posts, the tree's split canopy) are
//! their own materials, not the ground's, so M9.2's fog left them at full
//! colour in ground never seen. In the town each of their standard
//! materials is drawn by a copy extended with the fog (`FoggedMaterial`,
//! `shaders/fog_dressing.wesl`): in sight as it is, seen before dimmed and
//! greyed as the ground, never seen black. The fog's texture and look are
//! the ground's (`cutaway::follow`), so the two match; off in the other
//! modes as the ground's is.
//!
//! The entity keeps its standard material's handle ([`Fogged`]); a change
//! to it (the canopy fading, a lantern's head lighting up at dusk) is copied
//! to the fogged copy. Other scenes have no fog and keep their materials.

use bevy::{
    asset::uuid::Uuid,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use std::marker::PhantomData;

const SHADER: Handle<Shader> = Handle::Uuid(Uuid::from_u128(0x3c2d8f61_7a0e_4b5c_9e1d_5f4a8b2c6d03), PhantomData);

pub type FoggedMaterial = ExtendedMaterial<StandardMaterial, FogExt>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct FogExt {
    #[uniform(100)]
    pub fog: FogUniform,
    /// The fog of war's texture: the ground's (`fog::FogTexture`).
    #[texture(101)]
    #[sampler(102)]
    pub map: Handle<Image>,
}

/// The shader's `FogDressing`: the ground's `fog`, `fog_look` and `void`.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct FogUniform {
    pub fog: Vec4,
    pub fog_look: Vec4,
    pub void: Vec4,
}

impl MaterialExtension for FogExt {
    fn fragment_shader() -> ShaderRef { SHADER.clone().into() }
}

pub fn register_shader(app: &mut App) {
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(SHADER.id(), Shader::from_wesl(include_str!("../shaders/fog_dressing.wesl"), "fog_dressing.wesl"))
        .expect("static fog dressing shader handle");
}

/// A dressing entity drawn with the fog: its standard material, which the
/// fogged copy follows.
#[derive(Component)]
pub struct Fogged(pub Handle<StandardMaterial>);

/// The fogged copies made, by their standard material: kept across worlds,
/// as the style keeps its materials.
#[derive(Resource, Default)]
pub struct FoggedMaterials(Vec<(Handle<StandardMaterial>, Handle<FoggedMaterial>)>);

/// Whether a fogged copy of `base` must take its standard material again:
/// what the town changes on its dressing (the canopy's fade, a lantern's
/// glow).
fn stale(copy: &StandardMaterial, base: &StandardMaterial) -> bool {
    copy.base_color != base.base_color || copy.emissive != base.emissive || copy.alpha_mode != base.alpha_mode
}

/// In the town, draws the dressing with the fog: swaps each dressing
/// entity's standard material for its fogged copy (made once per material),
/// brings the copies up to their materials, and gives them the ground's fog
/// as `cutaway::follow` left it this frame.
#[allow(clippy::type_complexity)]
pub fn follow(
    mut commands: Commands,
    view: Option<Res<crate::cutaway::SightView>>,
    ground: Option<Res<crate::cutaway::CutawayGround>>,
    cutaways: Res<Assets<crate::cutaway::CutawayMaterial>>,
    standard: Res<Assets<StandardMaterial>>,
    mut fogged: ResMut<Assets<FoggedMaterial>>,
    mut made: ResMut<FoggedMaterials>,
    dressing: Query<(Entity, &MeshMaterial3d<StandardMaterial>), (Or<(With<crate::style::Environment>, With<crate::cutaway::Outdoor>)>, Without<Fogged>)>,
) {
    let (Some(view), Some(ground)) = (view, ground) else { return };
    if !view.fog {
        return;
    }
    let Some(ground) = cutaways.get(&ground.0) else { return };
    let uniform = FogUniform { fog: ground.extension.cutaway.fog, fog_look: ground.extension.cutaway.fog_look, void: ground.extension.cutaway.void };
    for (entity, material) in &dressing {
        let copy = match made.0.iter().find(|(base, _)| base.id() == material.0.id()) {
            Some((_, copy)) => copy.clone(),
            None => {
                let Some(base) = standard.get(&material.0) else { continue };
                let copy = fogged.add(ExtendedMaterial { base: base.clone(), extension: FogExt { fog: uniform, map: ground.extension.fog.clone() } });
                made.0.push((material.0.clone(), copy.clone()));
                copy
            }
        };
        commands.entity(entity).remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(copy), Fogged(material.0.clone())));
    }
    for (base, copy) in &made.0 {
        let Some(base) = standard.get(base) else { continue };
        let changed = fogged.get(copy).is_some_and(|c| c.extension.fog != uniform || c.extension.map != ground.extension.fog || stale(&c.base, base));
        if changed {
            if let Some(mut c) = fogged.get_mut(copy) {
                if stale(&c.base, base) {
                    c.base = base.clone();
                }
                c.extension.fog = uniform;
                c.extension.map = ground.extension.fog.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare app with the town's ground material, its fog on, and a pine
    /// (the baked environment) and a lamp post (`Outdoor`).
    fn app(fog: bool) -> (App, Entity, Entity, Handle<StandardMaterial>) {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<crate::cutaway::CutawayMaterial>>()
            .init_resource::<Assets<FoggedMaterial>>()
            .init_resource::<FoggedMaterials>()
            .init_resource::<crate::cutaway::SightView>()
            .add_systems(Update, follow);
        app.world_mut().resource_mut::<crate::cutaway::SightView>().fog = fog;
        let map = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let mut ground = crate::cutaway::material(StandardMaterial::default(), Handle::default(), map);
        ground.extension.cutaway.fog = Vec4::new(-128.5, -128.5, 256.0, 1.0);
        let ground = app.world_mut().resource_mut::<Assets<crate::cutaway::CutawayMaterial>>().add(ground);
        app.insert_resource(crate::cutaway::CutawayGround(ground));
        let leaves = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.2, 0.5, 0.2), ..default() });
        let wood = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let pine = app.world_mut().spawn((crate::style::Environment, MeshMaterial3d(leaves.clone()))).id();
        let post = app.world_mut().spawn((crate::cutaway::Outdoor, MeshMaterial3d(wood.clone()))).id();
        (app, pine, post, leaves)
    }

    #[test]
    fn in_the_town_the_pines_and_lamp_posts_are_fogged() {
        let (mut app, pine, post, leaves) = app(true);
        app.update();
        for e in [pine, post] {
            assert!(app.world().get::<MeshMaterial3d<StandardMaterial>>(e).is_none());
            assert!(app.world().get::<Fogged>(e).is_some());
        }
        let copy = app.world().get::<MeshMaterial3d<FoggedMaterial>>(pine).unwrap().0.clone();
        let fogged = app.world().resource::<Assets<FoggedMaterial>>().get(&copy).unwrap();
        assert_eq!(fogged.extension.fog.fog, Vec4::new(-128.5, -128.5, 256.0, 1.0), "the ground's fog");
        assert_eq!(fogged.extension.fog.void.w, crate::cutaway::FOG_FLOOR);
        assert_eq!(fogged.base.base_color, Color::srgb(0.2, 0.5, 0.2));
        // A change to its material (a lantern lit at dusk) follows.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&leaves).unwrap().emissive = LinearRgba::rgb(4.0, 2.0, 1.0);
        app.update();
        let fogged = app.world().resource::<Assets<FoggedMaterial>>().get(&copy).unwrap();
        assert_eq!(fogged.base.emissive, LinearRgba::rgb(4.0, 2.0, 1.0));
        // F7 off the fog: the ground's fog is off, and so is the copy's.
        let ground = app.world().resource::<crate::cutaway::CutawayGround>().0.clone();
        app.world_mut().resource_mut::<Assets<crate::cutaway::CutawayMaterial>>().get_mut(&ground).unwrap().extension.cutaway.fog.w = 0.0;
        app.update();
        assert_eq!(app.world().resource::<Assets<FoggedMaterial>>().get(&copy).unwrap().extension.fog.fog.w, 0.0);
    }

    #[test]
    fn other_scenes_keep_their_dressing() {
        let (mut app, pine, post, _) = app(false);
        app.update();
        for e in [pine, post] {
            assert!(app.world().get::<MeshMaterial3d<StandardMaterial>>(e).is_some());
            assert!(app.world().get::<Fogged>(e).is_none());
        }
    }
}

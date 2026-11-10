//! The town's canopy (World layer, view only): the camera looks down at the
//! Sacred Tree (from +x, +z by default, from any quarter turn once the view
//! is turned, `input::town::ViewTurn`), so its canopy hides what stands just
//! behind it from there. The test is from the live camera, so it follows the
//! turn.
//! While it hides the elder, a site or a blueprint's ghost, it fades, and
//! comes back once nothing is behind it. The sim never knows.
//!
//! The scene bakes its architecture into one mesh per material
//! (`style::apply`), so the canopy has no entity of its own. [`Canopy::split`]
//! takes the triangles inside the canopy's pieces out of the environment's
//! meshes into meshes of their own, with materials of their own to fade.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::render::render_resource::PrimitiveTopology;
use bevy::prelude::*;
use service::WorldScoped;

/// How high a piece's bottom must be for it to count as canopy, in meters:
/// above the elder's head, so the trunk and the ground stay.
const CANOPY_BOTTOM: f32 = 2.0;
/// How far from the scene's center the tree's canopy reaches, in meters:
/// the tree stands at the clearing's center, and the pines around it are
/// further out.
const CANOPY_REACH: f32 = 4.0;
/// The canopy's opacity while it hides something.
const SEE_THROUGH: f32 = 0.25;
/// How fast it fades, in opacity per second.
const FADE_RATE: f32 = 3.0;

/// The canopy's pieces and the meshes split off for them.
pub struct Canopy {
    /// Each piece as an ellipsoid: its center and half extents.
    pieces: Vec<(Vec3, Vec3)>,
    /// The split-off canopy entities' own materials.
    materials: Vec<Handle<StandardMaterial>>,
    opacity: f32,
}

impl Canopy {
    /// Splits the canopy out of the environment's meshes, once the scene's
    /// style has spawned them. `None` until then; a scene with no raised
    /// architecture near its center gets an empty canopy.
    pub fn split(world: &mut World) -> Option<Canopy> {
        let pack = world.resource::<crate::scene_view::SceneView>().pack.clone();
        let pieces: Vec<(Vec3, Vec3)> = pack
            .scene
            .architecture
            .iter()
            .map(|a| (Vec3::from_array(a.position), Vec3::from_array(a.size) * 0.5))
            .filter(|(center, half)| center.y - half.y >= CANOPY_BOTTOM && Vec2::new(center.x, center.z).length() <= CANOPY_REACH)
            .collect();
        // In the town's fog the environment may be drawn by fogged copies
        // already (`fog_dressing::Fogged`).
        let entities: Vec<(Entity, Handle<Mesh>, Handle<StandardMaterial>)> = world
            .query_filtered::<(Entity, &Mesh3d, AnyOf<(&MeshMaterial3d<StandardMaterial>, &crate::fog_dressing::Fogged)>), With<crate::style::Environment>>()
            .iter(world)
            .filter_map(|(e, mesh, (material, fogged))| Some((e, mesh.0.clone(), material.map(|m| m.0.clone()).or_else(|| fogged.map(|f| f.0.clone()))?)))
            .collect();
        if pack.environment.is_some() && entities.is_empty() {
            return None;
        }
        let mut canopy = Canopy { pieces, materials: Vec::new(), opacity: 1.0 };
        if canopy.pieces.is_empty() {
            return Some(canopy);
        }
        // A triangle is canopy when its center is inside a piece's box, a
        // voxel's width wider.
        let pieces = canopy.pieces.clone();
        let inside = |p: Vec3| pieces.iter().any(|&(center, half)| ((p - center).abs() - half).max_element() <= 0.1);
        for (e, mesh, material) in entities {
            let Some(source) = world.resource::<Assets<Mesh>>().get(&mesh) else { continue };
            let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Float32x3(normals)), Some(indices)) =
                (source.attribute(Mesh::ATTRIBUTE_POSITION), source.attribute(Mesh::ATTRIBUTE_NORMAL), source.indices())
            else {
                continue;
            };
            let indices: Vec<u32> = indices.iter().map(|i| i as u32).collect();
            let (mut rest, mut leaves) = (Vec::new(), Vec::new());
            for t in indices.chunks_exact(3) {
                let center = t.iter().map(|&i| Vec3::from_array(positions[i as usize])).sum::<Vec3>() / 3.0;
                (if inside(center) { &mut leaves } else { &mut rest }).extend_from_slice(t);
            }
            if leaves.is_empty() {
                continue;
            }
            let (positions, normals) = (positions.clone(), normals.clone());
            let part = |indices: Vec<u32>| {
                Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; positions.len()])
                    .with_inserted_indices(Indices::U32(indices))
            };
            let (rest, leaves) = (part(rest), part(leaves));
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            let (rest, leaves) = (meshes.add(rest), meshes.add(leaves));
            let own = world.resource::<Assets<StandardMaterial>>().get(&material).cloned().unwrap_or_default();
            let own = world.resource_mut::<Assets<StandardMaterial>>().add(own);
            // The cached environment mesh stays as it was for the next world.
            world.entity_mut(e).insert(Mesh3d(rest));
            world.spawn((WorldScoped::default(), crate::cutaway::Outdoor, Mesh3d(leaves), MeshMaterial3d(own.clone()), Transform::default()));
            canopy.materials.push(own);
        }
        Some(canopy)
    }

    /// Fades the canopy while it stands between the camera and any of
    /// `targets` (points in the scene), and brings it back once none is
    /// behind it, over `dt` seconds.
    pub fn fade(&mut self, world: &mut World, targets: &[Vec3], dt: f32) {
        if self.materials.is_empty() {
            return;
        }
        let camera = world.query_filtered::<&GlobalTransform, With<Camera3d>>().iter(world).next().map(|t| t.translation());
        let Some(eye) = camera else { return };
        let hides = |target: Vec3| self.pieces.iter().any(|&(center, half)| segment_meets_ellipsoid(eye, target, center, half));
        let wanted = if targets.iter().any(|&t| hides(t)) { SEE_THROUGH } else { 1.0 };
        let step = FADE_RATE * dt;
        let opacity = if wanted < self.opacity { (self.opacity - step).max(wanted) } else { (self.opacity + step).min(wanted) };
        if opacity == self.opacity {
            return;
        }
        self.opacity = opacity;
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        for handle in &self.materials {
            let Some(mut material) = materials.get_mut(handle) else { continue };
            material.base_color.set_alpha(opacity);
            // Opaque again once whole, so it sorts and shadows as before.
            material.alpha_mode = if opacity < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque };
        }
    }

    /// The canopy's opacity now, 1 when whole.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }
}

/// Whether the segment from `a` to `b` passes through the ellipsoid at
/// `center` with half extents `half`.
fn segment_meets_ellipsoid(a: Vec3, b: Vec3, center: Vec3, half: Vec3) -> bool {
    // In the ellipsoid's own space it is the unit sphere.
    let (a, b) = ((a - center) / half, (b - center) / half);
    let d = b - a;
    let t = if d.length_squared() > 0.0 { (-a.dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (a + d * t).length_squared() < 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_through_the_canopy_meets_it() {
        let (center, half) = (Vec3::new(0.0, 4.3, 0.0), Vec3::new(2.8, 1.4, 2.8));
        // From high up at +x, +z down to a spot just behind the tree.
        let eye = Vec3::new(14.0, 18.0, 14.0);
        assert!(segment_meets_ellipsoid(eye, Vec3::new(-2.0, 1.0, -2.0), center, half));
        // In front of it, nothing is hidden.
        assert!(!segment_meets_ellipsoid(eye, Vec3::new(3.0, 1.0, 3.0), center, half));
    }

    #[test]
    fn turned_half_way_round_the_canopy_hides_the_other_side() {
        let (center, half) = (Vec3::new(0.0, 4.3, 0.0), Vec3::new(2.8, 1.4, 2.8));
        // The view turned two quarter turns: the camera at -x, -z.
        let eye = Quat::from_rotation_y(std::f32::consts::PI) * Vec3::new(14.0, 18.0, 14.0);
        assert!(segment_meets_ellipsoid(eye, Vec3::new(2.0, 1.0, 2.0), center, half));
        // What hid behind the tree by default stands in front now.
        assert!(!segment_meets_ellipsoid(eye, Vec3::new(-3.0, 1.0, -3.0), center, half));
    }
}

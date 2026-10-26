//! Material-aware contact queries and reusable impact targets, independent of prefab shape.
use bevy::{math::Affine3A, prelude::*};
use std::sync::Arc;
use voxel_assets::{ImpactResponse, Model};

#[derive(Clone, Copy)]
pub struct Response {
    pub color: [f32; 3],
    pub sparks: usize,
    pub debris: usize,
    pub speed: f32,
    pub flash_seconds: f32,
    pub kick: f32,
    pub hit_stop: f32,
    pub trauma: f32,
    pub light: f32,
}
pub fn response(kind: ImpactResponse) -> Response {
    match kind {
        ImpactResponse::Stone => Response {
            color: [1.0, 0.38, 0.08],
            sparks: 12,
            debris: 18,
            speed: 5.0,
            flash_seconds: 0.12,
            kick: 0.025,
            hit_stop: 0.058,
            trauma: 0.48,
            light: 18000.0,
        },
        ImpactResponse::Metal => Response {
            color: [1.0, 0.65, 0.22],
            sparks: 48,
            debris: 3,
            speed: 11.0,
            flash_seconds: 0.09,
            kick: 0.012,
            hit_stop: 0.075,
            trauma: 0.58,
            light: 45000.0,
        },
        ImpactResponse::Crystal => Response {
            color: [0.18, 0.65, 1.0],
            sparks: 20,
            debris: 16,
            speed: 6.0,
            flash_seconds: 0.4,
            kick: 0.035,
            hit_stop: 0.045,
            trauma: 0.35,
            light: 28000.0,
        },
        ImpactResponse::Polymer => Response {
            color: [0.32, 1.0, 0.48],
            sparks: 0,
            debris: 10,
            speed: 2.8,
            flash_seconds: 0.06,
            kick: 0.10,
            hit_stop: 0.025,
            trauma: 0.18,
            light: 0.0,
        },
    }
}
pub fn index(kind: ImpactResponse) -> usize {
    match kind {
        ImpactResponse::Stone => 0,
        ImpactResponse::Metal => 1,
        ImpactResponse::Crystal => 2,
        ImpactResponse::Polymer => 3,
    }
}
pub const KINDS: [ImpactResponse; 4] = [
    ImpactResponse::Stone,
    ImpactResponse::Metal,
    ImpactResponse::Crystal,
    ImpactResponse::Polymer,
];

pub struct Surface {
    vertices: [Vec3; 3],
    material: usize,
}
pub fn surfaces(model: &Model) -> Arc<Vec<Surface>> {
    Arc::new(
        model
            .meshes
            .iter()
            .flat_map(|mesh| {
                mesh.indices.chunks_exact(3).map(move |tri| Surface {
                    vertices: [tri[0], tri[1], tri[2]]
                        .map(|i| Vec3::from_array(mesh.positions[i as usize])),
                    material: mesh.material,
                })
            })
            .collect(),
    )
}

pub struct ImpactSlot {
    pub handle: Handle<StandardMaterial>,
    pub rest_emissive: LinearRgba,
    pub kind: ImpactResponse,
    pub flash: f32,
}
#[derive(Component)]
pub struct ImpactTarget {
    pub base: Vec3,
    pub contact_radius: f32,
    pub kick: Vec3,
    pub slots: Vec<ImpactSlot>,
    pub surfaces: Arc<Vec<Surface>>,
    pub local_from_world: Affine3A,
}
impl ImpactTarget {
    /// Exact segment/triangle contact in prefab space. This follows yaw, scale,
    /// translation and recoil, and returns the first actual surface material.
    pub fn contact(&self, start: Vec3, end: Vec3) -> Option<(f32, usize, Vec3)> {
        let a = self.local_from_world.transform_point3(start - self.kick);
        let b = self.local_from_world.transform_point3(end - self.kick);
        let direction = b - a;
        self.surfaces
            .iter()
            .filter_map(|surface| {
                let [v0, v1, v2] = surface.vertices;
                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let p = direction.cross(e2);
                let det = e1.dot(p);
                if det.abs() < 1e-7 {
                    return None;
                }
                let tvec = a - v0;
                let u = tvec.dot(p) / det;
                let q = tvec.cross(e1);
                let v = direction.dot(q) / det;
                let t = e2.dot(q) / det;
                if u < -1e-5 || v < -1e-5 || u + v > 1.00001 || !(0.0..=1.0).contains(&t) {
                    return None;
                }
                let local_normal = e1.cross(e2).normalize();
                // Instance transforms use positive uniform scale, so inverse rotation
                // through the inverse matrix is sufficient for transforming normals.
                let mut normal = self
                    .local_from_world
                    .inverse()
                    .transform_vector3(local_normal)
                    .normalize();
                if normal.dot(end - start) > 0.0 {
                    normal = -normal;
                }
                Some((t, surface.material, normal))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel_assets::{generate_boulder, BoulderRecipe, MaterialPattern};

    #[test]
    fn mixed_surface_contacts_follow_instance_transform_and_recoil() {
        let model = generate_boulder(&BoulderRecipe {
            radius: 1.15,
            height: 1.6,
            voxel_size: 0.25,
            taper: 0.15,
            debris: 0,
            material_pattern: MaterialPattern::Quadrants,
            ..Default::default()
        })
        .unwrap();
        let transform = Transform::from_xyz(4.0, 0.3, -2.0)
            .with_rotation(Quat::from_rotation_y(0.7))
            .with_scale(Vec3::splat(1.7));
        let target = ImpactTarget {
            base: transform.translation,
            contact_radius: 2.0,
            kick: Vec3::new(0.03, 0.0, -0.02),
            slots: vec![],
            surfaces: surfaces(&model),
            local_from_world: transform.compute_affine().inverse(),
        };
        for (x, z, material) in [
            (-0.4, -3.0, 0),
            (0.4, -3.0, 1),
            (-0.4, 3.0, 2),
            (0.4, 3.0, 3),
        ] {
            let start = transform.transform_point(Vec3::new(x, 0.9, z)) + target.kick;
            let end = transform.transform_point(Vec3::new(x, 0.9, 0.0)) + target.kick;
            let (t, actual, normal) = target.contact(start, end).expect("surface hit");
            assert_eq!(actual, material);
            assert!((0.0..1.0).contains(&t));
            assert!(normal.dot(end - start) < 0.0);
        }
        let start = transform.transform_point(Vec3::new(0.4, 4.0, -3.0)) + target.kick;
        let end = transform.transform_point(Vec3::new(0.4, 4.0, 3.0)) + target.kick;
        assert!(target.contact(start, end).is_none());
    }

    #[test]
    fn material_flash_is_instance_local_and_restores_authored_emission() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<crate::effects::Feedback>()
            .init_resource::<crate::effects::CoverContacts>()
            .insert_resource(crate::sound::Sound::silent())
            .insert_resource(crate::effects::FxAssets {
                cube: default(),
                spark: std::array::from_fn(|_| default()),
                debris: std::array::from_fn(|_| default()),
                trail: default(),
                laser: default(),
                blood: std::array::from_fn(|_| default()),
            })
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, crate::effects::update_effects);
        let rest = LinearRgba::rgb(0.2, 0.4, 0.8);
        let handles: Vec<_> = (0..3)
            .map(|_| {
                app.world_mut()
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(StandardMaterial {
                        emissive: rest,
                        ..default()
                    })
            })
            .collect();
        for (instance, slots) in [
            vec![handles[0].clone(), handles[1].clone()],
            vec![handles[2].clone()],
        ]
        .into_iter()
        .enumerate()
        {
            app.world_mut().spawn((
                Transform::default(),
                ImpactTarget {
                    base: Vec3::ZERO,
                    contact_radius: 1.0,
                    kick: Vec3::ZERO,
                    slots: slots
                        .into_iter()
                        .enumerate()
                        .map(|(slot, handle)| ImpactSlot {
                            handle,
                            rest_emissive: rest,
                            kind: ImpactResponse::Crystal,
                            flash: if instance == 0 && slot == 0 { 1.0 } else { 0.0 },
                        })
                        .collect(),
                    surfaces: Arc::new(vec![]),
                    local_from_world: Affine3A::IDENTITY,
                },
            ));
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
        {
            let materials = app.world().resource::<Assets<StandardMaterial>>();
            assert!(materials.get(&handles[0]).unwrap().emissive.blue > rest.blue);
            for handle in &handles[1..] {
                assert_eq!(materials.get(handle).unwrap().emissive, rest);
            }
        }
        for _ in 0..5 {
            app.update();
        }
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        for handle in &handles {
            assert_eq!(materials.get(handle).unwrap().emissive, rest);
        }
    }
}

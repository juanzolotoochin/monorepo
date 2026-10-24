//! Movement controls, pointer targeting and route display.
use crate::{character::Player, combat::Attack};
use bevy::prelude::*;

pub const WALK_SPEED: f32 = 3.8;

#[derive(Resource, Default)]
pub struct Controls {
    pub attack_serial: u64,
    pub weapon: content::scenario::Weapon,
    pub route: Vec<Vec3>,
    pub velocity: Vec3,
    pub stride: f32,
    pub foot_distance: f32,
    pub attack: Option<Attack>,
    pub queued: Option<(Vec3, f32)>,
    pub repath: f32,
    pub impacts: u32,
    pub last_material: Option<voxel_assets::ImpactResponse>,
}
impl Controls {
    pub fn strike(&mut self, aim: Vec3) {
        if self.attack.is_none() {
            self.attack_serial += 1;
            let mut attack = Attack::new(aim, self.weapon);
            attack.serial = self.attack_serial;
            self.attack = Some(attack);
            self.route.clear();
        } else {
            // A short recovery buffer accepts the next deliberate click, not a held button.
            self.queued = Some((aim, 0.18));
        }
    }
}

pub fn cursor_ground(
    camera: &Camera,
    transform: &GlobalTransform,
    window: &Window,
) -> Option<Vec3> {
    let ray = camera
        .viewport_to_world(transform, window.cursor_position()?)
        .ok()?;
    Some(ray.get_point(ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?))
}

pub fn draw_navigation(
    controls: Res<Controls>,
    player: Single<&Transform, With<Player>>,
    mut gizmos: Gizmos,
    library: Res<crate::assets::SceneLibrary>,
    flight: Res<crate::jetpack::Flight>,
    time: Res<Time>,
) {
    if let Some(goal) = controls.route.last() {
        let color = Color::srgba(0.25, 0.95, 1.0, 0.7);
        let y = Vec3::Y * 0.035;
        let waypoint = |p: Vec3| {
            Vec3::new(
                p.x,
                crate::jetpack::height(&library, p).unwrap_or(flight.support)
                    + if flight.airborne {
                        crate::jetpack::HOVER
                    } else {
                        0.0
                    },
                p.z,
            )
        };
        let r = 0.24 + 0.025 * (time.elapsed_secs() * 7.0).sin();
        gizmos.circle(
            Isometry3d::new(
                waypoint(*goal) + y,
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
            r,
            color,
        );
        let mut previous = player.translation + y;
        for point in &controls.route {
            let end = waypoint(*point) + y;
            let delta = end - previous;
            let count = (delta.length() / 0.3) as usize;
            for i in 0..count {
                let a = previous + delta.normalize_or_zero() * i as f32 * 0.3;
                gizmos.line(
                    a,
                    a + delta.normalize_or_zero() * 0.08,
                    Color::srgba(0.22, 0.65, 0.72, 0.3),
                );
            }
            previous = end;
        }
    }
}

//! Movement controls, pointer targeting and route display.
use crate::{
    character::{GaitClock, Player},
    combat::Attack,
};
use bevy::prelude::*;
use sim::settlement::avatar::SwordStroke;

pub const WALK_SPEED: f32 = 3.8;

#[derive(Resource, Default)]
pub struct Controls {
    /// Current world-space mouse aim, independent of movement and firing.
    pub aim: Option<Vec3>,
    pub swift_sword: bool,
    pub attack_serial: u64,
    pub combo_next: SwordStroke,
    pub combo_timer: f32,
    pub weapon: content::scenario::Weapon,
    pub route: Vec<Vec3>,
    pub velocity: Vec3,
    pub gait: GaitClock,
    pub attack: Option<Attack>,
    pub queued: Option<Vec3>,
    pub repath: f32,
    pub impacts: u32,
    pub last_material: Option<voxel_assets::ImpactResponse>,
}
impl Controls {
    pub fn strike(&mut self, aim: Vec3) {
        if self.attack.is_none() {
            self.attack_serial += 1;
            let mut attack = Attack::new(aim, self.weapon);
            attack.swift = self.swift_sword;
            if self.weapon == content::scenario::Weapon::Sword {
                attack.stroke = if self.combo_timer > 0.0 {
                    self.combo_next
                } else {
                    SwordStroke::Forehand
                };
                self.combo_next = attack.stroke.next();
            } else {
                self.combo_next = SwordStroke::Forehand;
            }
            self.combo_timer = 1.25;
            attack.serial = self.attack_serial;
            self.attack = Some(attack);
            if self.weapon != content::scenario::Weapon::Laser {
                self.route.clear();
            }
        } else {
            // One deliberate press can queue the next cut during this one.
            self.queued = Some(aim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sword_inputs_cycle_within_window_and_reset_after_pause() {
        let mut controls = Controls::default();
        for stroke in [
            SwordStroke::Forehand,
            SwordStroke::Backhand,
            SwordStroke::Overhead,
        ] {
            controls.strike(Vec3::Z);
            assert_eq!(controls.attack.unwrap().stroke, stroke);
            controls.attack = None;
        }
        controls.combo_timer = 0.0;
        controls.strike(Vec3::Z);
        assert_eq!(controls.attack.unwrap().stroke, SwordStroke::Forehand);
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
    scene: Res<crate::scene_view::SceneView>,
    flight: Res<crate::jetpack::Flight>,
    time: Res<Time>,
) {
    if let Some(goal) = controls.route.last() {
        let color = Color::srgba(0.25, 0.95, 1.0, 0.7);
        let y = Vec3::Y * 0.035;
        let waypoint = |p: Vec3| {
            Vec3::new(
                p.x,
                crate::jetpack::height(&scene.pack, p).unwrap_or(flight.support)
                    + if flight.airborne && !scene.has_feature(voxel_assets::SceneFeature::Jump) {
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

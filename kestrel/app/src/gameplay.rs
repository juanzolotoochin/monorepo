//! Movement controls and pointer targeting.
use crate::{
    character::GaitClock,
    combat::Attack,
};
use bevy::prelude::*;
use sim::settlement::avatar::SwordStroke;

#[derive(Resource, Default)]
pub struct Controls {
    /// Current world-space mouse aim, independent of movement and firing.
    pub aim: Option<Vec3>,
    pub swift_sword: bool,
    pub attack_serial: u64,
    pub combo_next: SwordStroke,
    pub combo_timer: f32,
    pub weapon: content::scenario::Weapon,
    pub velocity: Vec3,
    pub gait: GaitClock,
    pub attack: Option<Attack>,
    pub queued: Option<Vec3>,
    /// The tick the queued press's command asked for (`Attack::asked`).
    pub queued_asked: Option<u64>,
    pub impacts: u32,
    pub last_material: Option<voxel_assets::ImpactResponse>,
}
impl Controls {
    pub fn strike(&mut self, aim: Vec3) {
        self.strike_at(aim, None);
    }

    /// Swings now, or queues the press during a swing. `asked` is the tick
    /// the local player's `Attack` command asked for, which lets the swing
    /// follow the drawn avatar's (`combat::follow_drawn_swing`).
    pub fn strike_at(&mut self, aim: Vec3, asked: Option<u64>) {
        if self.attack.is_none() {
            self.attack_serial += 1;
            let mut attack = Attack::new(aim, self.weapon);
            attack.swift = self.swift_sword;
            // Predicts the stroke the simulation will pick, with its rule,
            // until the drawn avatar shows it.
            (attack.stroke, self.combo_next) =
                SwordStroke::combo(self.weapon, self.combo_next, self.combo_timer > 0.0);
            self.combo_timer = crate::combat::combo_window();
            attack.serial = self.attack_serial;
            attack.asked = asked;
            self.attack = Some(attack);
        } else {
            // One deliberate press can queue the next cut during this one.
            self.queued = Some(aim);
            self.queued_asked = asked;
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

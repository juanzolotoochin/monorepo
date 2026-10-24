//! View-side geometry. Routes and collision belong to the simulation
//! (`game/sim::nav`); these helpers place effects and hit-test props.
use bevy::prelude::*;
/// The original arena half-width, for scenes without a scenario.
pub const EDGE: f32 = 8.0;
/// The player's collision radius, as in the simulation.
pub const RADIUS: f32 = 0.32;
#[derive(Clone, Copy)]
pub struct Disc {
    pub center: Vec3,
    pub radius: f32,
}
pub fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
pub fn segment_distance(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = flat(b - a);
    let t = (flat(point - a).dot(ab) / ab.length_squared().max(1e-8)).clamp(0.0, 1.0);
    flat(point - a - ab * t).length()
}

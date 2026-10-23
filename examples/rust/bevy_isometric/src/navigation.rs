//! Bevy vector adapter for the shared deterministic navigation grid.
use bevy::prelude::*;
pub const EDGE: f32 = gameplay_core::navigation::EDGE;
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
fn discs(obstacles: &[Disc]) -> Vec<gameplay_core::navigation::Disc> {
    obstacles
        .iter()
        .map(|d| gameplay_core::navigation::Disc {
            center: [d.center.x, d.center.z],
            radius: d.radius,
        })
        .collect()
}
#[cfg(test)]
pub fn clear(a: Vec3, b: Vec3, obstacles: &[Disc]) -> bool {
    gameplay_core::navigation::clear([a.x, a.z], [b.x, b.z], &discs(obstacles), RADIUS)
}
pub fn path(start: Vec3, goal: Vec3, obstacles: &[Disc]) -> Vec<Vec3> {
    path_with(start, goal, obstacles, |_, _| true)
}
pub fn path_with(
    start: Vec3,
    goal: Vec3,
    obstacles: &[Disc],
    valid: impl Fn(Vec3, Vec3) -> bool,
) -> Vec<Vec3> {
    gameplay_core::navigation::path_with(
        [start.x, start.z],
        [goal.x, goal.z],
        &discs(obstacles),
        RADIUS,
        |a, b| valid(Vec3::new(a[0], 0.0, a[1]), Vec3::new(b[0], 0.0, b[1])),
    )
    .into_iter()
    .map(|p| Vec3::new(p[0], 0.0, p[1]))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn route_goes_around_rock_with_body_clearance() {
        let obstacles = [Disc {
            center: Vec3::ZERO,
            radius: 1.0,
        }];
        let start = Vec3::new(-3.0, 0.0, 0.0);
        let goal = Vec3::new(3.0, 0.0, 0.0);
        let route = path(start, goal, &obstacles);
        assert!(route.len() >= 2);
        assert_eq!(route.last(), Some(&goal));
        let mut previous = start;
        for p in route {
            assert!(clear(previous, p, &obstacles));
            previous = p;
        }
        assert!(path(start, Vec3::ZERO, &obstacles).is_empty());
        assert!(path(start, Vec3::X * 9.0, &obstacles).is_empty());
    }
}

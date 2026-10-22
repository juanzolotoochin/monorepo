//! Small, deterministic A* navigation with clearance for the player's body.
use bevy::prelude::*;
use std::{cmp::Reverse, collections::BinaryHeap};

pub const EDGE: f32 = 8.0;
pub const RADIUS: f32 = 0.32;
const SPACING: f32 = 0.5;
const SIDE: usize = 33;

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

pub fn clear(a: Vec3, b: Vec3, obstacles: &[Disc]) -> bool {
    [a, b]
        .iter()
        .all(|p| p.x.abs() <= EDGE && p.z.abs() <= EDGE)
        && obstacles
            .iter()
            .all(|o| segment_distance(o.center, a, b) > o.radius + RADIUS + 0.015)
}

fn point(index: usize) -> Vec3 {
    Vec3::new(
        (index % SIDE) as f32 * SPACING - EDGE,
        0.0,
        (index / SIDE) as f32 * SPACING - EDGE,
    )
}

fn nearest_visible(p: Vec3, obstacles: &[Disc]) -> Option<usize> {
    (0..SIDE * SIDE)
        .filter(|&i| clear(p, point(i), obstacles))
        .min_by(|&a, &b| {
            point(a)
                .distance_squared(p)
                .total_cmp(&point(b).distance_squared(p))
        })
}

pub fn path(start: Vec3, goal: Vec3, obstacles: &[Disc]) -> Vec<Vec3> {
    let start = flat(start);
    let goal = flat(goal);
    if !clear(goal, goal, obstacles) {
        return vec![];
    }
    if clear(start, goal, obstacles) {
        return vec![goal];
    }
    let (Some(first), Some(last)) = (
        nearest_visible(start, obstacles),
        nearest_visible(goal, obstacles),
    ) else {
        return vec![];
    };
    let mut costs = vec![u32::MAX; SIDE * SIDE];
    let mut parents = vec![usize::MAX; SIDE * SIDE];
    let heuristic = |i: usize| -> u32 {
        let dx = (i % SIDE).abs_diff(last % SIDE) as u32;
        let dz = (i / SIDE).abs_diff(last / SIDE) as u32;
        10 * dx.max(dz) + 4 * dx.min(dz)
    };
    let mut open = BinaryHeap::new();
    costs[first] = 0;
    open.push(Reverse((heuristic(first), 0, first)));
    while let Some(Reverse((_, cost, current))) = open.pop() {
        if cost != costs[current] {
            continue;
        }
        if current == last {
            break;
        }
        let (x, z) = ((current % SIDE) as i32, (current / SIDE) as i32);
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let (nx, nz) = (x + dx, z + dz);
                if !(0..SIDE as i32).contains(&nx) || !(0..SIDE as i32).contains(&nz) {
                    continue;
                }
                let next = nz as usize * SIDE + nx as usize;
                let new_cost = cost + if dx != 0 && dz != 0 { 14 } else { 10 };
                if new_cost < costs[next] && clear(point(current), point(next), obstacles) {
                    costs[next] = new_cost;
                    parents[next] = current;
                    open.push(Reverse((new_cost + heuristic(next), new_cost, next)));
                }
            }
        }
    }
    if costs[last] == u32::MAX {
        return vec![];
    }
    let mut raw = vec![goal];
    let mut node = last;
    loop {
        raw.push(point(node));
        if node == first {
            break;
        }
        node = parents[node];
    }
    raw.reverse();
    // String pulling removes the grid's stair-step turns.
    let mut result = vec![];
    let mut from = start;
    let mut i = 0;
    while i < raw.len() {
        let next = (i..raw.len())
            .rev()
            .find(|&j| clear(from, raw[j], obstacles))
            .unwrap_or(i);
        from = raw[next];
        result.push(from);
        i = next + 1;
    }
    result
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

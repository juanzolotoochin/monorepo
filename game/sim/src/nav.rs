//! Clearance-aware A* on a half-tile grid, ported from the original game's
//! navigation in fixed point. Paths are string-pulled into straight segments.
//! Every query is a pure function of the terrain, so the read phase can plan
//! in parallel.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use fx::{Fx, FxVec2};

use crate::terrain::{Disc, Terrain};

const SPACING: Fx = Fx::HALF;

struct Grid<'a> {
    terrain: &'a Terrain,
    extra: &'a [Disc],
    radius: Fx,
    side: usize,
    origin: FxVec2,
}

impl Grid<'_> {
    fn point(&self, index: usize) -> FxVec2 {
        FxVec2::new(
            self.origin.x + SPACING.mul_int((index % self.side) as i64),
            self.origin.y + SPACING.mul_int((index / self.side) as i64),
        )
    }

    fn clear(&self, a: FxVec2, b: FxVec2) -> bool {
        self.terrain.clear_with(a, b, self.radius, self.extra)
    }

    /// The grid node nearest to `p` that `p` can see.
    fn nearest_visible(&self, p: FxVec2) -> Option<usize> {
        (0..self.side * self.side)
            .filter(|&i| self.clear(p, self.point(i)))
            .min_by_key(|&i| ((self.point(i) - p).length_squared(), i))
    }
}

/// A path from `start` to `goal` for a body of `radius`, as waypoints ending
/// at `goal`, or empty if there is none.
pub fn path(terrain: &Terrain, start: FxVec2, goal: FxVec2, radius: Fx) -> Vec<FxVec2> {
    path_avoiding(terrain, start, goal, radius, &[])
}

/// [`path`], also avoiding `extra` discs (other bodies).
pub fn path_avoiding(terrain: &Terrain, start: FxVec2, goal: FxVec2, radius: Fx, extra: &[Disc]) -> Vec<FxVec2> {
    let side = (terrain.extent.mul_int(2) / SPACING).round().floor_to_int() as usize + 1;
    let grid = Grid { terrain, extra, radius, side, origin: terrain.min() };
    if !grid.clear(goal, goal) {
        return Vec::new();
    }
    if grid.clear(start, goal) {
        return vec![goal];
    }
    let (Some(first), Some(last)) = (grid.nearest_visible(start), grid.nearest_visible(goal)) else {
        return Vec::new();
    };
    let mut costs = vec![u32::MAX; side * side];
    let mut parents = vec![usize::MAX; side * side];
    let heuristic = |i: usize| -> u32 {
        let dx = (i % side).abs_diff(last % side) as u32;
        let dy = (i / side).abs_diff(last / side) as u32;
        10 * dx.max(dy) + 4 * dx.min(dy)
    };
    let mut open = BinaryHeap::new();
    costs[first] = 0;
    open.push(Reverse((heuristic(first), 0u32, first)));
    while let Some(Reverse((_, cost, current))) = open.pop() {
        if cost != costs[current] {
            continue;
        }
        if current == last {
            break;
        }
        let (x, y) = ((current % side) as i64, (current / side) as i64);
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= side as i64 || ny >= side as i64 {
                    continue;
                }
                let next = ny as usize * side + nx as usize;
                let new_cost = cost + if dx != 0 && dy != 0 { 14 } else { 10 };
                if new_cost < costs[next] && grid.clear(grid.point(current), grid.point(next)) {
                    costs[next] = new_cost;
                    parents[next] = current;
                    open.push(Reverse((new_cost + heuristic(next), new_cost, next)));
                }
            }
        }
    }
    if costs[last] == u32::MAX {
        return Vec::new();
    }
    let mut raw = vec![goal];
    let mut node = last;
    loop {
        raw.push(grid.point(node));
        if node == first {
            break;
        }
        node = parents[node];
    }
    raw.reverse();
    // String pulling removes the grid's stair steps.
    let mut result = Vec::new();
    let mut from = start;
    let mut i = 0;
    while i < raw.len() {
        let next = (i..raw.len()).rev().find(|&j| grid.clear(from, raw[j])).unwrap_or(i);
        from = raw[next];
        result.push(from);
        i = next + 1;
    }
    result
}

pub fn distance(a: FxVec2, b: FxVec2) -> Fx {
    (b - a).length()
}

/// The heading from `a` to `b`: 0 along +y, increasing toward +x.
pub fn bearing(a: FxVec2, b: FxVec2) -> Fx {
    let d = b - a;
    Fx::atan2(d.x, d.y)
}

/// `to - from` wrapped into [-PI, PI).
pub fn angle_between(from: Fx, to: Fx) -> Fx {
    let v = to - from + Fx::PI;
    v - (v / Fx::TAU).floor() * Fx::TAU - Fx::PI
}

/// The unit vector for a heading (0 along +y).
pub fn heading_vector(heading: Fx) -> FxVec2 {
    let (sin, cos) = heading.sin_cos();
    FxVec2::new(sin, cos)
}

/// A route for a body towing a trailer on a hitch. A sharp change of
/// direction first follows a sampled turning circle, so the body never
/// reverses through its trailer; the trailer's swept path is checked too.
#[allow(clippy::too_many_arguments)]
pub fn articulated_path(
    terrain: &Terrain,
    start: FxVec2,
    goal: FxVec2,
    heading: Fx,
    trailer: FxVec2,
    hitch: Fx,
    radius: Fx,
    trailer_radius: Fx,
) -> Vec<FxVec2> {
    let direct = path(terrain, start, goal, radius);
    if distance(start, goal) < Fx::from_ratio(1, 10) {
        return direct;
    }
    let Some(&first) = direct.first() else { return direct };
    if angle_between(heading, bearing(start, first)).abs() < Fx::from_ratio(9, 10) {
        return direct;
    }
    let turn_radius = hitch.max(Fx::from_ratio(6, 5));
    let (sin_h, cos_h) = heading.sin_cos();
    let mut best = Vec::new();
    let mut best_length = Fx::MAX;
    for side in [Fx::ONE, -Fx::ONE] {
        for sixths in 2..=10 {
            let turn = Fx::PI.mul_int(sixths).div_int(6);
            let steps = (turn * turn_radius / Fx::from_ratio(3, 20)).ceil().floor_to_int() as i64;
            let mut route = Vec::with_capacity(steps as usize + 8);
            let mut previous = start;
            let mut cart = trailer;
            let mut valid = true;
            for i in 1..=steps {
                let theta = turn.mul_int(i).div_int(steps);
                let (sin_t, cos_t) = theta.sin_cos();
                let forward = turn_radius * sin_t;
                let sideways = turn_radius * (Fx::ONE - cos_t) * side;
                let next = FxVec2::new(
                    start.x + sin_h * forward + cos_h * sideways,
                    start.y + cos_h * forward - sin_h * sideways,
                );
                let d = distance(cart, next);
                let next_cart = if d > hitch { next + (cart - next) * (hitch / d) } else { cart };
                if !terrain.clear(previous, next, radius)
                    || !terrain.clear(cart, next_cart, trailer_radius)
                    || distance(next, next_cart) < radius + trailer_radius + Fx::from_ratio(1, 20)
                {
                    valid = false;
                    break;
                }
                route.push(next);
                previous = next;
                cart = next_cart;
            }
            if !valid {
                continue;
            }
            let tail = path(terrain, previous, goal, radius);
            let Some(&next) = tail.first() else { continue };
            if angle_between(heading + side * turn, bearing(previous, next)).abs() > Fx::from_ratio(3, 4) {
                continue;
            }
            let mut length = turn * turn_radius;
            for p in &tail {
                length += distance(previous, *p);
                previous = *p;
            }
            if length < best_length {
                route.extend(tail);
                best = route;
                best_length = length;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(v: f64) -> Fx {
        Fx::from_bits((v * 4294967296.0).round() as i64)
    }

    fn v(x: f64, y: f64) -> FxVec2 {
        FxVec2::new(fx(x), fx(y))
    }

    fn arena(extent: i32) -> Terrain {
        Terrain::flat(v(0.0, 0.0), Fx::from_int(extent))
    }

    #[test]
    fn straight_when_clear_and_around_obstacles_otherwise() {
        let mut terrain = arena(8);
        assert_eq!(path(&terrain, v(-3.0, 0.0), v(3.0, 0.0), fx(0.2)), vec![v(3.0, 0.0)]);
        terrain.obstacles.push(Disc { center: v(0.0, 0.0), radius: Fx::ONE });
        let route = path(&terrain, v(-3.0, 0.0), v(3.0, 0.0), fx(0.2));
        assert!(route.len() > 1);
        assert_eq!(route.last(), Some(&v(3.0, 0.0)));
        let mut from = v(-3.0, 0.0);
        for &p in &route {
            assert!(terrain.clear(from, p, fx(0.2)));
            from = p;
        }
    }

    /// From the original game's tests.
    #[test]
    fn larger_scenes_route_around_obstacles_within_bounds() {
        let obstacle = Disc { center: v(9.0, 0.0), radius: fx(0.9) };
        let mut small = arena(8);
        small.obstacles.push(obstacle);
        assert!(path(&small, v(7.0, 0.0), v(11.0, 0.0), fx(0.2)).is_empty());
        let mut big = arena(12);
        big.obstacles.push(obstacle);
        let route = path(&big, v(7.0, 0.0), v(11.0, 0.0), fx(0.2));
        assert!(route.len() > 1);
        assert_eq!(route.last(), Some(&v(11.0, 0.0)));
        assert!(route.iter().all(|p| p.x.abs() <= fx(12.0) && p.y.abs() <= fx(12.0)));
        assert!(path(&arena(12), v(0.0, 0.0), v(25.0, 0.0), fx(0.2)).is_empty());
    }

    #[test]
    fn walls_without_gaps_have_no_path() {
        let mut terrain = arena(8);
        for n in -16..=16 {
            terrain.obstacles.push(Disc { center: v(0.0, n as f64 * 0.5), radius: fx(0.3) });
        }
        assert!(path(&terrain, v(-3.0, 0.0), v(3.0, 0.0), fx(0.18)).is_empty());
    }

    /// Digging reshapes paths deterministically.
    #[test]
    fn digging_opens_a_shorter_route() {
        let mut terrain = arena(8);
        // A wall at x = 0 with a gap below y = -4.
        for y in -4..8 {
            terrain.add_rock((0, y));
        }
        let length = |route: &[FxVec2]| {
            let mut from = v(-3.0, 0.0);
            route.iter().fold(Fx::ZERO, |sum, &p| {
                let d = distance(from, p);
                from = p;
                sum + d
            })
        };
        let around = path(&terrain, v(-3.0, 0.0), v(3.0, 0.0), fx(0.2));
        assert!(!around.is_empty(), "the gap is open");
        for y in -1..=1 {
            assert!(terrain.dig((0, y)));
        }
        let through = path(&terrain, v(-3.0, 0.0), v(3.0, 0.0), fx(0.2));
        assert!(length(&through) < length(&around));
        assert_eq!(through, path(&terrain.clone(), v(-3.0, 0.0), v(3.0, 0.0), fx(0.2)));
    }

    /// From the original game's tests: a cart reversal turns forward and the
    /// animal never clips its cart.
    #[test]
    fn cart_reversal_uses_a_forward_arc() {
        let terrain = arena(12);
        let hitch = fx(1.85);
        let mut body = v(0.0, 0.0);
        let mut trailer = v(0.0, -1.85);
        let route = articulated_path(&terrain, body, v(0.0, -6.0), Fx::ZERO, trailer, hitch, fx(0.75), fx(0.7));
        assert!(route.len() > 10, "reversal must include a turn");
        assert!(route[0].y > Fx::ZERO, "start forward, never through the cart");
        for next in route {
            let d = distance(body, next);
            let steps = (d / fx(0.02)).ceil().floor_to_int().max(1);
            let start = body;
            for n in 1..=steps {
                body = start + (next - start) * Fx::from_ratio(n, steps);
                let d = distance(body, trailer);
                if d > hitch {
                    trailer = body + (trailer - body) * (hitch / d);
                }
                assert!(distance(body, trailer) >= fx(1.45), "animal clipped its cart");
            }
        }
        assert_eq!(body, v(0.0, -6.0));
    }

    #[test]
    fn angles_wrap() {
        assert!((angle_between(fx(3.0), fx(-3.0)).to_f64() - (std::f64::consts::TAU - 6.0)).abs() < 1e-6);
        assert!((bearing(v(0.0, 0.0), v(1.0, 0.0)).to_f64() - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    }
}

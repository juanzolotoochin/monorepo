//! Small, deterministic A* navigation with clearance for the player's body.
pub type Point = [f32; 2];
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct V2(f32, f32);
impl V2 {
    fn length_squared(self) -> f32 {
        self.0 * self.0 + self.1 * self.1
    }
    fn length(self) -> f32 {
        self.length_squared().sqrt()
    }
    fn distance_squared(self, b: Self) -> f32 {
        (self - b).length_squared()
    }
    fn dot(self, b: Self) -> f32 {
        self.0 * b.0 + self.1 * b.1
    }
}
impl std::ops::Sub for V2 {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self(self.0 - b.0, self.1 - b.1)
    }
}
impl std::ops::Add for V2 {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self(self.0 + b.0, self.1 + b.1)
    }
}
impl std::ops::Mul<f32> for V2 {
    type Output = Self;
    fn mul(self, b: f32) -> Self {
        Self(self.0 * b, self.1 * b)
    }
}
fn flat(v: V2) -> V2 {
    v
}
use std::{cmp::Reverse, collections::BinaryHeap};

pub const EDGE: f32 = 8.0;
pub const RADIUS: f32 = 0.32;
const SPACING: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
pub struct Disc {
    pub center: Point,
    pub radius: f32,
}

fn segment_distance(point: V2, a: V2, b: V2) -> f32 {
    let ab = flat(b - a);
    let t = (flat(point - a).dot(ab) / ab.length_squared().max(1e-8)).clamp(0.0, 1.0);
    flat(point - a - ab * t).length()
}

fn clear_inner(a: V2, b: V2, obstacles: &[Disc], radius: f32, edge: f32) -> bool {
    [a, b]
        .iter()
        .all(|p| p.0.abs() <= edge && p.1.abs() <= edge)
        && obstacles.iter().all(|o| {
            segment_distance(V2(o.center[0], o.center[1]), a, b) > o.radius + radius + 0.015
        })
}

fn point(index: usize, side: usize, edge: f32) -> V2 {
    V2(
        (index % side) as f32 * SPACING - edge,
        (index / side) as f32 * SPACING - edge,
    )
}

fn nearest_visible(
    p: V2,
    obstacles: &[Disc],
    valid: &impl Fn(Point, Point) -> bool,
    radius: f32,
    side: usize,
    edge: f32,
) -> Option<usize> {
    (0..side * side)
        .filter(|&i| {
            clear_inner(p, point(i, side, edge), obstacles, radius, edge)
                && valid([p.0, p.1], [point(i, side, edge).0, point(i, side, edge).1])
        })
        .min_by(|&a, &b| {
            point(a, side, edge)
                .distance_squared(p)
                .total_cmp(&point(b, side, edge).distance_squared(p))
        })
}

pub fn clear(a: Point, b: Point, obstacles: &[Disc], radius: f32) -> bool {
    clear_inner(V2(a[0], a[1]), V2(b[0], b[1]), obstacles, radius, EDGE)
}
pub fn path(start: Point, goal: Point, obstacles: &[Disc], radius: f32) -> Vec<Point> {
    path_with(start, goal, obstacles, radius, |_, _| true)
}
pub fn path_with(
    start: Point,
    goal: Point,
    obstacles: &[Disc],
    radius: f32,
    valid: impl Fn(Point, Point) -> bool,
) -> Vec<Point> {
    path_with_bounds(start, goal, obstacles, radius, EDGE, valid)
}
pub fn path_with_bounds(
    start: Point,
    goal: Point,
    obstacles: &[Disc],
    radius: f32,
    edge: f32,
    valid: impl Fn(Point, Point) -> bool,
) -> Vec<Point> {
    if !edge.is_finite() || !(1.0..=24.0).contains(&edge) {
        return vec![];
    }
    let side = (edge * 2. / SPACING).round() as usize + 1;
    let clear = |a: V2, b: V2, obs: &[Disc]| {
        clear_inner(a, b, obs, radius, edge) && valid([a.0, a.1], [b.0, b.1])
    };
    let start = V2(start[0], start[1]);
    let goal = V2(goal[0], goal[1]);
    let start = flat(start);
    let goal = flat(goal);
    if !clear(goal, goal, obstacles) {
        return vec![];
    }
    if clear(start, goal, obstacles) {
        return vec![[goal.0, goal.1]];
    }
    let (Some(first), Some(last)) = (
        nearest_visible(start, obstacles, &valid, radius, side, edge),
        nearest_visible(goal, obstacles, &valid, radius, side, edge),
    ) else {
        return vec![];
    };
    let mut costs = vec![u32::MAX; side * side];
    let mut parents = vec![usize::MAX; side * side];
    let heuristic = |i: usize| -> u32 {
        let dx = (i % side).abs_diff(last % side) as u32;
        let dz = (i / side).abs_diff(last / side) as u32;
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
        let (x, z) = ((current % side) as i32, (current / side) as i32);
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let (nx, nz) = (x + dx, z + dz);
                if !(0..side as i32).contains(&nx) || !(0..side as i32).contains(&nz) {
                    continue;
                }
                let next = nz as usize * side + nx as usize;
                let new_cost = cost + if dx != 0 && dz != 0 { 14 } else { 10 };
                if new_cost < costs[next]
                    && clear(
                        point(current, side, edge),
                        point(next, side, edge),
                        obstacles,
                    )
                {
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
        raw.push(point(node, side, edge));
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
        result.push([from.0, from.1]);
        i = next + 1;
    }
    result
}

pub fn distance(a: Point, b: Point) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

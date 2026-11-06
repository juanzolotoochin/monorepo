//! Clearance-aware A* on a half-tile grid, ported from the original game's
//! navigation in fixed point. Paths are string-pulled into straight segments.
//! Every query is a pure function of the terrain, so the read phase can plan
//! in parallel. On big arenas (mines), a query searches a window around its
//! start and goal (and a mine's hill, with its ramps) instead of the whole
//! arena.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use fx::{Fx, FxVec2};

use crate::terrain::{Disc, Terrain};

const SPACING: Fx = Fx::HALF;
/// Arenas up to this half-width search all of themselves.
const WHOLE_ARENA: Fx = Fx::from_bits(32 << 32);
/// Bigger ones search this far around the start and goal.
const WINDOW_MARGIN: Fx = Fx::from_bits(16 << 32);
/// How far past a hill's sides and ramps the window reaches, in tiles.
const HILL_MARGIN: i32 = 4;
/// Nodes this close to a point are tried first for its nearest visible node.
const NEAR_NODES: i64 = 4;

struct Grid<'a> {
    terrain: &'a Terrain,
    extra: &'a [Disc],
    radius: Fx,
    width: usize,
    height: usize,
    origin: FxVec2,
    valid: &'a dyn Fn(FxVec2, FxVec2) -> bool,
}

impl Grid<'_> {
    /// The searched part of the arena's half-tile lattice.
    fn new<'a>(
        terrain: &'a Terrain,
        start: FxVec2,
        goal: FxVec2,
        radius: Fx,
        extra: &'a [Disc],
        valid: &'a dyn Fn(FxVec2, FxVec2) -> bool,
    ) -> Grid<'a> {
        let side = (terrain.extent.mul_int(2) / SPACING).round().floor_to_int() + 1;
        let min = terrain.min();
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, side - 1, side - 1);
        if terrain.extent > WHOLE_ARENA {
            let node = |v: Fx, from: Fx| ((v - from) / SPACING).floor_to_int().clamp(0, side - 1);
            x0 = node(start.x.min(goal.x) - WINDOW_MARGIN, min.x);
            y0 = node(start.y.min(goal.y) - WINDOW_MARGIN, min.y);
            x1 = node(start.x.max(goal.x) + WINDOW_MARGIN, min.x);
            y1 = node(start.y.max(goal.y) + WINDOW_MARGIN, min.y);
            // A hill's only ways up are its ramps, whose feet may lie past
            // the margin (from the plateau to the ground below one of its
            // sides): the window always holds the hill and its ramps' feet.
            if let Some(hill) = terrain.mine.and_then(|m| m.hill) {
                let (half, reach) = (hill.plateau as i32 + HILL_MARGIN, hill.plateau as i32 + hill.ramp_length as i32 + HILL_MARGIN);
                let corner = |dx: i32, dy: i32| Terrain::pit_center((hill.center.0 + dx, hill.center.1 + dy));
                let (low, high) = (corner(-reach, -half), corner(reach, half));
                x0 = x0.min(node(low.x, min.x));
                y0 = y0.min(node(low.y, min.y));
                x1 = x1.max(node(high.x, min.x));
                y1 = y1.max(node(high.y, min.y));
            }
        }
        Grid {
            terrain,
            extra,
            radius,
            width: (x1 - x0 + 1) as usize,
            height: (y1 - y0 + 1) as usize,
            origin: FxVec2::new(min.x + SPACING.mul_int(x0), min.y + SPACING.mul_int(y0)),
            valid,
        }
    }

    fn point(&self, index: usize) -> FxVec2 {
        FxVec2::new(
            self.origin.x + SPACING.mul_int((index % self.width) as i64),
            self.origin.y + SPACING.mul_int((index / self.width) as i64),
        )
    }

    fn clear(&self, a: FxVec2, b: FxVec2) -> bool {
        self.terrain.clear_with(a, b, self.radius, self.extra) && (self.valid)(a, b)
    }

    /// The grid node nearest to `p` that `p` can see. Nodes close by go
    /// first; the rest only if none of those is closer than they could be.
    fn nearest_visible(&self, p: FxVec2) -> Option<usize> {
        let best = |nodes: &mut dyn Iterator<Item = usize>| {
            nodes.filter(|&i| self.clear(p, self.point(i))).min_by_key(|&i| ((self.point(i) - p).length_squared(), i))
        };
        let cx = ((p.x - self.origin.x) / SPACING).round().floor_to_int();
        let cy = ((p.y - self.origin.y) / SPACING).round().floor_to_int();
        let (w, h) = (self.width as i64, self.height as i64);
        let near = (cy - NEAR_NODES).max(0)..=(cy + NEAR_NODES).min(h - 1);
        let mut nodes = near.flat_map(|y| ((cx - NEAR_NODES).max(0)..=(cx + NEAR_NODES).min(w - 1)).map(move |x| (y * w + x) as usize));
        let reach = SPACING.mul_int(NEAR_NODES);
        match best(&mut nodes) {
            // Every node outside the square is at least `reach` away.
            Some(i) if (self.point(i) - p).length_squared() < reach * reach => Some(i),
            _ => best(&mut (0..self.width * self.height)),
        }
    }
}

/// A path from `start` to `goal` for a body of `radius`, as waypoints ending
/// at `goal`, or empty if there is none.
pub fn path(terrain: &Terrain, start: FxVec2, goal: FxVec2, radius: Fx) -> Vec<FxVec2> {
    path_avoiding(terrain, start, goal, radius, &[])
}

/// [`path`], also avoiding `extra` discs (other bodies). In sloped scenes it
/// keeps to walkable ground ([`Terrain::walkable`]), so routes go around
/// pits and steep slopes and walk in by their ramps.
pub fn path_avoiding(terrain: &Terrain, start: FxVec2, goal: FxVec2, radius: Fx, extra: &[Disc]) -> Vec<FxVec2> {
    path_with(terrain, start, goal, radius, extra, &|a, b| !terrain.sloped || terrain.walkable(a, b, radius))
}

/// [`path_avoiding`], where every straight segment must also pass `valid`
/// (for example, walkable ground without drops).
pub fn path_with(
    terrain: &Terrain,
    start: FxVec2,
    goal: FxVec2,
    radius: Fx,
    extra: &[Disc],
    valid: &dyn Fn(FxVec2, FxVec2) -> bool,
) -> Vec<FxVec2> {
    let grid = Grid::new(terrain, start, goal, radius, extra, valid);
    match search(&grid, start, goal, false) {
        Some((route, true)) => route,
        _ => Vec::new(),
    }
}

/// [`path`], or, when no route reaches `goal`, a route to the reachable
/// point nearest it: the searched window's node, of those the search
/// reached from `start`, closest to `goal` (the lowest index on a tie). The
/// flag says whether the route reaches `goal`. Empty when `start` sees no
/// node. A wave cut off from the tree (a moat round a ramp's foot) walks
/// as close as it can and waits there (`Threat::stranded`).
pub fn path_toward(terrain: &Terrain, start: FxVec2, goal: FxVec2, radius: Fx) -> (Vec<FxVec2>, bool) {
    let valid = |a, b| !terrain.sloped || terrain.walkable(a, b, radius);
    let grid = Grid::new(terrain, start, goal, radius, &[], &valid);
    search(&grid, start, goal, true).unwrap_or_default()
}

/// How long a string-pulled segment of a 3D path may get
/// ([`path_at`]): pulling tries every node of a segment, so a cap keeps it
/// cheap on long straight runs.
const MAX_SEGMENT: i64 = 12;

/// The nodes of the 3D lattice (V8 of the verticality design), made as the
/// search reaches their lattice point: one per floor a body of `radius`
/// fits on there, at the height its feet would be ([`Terrain::footprint_at`]
/// from the floor's height).
struct Floors {
    /// By lattice point: the first node and how many.
    cells: Vec<Option<(u32, u8)>>,
    /// Every node made: its lattice point and feet height.
    nodes: Vec<(usize, Fx)>,
}

impl Floors {
    fn of(&mut self, grid: &Grid, cell: usize) -> std::ops::Range<usize> {
        if let Some((first, count)) = self.cells[cell] {
            return first as usize..first as usize + count as usize;
        }
        let p = grid.point(cell);
        let volume = grid.terrain.volume.as_ref().expect("3D paths need a volume");
        let mut heights: Vec<Fx> = if grid.terrain.inside(p) {
            volume.floors(Terrain::pit_tile(p)).filter_map(|f| grid.terrain.footprint_at(p, crate::volume::height(f), grid.radius)).collect()
        } else {
            Vec::new()
        };
        heights.sort();
        heights.dedup();
        let first = self.nodes.len() as u32;
        self.nodes.extend(heights.iter().map(|&h| (cell, h)));
        self.cells[cell] = Some((first, heights.len() as u8));
        first as usize..first as usize + heights.len()
    }

    /// The node at `cell` whose feet are within a layer of `z`, if any:
    /// floors in a column are further apart than that.
    fn at(&mut self, grid: &Grid, cell: usize, z: Fx) -> Option<usize> {
        self.of(grid, cell).find(|&n| (self.nodes[n].1 - z).abs() <= crate::terrain::LAYER)
    }
}

/// Where a walk from `a`, feet at `z`, straight to `b` puts the feet
/// ([`Terrain::walk_at`]), if it passes the grid's obstacles too.
fn walk(grid: &Grid, a: FxVec2, z: Fx, b: FxVec2) -> Option<Fx> {
    if !grid.terrain.clear_with(a, b, grid.radius, grid.extra) {
        return None;
    }
    grid.terrain.walk_at(a, b, z, grid.radius)
}

/// How many nodes a 3D plan expands at most, its search and its goal's
/// flood together ([`path_at`], [`path_toward_at`]; M8.1 of the depths
/// design). The longest routes across the Tunnel Lab (round the mountain
/// to the bridge's stairs and up) fit in it; a plan to a goal it can't
/// reach that the goal's flood doesn't settle (the goal's floor is big)
/// stops here instead of searching every floor of the window, which once
/// took 1.2 s.
pub const PATH_BUDGET: u32 = 16384;

/// How many nodes the goal's flood takes at most. A 3D plan floods back
/// from the goal (the nodes that walk to it) alongside its search, a node
/// for a node from the search's `FLOOD_AFTER`th on. When the flood runs out
/// of nodes before it meets the search, the goal is shut in (a pit, a
/// mountain's top, a sealed room) and can't be reached: settled for about
/// twice the floor's size. Past this the floor is big, and the search goes
/// on alone.
pub const GOAL_FLOOD: u32 = 1024;

/// The goal's flood starts once the search expanded this many nodes: most
/// plans are found before, and pay nothing for it.
const FLOOD_AFTER: u32 = 256;

/// A partial plan to a goal shut in stops once this many nodes in a row
/// came no nearer it.
pub const STALL: u32 = 256;

/// A 3D plan ([`plan_at`]): the route, as waypoints with the feet's
/// height at each; whether it reaches the goal; and how many nodes the
/// search and the goal's flood expanded (at most the budget).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathAt {
    pub route: Vec<(FxVec2, Fx)>,
    /// The route ends at the goal, on the wanted floor. Without it the
    /// route, if any, goes to the reachable node nearest the goal.
    pub reached: bool,
    pub expanded: u32,
}

/// A path on foot through a volume scene (V8 of the verticality design)
/// from `start`, its feet at `z`, to `goal`: on the floor there nearest
/// `goal_z` (within a layer of its [`Terrain::footprint_near`]), or on
/// any floor without one. Its waypoints come with the feet's height at
/// each, the last at `goal`; empty if there is no way, or none within
/// [`PATH_BUDGET`] nodes.
///
/// The nodes are the half-tile lattice's points, one per floor there
/// (`Floors`); an edge joins neighbours where a body walks from one to the
/// other ([`Terrain::walk_at`]: a step of at most a layer up or down, with
/// headroom all the way, no drops). A* over them breaks ties by node, so
/// the same ground gives the same path. String pulling joins nodes a
/// straight walk joins, up to `MAX_SEGMENT` nodes a segment. Alongside, a
/// flood from the goal ([`GOAL_FLOOD`]) finds a shut-in goal early.
/// Without a volume, [`path_avoiding`] with each waypoint's ground.
pub fn path_at(terrain: &Terrain, start: FxVec2, z: Fx, goal: FxVec2, goal_z: Option<Fx>, radius: Fx, extra: &[Disc]) -> Vec<(FxVec2, Fx)> {
    plan_at(terrain, start, z, goal, goal_z, radius, extra, PATH_BUDGET, false).route
}

/// [`path_at`], or, when it finds no way to `goal` (there is none, or none
/// within [`PATH_BUDGET`] nodes), a route as close to it as the search got:
/// to the node, of those it reached from `start`, nearest `goal` (in 3D,
/// to the wanted floor's height when there is one; the lowest node on a
/// tie). `reached` says which. When the goal is shut in, the search stops
/// once [`STALL`] nodes in a row come no nearer. For a body that should
/// still come as close as it can: a ghoul chasing someone up where it
/// can't climb waits below, a walker whose player stands in a pit it can't
/// enter waits at its edge. The route is empty only when `start` attaches
/// to no node (or, without a volume, there is no full path: partial routes
/// are 3D only).
pub fn path_toward_at(terrain: &Terrain, start: FxVec2, z: Fx, goal: FxVec2, goal_z: Option<Fx>, radius: Fx, extra: &[Disc]) -> PathAt {
    plan_at(terrain, start, z, goal, goal_z, radius, extra, PATH_BUDGET, true)
}

/// The 3D search behind [`path_at`] (`toward` false: a route only if it
/// reaches the goal) and [`path_toward_at`] (`toward`: else the partial
/// route), expanding at most `budget` nodes. Deterministic: the budget
/// counts nodes, not time.
#[allow(clippy::too_many_arguments)]
pub fn plan_at(
    terrain: &Terrain,
    start: FxVec2,
    z: Fx,
    goal: FxVec2,
    goal_z: Option<Fx>,
    radius: Fx,
    extra: &[Disc],
    budget: u32,
    toward: bool,
) -> PathAt {
    use crate::terrain::LAYER;
    if terrain.volume.is_none() {
        let route: Vec<_> = path_avoiding(terrain, start, goal, radius, extra).into_iter().map(|p| (p, terrain.height(p))).collect();
        return PathAt { reached: !route.is_empty(), route, expanded: 0 };
    }
    let any = |_: FxVec2, _: FxVec2| true;
    let grid = Grid::new(terrain, start, goal, radius, extra, &any);
    // The floor wanted at the goal; `open` is false when the goal's column
    // has no floor for the body (nothing arrives, but a partial route
    // still aims at `goal_z`).
    let floor = goal_z.map(|goal_z| terrain.footprint_near(goal, goal_z, radius));
    let open = !matches!(floor, Some(None));
    if !open && !toward {
        return PathAt::default();
    }
    let wanted = floor.zip(goal_z).map(|(floor, goal_z)| floor.unwrap_or(goal_z));
    let arrives = |end: Fx| open && wanted.is_none_or(|w| (end - w).abs() <= LAYER);
    if let Some(end) = walk(&grid, start, z, goal).filter(|&end| arrives(end)) {
        return PathAt { route: vec![(goal, end)], reached: true, expanded: 0 };
    }
    let (width, height) = (grid.width, grid.height);
    let mut floors = Floors { cells: vec![None; width * height], nodes: Vec::new() };
    let lattice = |p: FxVec2| -> (i64, i64) {
        (((p.x - grid.origin.x) / SPACING).round().floor_to_int(), ((p.y - grid.origin.y) / SPACING).round().floor_to_int())
    };
    let near = |c: (i64, i64)| {
        let ys = (c.1 - NEAR_NODES).max(0)..=(c.1 + NEAR_NODES).min(height as i64 - 1);
        ys.flat_map(move |y| ((c.0 - NEAR_NODES).max(0)..=(c.0 + NEAR_NODES).min(width as i64 - 1)).map(move |x| (y * width as i64 + x) as usize))
    };
    // The 8 lattice neighbours of a cell, in order.
    let neighbours = |cell: usize| {
        let (x, y) = ((cell % width) as i64, (cell / width) as i64);
        (-1..=1i64).flat_map(move |dy| (-1..=1i64).map(move |dx| (dx, dy))).filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            let inside = (dx, dy) != (0, 0) && nx >= 0 && ny >= 0 && nx < width as i64 && ny < height as i64;
            inside.then(|| (ny as usize * width + nx as usize, dx != 0 && dy != 0))
        })
    };
    // The start's node: the nearest it walks to (the lowest node on a tie).
    let mut first = None;
    for cell in near(lattice(start)) {
        for n in floors.of(&grid, cell) {
            let (at, h) = (grid.point(cell), floors.nodes[n].1);
            let reached = walk(&grid, start, z, at).is_some_and(|end| (end - h).abs() <= LAYER);
            if reached {
                let key = ((at - start).length_squared(), n);
                if first.is_none_or(|(best, _)| key < best) {
                    first = Some((key, n));
                }
            }
        }
    }
    let Some((_, first)) = first else { return PathAt::default() };
    // The goal's nodes: those near it that walk onto the wanted floor there.
    let mut last: Vec<(usize, Fx)> = Vec::new();
    if open {
        for cell in near(lattice(goal)) {
            for n in floors.of(&grid, cell) {
                if let Some(end) = walk(&grid, grid.point(cell), floors.nodes[n].1, goal).filter(|&end| arrives(end)) {
                    last.push((n, end));
                }
            }
        }
    }
    if last.is_empty() && !toward {
        return PathAt::default();
    }
    let aim = lattice(goal);
    let heuristic = |cell: usize| -> u32 {
        let dx = ((cell % width) as i64).abs_diff(aim.0.clamp(0, width as i64 - 1)) as u32;
        let dy = ((cell / width) as i64).abs_diff(aim.1.clamp(0, height as i64 - 1)) as u32;
        10 * dx.max(dy) + 4 * dx.min(dy)
    };
    // How far a node is from the goal, for a partial route: in 3D when a
    // floor is wanted; the lower node on a tie.
    let gap = |floors: &Floors, n: usize| {
        let (cell, h) = floors.nodes[n];
        let rise = wanted.map_or(Fx::ZERO, |w| (h - w) * (h - w));
        ((grid.point(cell) - goal).length_squared() + rise, n)
    };
    let mut costs: Vec<u32> = Vec::new();
    let mut parents: Vec<usize> = Vec::new();
    // Nodes the goal's flood reached: they walk to the goal.
    let mut back: Vec<bool> = Vec::new();
    let grow = |costs: &mut Vec<u32>, parents: &mut Vec<usize>, back: &mut Vec<bool>, len: usize| {
        if costs.len() < len {
            costs.resize(len, u32::MAX);
            parents.resize(len, usize::MAX);
            back.resize(len, false);
        }
    };
    grow(&mut costs, &mut parents, &mut back, floors.nodes.len());
    let mut flood = std::collections::VecDeque::new();
    for &(n, _) in &last {
        if !back[n] {
            back[n] = true;
            flood.push_back(n);
        }
    }
    // The flood runs until it meets the search (a way), grows past
    // `GOAL_FLOOD` (the floor is big) or runs out of nodes: then the goal
    // is `shut` in.
    let mut flooding = !last.is_empty();
    let mut shut = last.is_empty();
    let (mut flooded, mut expanded) = (0u32, 0u32);
    costs[first] = 0;
    let mut nearest = gap(&floors, first);
    let mut stalled = 0u32;
    let mut open_set = BinaryHeap::new();
    open_set.push(Reverse((heuristic(floors.nodes[first].0), 0u32, first)));
    let mut end = None;
    loop {
        // A node of the goal's flood: the nodes that walk onto it.
        if flooding && expanded >= FLOOD_AFTER {
            match flood.pop_front() {
                Some(_) if back[first] || flooded >= GOAL_FLOOD || expanded + flooded >= budget => flooding = false,
                Some(v) => {
                    flooded += 1;
                    let (cell, h) = floors.nodes[v];
                    for (from, _) in neighbours(cell) {
                        for u in floors.of(&grid, from) {
                            grow(&mut costs, &mut parents, &mut back, floors.nodes.len());
                            if back[u] {
                                continue;
                            }
                            if walk(&grid, grid.point(from), floors.nodes[u].1, grid.point(cell)).is_some_and(|arrived| (arrived - h).abs() <= LAYER) {
                                back[u] = true;
                                // The search reached it: there is a way.
                                if costs[u] != u32::MAX {
                                    flooding = false;
                                }
                                flood.push_back(u);
                            }
                        }
                    }
                }
                None => {
                    // Every node that walks to the goal is flooded, and the
                    // search reached none of them: no way.
                    (flooding, shut) = (false, true);
                    if !toward {
                        return PathAt { expanded: expanded + flooded, ..PathAt::default() };
                    }
                }
            }
        }
        // A node of the search.
        let Some(Reverse((_, cost, current))) = open_set.pop() else { break };
        if cost != costs[current] {
            continue;
        }
        if let Some(&(n, h)) = last.iter().find(|&&(n, _)| n == current) {
            end = Some((n, h));
            break;
        }
        // Out of budget (the goal may still be reachable), or shut in and
        // coming no nearer it.
        if expanded + flooded >= budget || (shut && stalled >= STALL) {
            break;
        }
        expanded += 1;
        stalled += 1;
        let (cell, z) = floors.nodes[current];
        for (next_cell, diagonal) in neighbours(cell) {
            let new_cost = cost + if diagonal { 14 } else { 10 };
            let Some(arrived) = walk(&grid, grid.point(cell), z, grid.point(next_cell)) else { continue };
            let Some(next) = floors.at(&grid, next_cell, arrived) else { continue };
            grow(&mut costs, &mut parents, &mut back, floors.nodes.len());
            if costs[next] == u32::MAX {
                if back[next] {
                    flooding = false;
                }
                let key = gap(&floors, next);
                if key < nearest {
                    (nearest, stalled) = (key, 0);
                }
            }
            if new_cost < costs[next] {
                costs[next] = new_cost;
                parents[next] = current;
                open_set.push(Reverse((new_cost + heuristic(next_cell), new_cost, next)));
            }
        }
    }
    let expanded = expanded + flooded;
    let (end, mut raw) = match end {
        Some((end, arrival)) => (end, vec![(goal, arrival)]),
        None if toward => (nearest.1, Vec::new()),
        None => return PathAt { expanded, ..PathAt::default() },
    };
    let reached = !raw.is_empty();
    let mut node = end;
    loop {
        let (cell, h) = floors.nodes[node];
        raw.push((grid.point(cell), h));
        if node == first {
            break;
        }
        node = parents[node];
    }
    raw.reverse();
    // String pulling: from each waypoint, the furthest of the next nodes a
    // straight walk reaches on their floor, stopping at the first it
    // doesn't.
    let mut route = Vec::new();
    let (mut from, mut feet) = (start, z);
    let mut i = 0;
    while i < raw.len() {
        let mut next = (i, raw[i].1);
        for j in i..raw.len().min(i + MAX_SEGMENT as usize) {
            match walk(&grid, from, feet, raw[j].0).filter(|&h| (h - raw[j].1).abs() <= LAYER) {
                Some(h) => next = (j, h),
                None if j > i => break,
                None => {}
            }
        }
        from = raw[next.0].0;
        feet = next.1;
        route.push((from, feet));
        i = next.0 + 1;
    }
    PathAt { route, reached, expanded }
}

/// A* over `grid` from `start` to `goal`, string-pulled, and whether it
/// reaches `goal`. With no way there, `None`, or, `toward`, the route to
/// the reached node nearest `goal`.
fn search(grid: &Grid, start: FxVec2, goal: FxVec2, toward: bool) -> Option<(Vec<FxVec2>, bool)> {
    let (width, height) = (grid.width, grid.height);
    let open_goal = grid.clear(goal, goal);
    if !open_goal && !toward {
        return None;
    }
    if open_goal && grid.clear(start, goal) {
        return Some((vec![goal], true));
    }
    let first = grid.nearest_visible(start)?;
    let last = if open_goal { grid.nearest_visible(goal) } else { None };
    if last.is_none() && !toward {
        return None;
    }
    // With no node for the goal, the search aims at the lattice point
    // nearest it.
    let aim = |v: Fx, from: Fx, side: usize| ((v - from) / SPACING).round().floor_to_int().clamp(0, side as i64 - 1) as usize;
    let (aim_x, aim_y) = match last {
        Some(last) => (last % width, last / width),
        None => (aim(goal.x, grid.origin.x, width), aim(goal.y, grid.origin.y, height)),
    };
    let mut costs = vec![u32::MAX; width * height];
    let mut parents = vec![usize::MAX; width * height];
    let heuristic = |i: usize| -> u32 {
        let dx = (i % width).abs_diff(aim_x) as u32;
        let dy = (i / width).abs_diff(aim_y) as u32;
        10 * dx.max(dy) + 4 * dx.min(dy)
    };
    let mut open = BinaryHeap::new();
    costs[first] = 0;
    open.push(Reverse((heuristic(first), 0u32, first)));
    while let Some(Reverse((_, cost, current))) = open.pop() {
        if cost != costs[current] {
            continue;
        }
        if Some(current) == last {
            break;
        }
        let (x, y) = ((current % width) as i64, (current / width) as i64);
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                    continue;
                }
                let next = ny as usize * width + nx as usize;
                let new_cost = cost + if dx != 0 && dy != 0 { 14 } else { 10 };
                if new_cost < costs[next] && grid.clear(grid.point(current), grid.point(next)) {
                    costs[next] = new_cost;
                    parents[next] = current;
                    open.push(Reverse((new_cost + heuristic(next), new_cost, next)));
                }
            }
        }
    }
    let reached = last.filter(|&l| costs[l] != u32::MAX);
    let (end, mut raw) = match reached {
        Some(last) => (last, vec![goal]),
        None if toward => {
            let nearest = (0..width * height).filter(|&i| costs[i] != u32::MAX).min_by_key(|&i| ((grid.point(i) - goal).length_squared(), i));
            (nearest?, Vec::new())
        }
        None => return None,
    };
    let mut node = end;
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
    Some((result, reached.is_some()))
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
    articulated_path_avoiding(terrain, start, goal, heading, trailer, hitch, radius, trailer_radius, &[])
}

/// [`articulated_path`], also avoiding `extra` discs (other bodies).
#[allow(clippy::too_many_arguments)]
pub fn articulated_path_avoiding(
    terrain: &Terrain,
    start: FxVec2,
    goal: FxVec2,
    heading: Fx,
    trailer: FxVec2,
    hitch: Fx,
    radius: Fx,
    trailer_radius: Fx,
    extra: &[Disc],
) -> Vec<FxVec2> {
    let direct = path_avoiding(terrain, start, goal, radius, extra);
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
                if !terrain.clear_with(previous, next, radius, extra)
                    || !terrain.clear_with(cart, next_cart, trailer_radius, extra)
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
            let tail = path_avoiding(terrain, previous, goal, radius, extra);
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

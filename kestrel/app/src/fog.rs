//! The fog of war's grid (M9.1 of the Sacred Tree fog, `SACRED_TREE_FOG.md`):
//! per tile and level of the whole town, whether the team sees it now, has
//! seen it before or never has (F2, F3), for M9.2 to draw and M9.3 to hide
//! things by.
//!
//! View only and pure (F9): it reads the sim's state (bodies, buildings,
//! the volume, the clock and the light grid) and keeps nothing the
//! simulation sees. Nothing is saved: a new world, a loaded one too, starts
//! with the town round the tree seen (`START_RANGE`) and what is in sight.
//! Only the Sacred Tree town has one (F11, [`town`]).
//!
//! **Eyes** (F1, F8): the team's, all of them. Every player's elder (or the
//! building it possesses, whose slot it sits in) sees `ELDER_RANGE`, every
//! sprite `SPRITE_RANGE`, the tree `TREE_RANGE` and every built building
//! `BUILDING_RANGE`, each by line of sight from its head on its floor. By
//! night, and always underground, the elder's and the sprites' ranges are
//! halved (`NIGHT`, F4); the tree's and buildings' are not, and lanterns'
//! light reveals the floors it lights (the sim's light grid, M8.6) at any
//! hour.
//!
//! **Levels** (F3): a tile's top floor, open to the sky, is the surface
//! ([`Level::Surface`]); a floor with ground over it is a level of its own,
//! by its layer ([`Level::Layer`]). Floors in a column are more than two
//! layers apart, so a tunnel never shares a level with the surface or
//! another tunnel. An eye under open sky (`sight::open`) sees the surface:
//! a tile blocks it when its ground is over the eye (`sight::EYE` layers
//! over its floor, F5), so on the hill you see over the town and from the
//! town not past the hill; buildings and pines are no ground and don't
//! block. An eye under a roof sees its own level as the inside view does
//! (`sight::blocks`): the floors within a layer of its own, the walls round
//! them (stored on its own floor's layer), and out of a mouth the surface.
//!
//! **Rays**: `sight.rs`'s grid walk (`sight::reaches`), from the eye tile's
//! middle to the middle and four corners of each tile in range
//! (`sight::TARGET_SAMPLES`); a tile is in sight when any reaches it. The
//! target needn't be clear, so a wall is seen by its face and what is
//! behind it isn't.
//!
//! **Cost**: each eye keeps what it saw and its key (its tile, floor and
//! range, and the terrain's revision); it is looked again only when the key
//! changes, so an eye that doesn't move costs nothing. The light grid's
//! floors are read again only when its revision changes. When anything
//! changed, what is in sight now is rebuilt from the eyes' kept tiles (no
//! rays), and what is in sight is remembered as seen.

use std::collections::BTreeMap;

use sim::volume::Volume;

use crate::sight;

/// How far an elder (or the building it possesses) sees by day, meters.
pub const ELDER_RANGE: f32 = 14.0;
/// How far a sprite sees by day.
pub const SPRITE_RANGE: f32 = 6.0;
/// How far round it the Sacred Tree reveals, day and night: its glow.
pub const TREE_RANGE: f32 = 8.0;
/// How far round it a built building reveals, day and night.
pub const BUILDING_RANGE: f32 = 4.0;
/// The share of their range the elder and sprites keep by night and
/// underground (F4).
pub const NIGHT: f32 = 0.5;
/// How far round the tree a new world starts seen (F9): the clearing and
/// the ring of ballistas, 28 m out, by the tree's line of sight.
pub const START_RANGE: f32 = 30.0;

/// A cell's bit for ever seen...
const SEEN: u8 = 1;
/// ...and for in sight now.
const NOW: u8 = 2;

/// Which level of a tile: its top floor or a floor under ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// The top floor, open to the sky.
    Surface,
    /// A floor with ground over it, by its layer (and the walls round
    /// floors an eye on that layer sees).
    Layer(i32),
}

/// What the team knows of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// Never seen: black.
    Never,
    /// Seen before, out of sight now: remembered.
    Seen,
    /// In sight now.
    Now,
}

impl State {
    fn of(bits: u8) -> State {
        if bits & NOW != 0 {
            State::Now
        } else if bits & SEEN != 0 {
            State::Seen
        } else {
            State::Never
        }
    }

    /// As a texture's byte: 0 never seen, 128 seen, 255 in sight.
    pub fn byte(self) -> u8 {
        match self {
            State::Never => 0,
            State::Seen => 128,
            State::Now => 255,
        }
    }
}

/// Who an eye is, to keep its sight between frames.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EyeId {
    /// An elder or a sprite, by its agent's id.
    Agent(String),
    /// A built building, by its index.
    Building(usize),
    /// The Sacred Tree.
    Tree,
}

/// An eye: where it stands and how far it sees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    /// Its tile (`Terrain::pit_tile`).
    pub tile: (i32, i32),
    /// The floor layer it stands on.
    pub floor: i32,
    /// How far it sees, meters (tiles), between tile middles.
    pub range: f32,
}

/// What an eye saw, and what it was looked for.
#[derive(Clone, Debug)]
struct Seeing {
    eye: Eye,
    /// `Terrain::revision` when looked.
    revision: u64,
    cells: Vec<(Level, u32)>,
}

/// The fog over a square of tiles, per level.
#[derive(Clone, Debug)]
pub struct Fog {
    /// The square's lowest tile...
    pub origin: (i32, i32),
    /// ...and its side in tiles.
    pub size: i32,
    /// Per level, row by row (y, then x): the `SEEN` and `NOW` bits. A
    /// level is made when something on it is first seen.
    levels: BTreeMap<Level, Vec<u8>>,
    eyes: BTreeMap<EyeId, Seeing>,
    /// The light grid's revision and the floors it lights.
    lit: Option<(u64, Vec<(Level, u32)>)>,
    /// The cells in sight now.
    now: Vec<(Level, u32)>,
    /// Goes up by one each time what is in sight is rebuilt: for M9.2 to
    /// upload its textures again.
    pub revision: u64,
    /// The rays the last [`Fog::update`] cast: the cost test's measure.
    pub rays: u64,
    /// The eyes it looked again for.
    pub looked: usize,
}

impl Fog {
    /// A fog over `size` tiles from `origin`, nothing seen.
    pub fn new(origin: (i32, i32), size: i32) -> Fog {
        Fog { origin, size, levels: BTreeMap::new(), eyes: BTreeMap::new(), lit: None, now: Vec::new(), revision: 0, rays: 0, looked: 0 }
    }

    /// `tile`'s index in a level, if it is in the square.
    fn index(&self, tile: (i32, i32)) -> Option<u32> {
        let (x, y) = (tile.0 - self.origin.0, tile.1 - self.origin.1);
        (x >= 0 && y >= 0 && x < self.size && y < self.size).then(|| (y * self.size + x) as u32)
    }

    fn bits(&self, level: Level, tile: (i32, i32)) -> u8 {
        self.index(tile).and_then(|i| self.levels.get(&level).map(|cells| cells[i as usize])).unwrap_or(0)
    }

    /// What the team knows of `tile` on `level`.
    pub fn state(&self, tile: (i32, i32), level: Level) -> State {
        State::of(self.bits(level, tile))
    }

    /// What the team knows of `tile`'s floor within a layer of `layer`
    /// under ground (and the walls an eye on those layers saw): the level
    /// as an elder standing on `layer` sees it.
    #[cfg_attr(not(test), allow(dead_code))] // M9.2 draws by it.
    pub fn near(&self, tile: (i32, i32), layer: i32) -> State {
        State::of((layer - 1..=layer + 1).fold(0, |bits, l| bits | self.bits(Level::Layer(l), tile)))
    }

    /// The surface as an R8 texture's rows ([`State::byte`]).
    #[cfg_attr(not(test), allow(dead_code))] // M9.2 uploads it.
    pub fn surface_bytes(&self) -> Vec<u8> {
        self.bytes_with(|fog, tile| fog.state(tile, Level::Surface))
    }

    /// The level under ground of an elder on `layer` as an R8 texture's
    /// rows ([`Fog::near`]).
    #[cfg_attr(not(test), allow(dead_code))] // M9.2 uploads it.
    pub fn level_bytes(&self, layer: i32) -> Vec<u8> {
        self.bytes_with(|fog, tile| fog.near(tile, layer))
    }

    fn bytes_with(&self, state: impl Fn(&Fog, (i32, i32)) -> State) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((self.size * self.size) as usize);
        for y in 0..self.size {
            for x in 0..self.size {
                bytes.push(state(self, (self.origin.0 + x, self.origin.1 + y)).byte());
            }
        }
        bytes
    }

    /// Ors `bits` into every cell of `cells`; returns the cells it put in
    /// sight that weren't (`NOW`), each once.
    fn mark(&mut self, cells: &[(Level, u32)], bits: u8) -> Vec<(Level, u32)> {
        let n = (self.size * self.size) as usize;
        let mut fresh = Vec::new();
        for &(level, i) in cells {
            let slab = self.levels.entry(level).or_insert_with(|| vec![0; n]);
            let cell = &mut slab[i as usize];
            if bits & NOW != 0 && *cell & NOW == 0 {
                fresh.push((level, i));
            }
            *cell |= bits;
        }
        fresh
    }

    /// The cells `eye` sees over `volume`, in the square.
    fn sees(&self, volume: &Volume, eye: Eye) -> (Vec<(Level, u32)>, u64) {
        let (tiles, rays) = look(volume, eye);
        (tiles.into_iter().filter_map(|(tile, level)| self.index(tile).map(|i| (level, i))).collect(), rays)
    }

    /// Remembers what `eye` sees as seen, not in sight: the town round the
    /// tree a new world starts with (F9).
    pub fn remember(&mut self, volume: &Volume, eye: Eye) {
        let (cells, _) = self.sees(volume, eye);
        self.mark(&cells, SEEN);
        self.revision += 1;
    }

    /// Brings the fog up to `eyes` over `volume` (at terrain revision
    /// `revision`) and the light grid at `light` (its revision; `lit` gives
    /// the floors it lights, by tile and layer, read only when the revision
    /// changed). Each eye is looked again only when it moved a tile or a
    /// floor, its range changed or the ground did. Returns whether what is
    /// in sight was rebuilt.
    pub fn update<L: IntoIterator<Item = ((i32, i32), i32)>>(&mut self, volume: &Volume, revision: u64, eyes: &[(EyeId, Eye)], light: u64, lit: impl FnOnce() -> L) -> bool {
        self.rays = 0;
        self.looked = 0;
        // Eyes gone (a sprite died, a building fell) are dropped.
        let wanted: std::collections::BTreeSet<&EyeId> = eyes.iter().map(|(id, _)| id).collect();
        let before = self.eyes.len();
        self.eyes.retain(|id, _| wanted.contains(id));
        let mut changed = self.eyes.len() != before;
        for (id, eye) in eyes {
            if self.eyes.get(id).is_some_and(|s| s.eye == *eye && s.revision == revision) {
                continue;
            }
            let (cells, rays) = self.sees(volume, *eye);
            self.rays += rays;
            self.looked += 1;
            self.eyes.insert(id.clone(), Seeing { eye: *eye, revision, cells });
            changed = true;
        }
        if self.lit.as_ref().is_none_or(|(r, _)| *r != light) {
            let cells = lit().into_iter().filter_map(|(tile, floor)| self.index(tile).map(|i| (level_of(volume, tile, floor), i))).collect();
            self.lit = Some((light, cells));
            changed = true;
        }
        if !changed {
            return false;
        }
        for (level, i) in std::mem::take(&mut self.now) {
            if let Some(slab) = self.levels.get_mut(&level) {
                slab[i as usize] &= !NOW;
            }
        }
        let seen: Vec<(Level, u32)> = self.eyes.values().flat_map(|s| s.cells.iter().copied()).chain(self.lit.iter().flat_map(|(_, c)| c.iter().copied())).collect();
        self.now = self.mark(&seen, NOW | SEEN);
        self.revision += 1;
        true
    }

    /// Brings the fog up to `world`'s team ([`eyes`]) and light grid.
    pub fn follow(&mut self, world: &sim::World) -> bool {
        let terrain = world.terrain();
        let Some(volume) = &terrain.volume else { return false };
        let eyes = eyes(world);
        let grid = world.settlement().map(|s| &s.light);
        self.update(volume, terrain.revision, &eyes, grid.map_or(0, |g| g.revision), || grid.into_iter().flat_map(|g| g.cells()).filter(|&(_, light)| light > 0).map(|(cell, _)| cell))
    }

    /// The fog for `world`'s town, if it is one ([`town`]): over the whole
    /// arena, with the town round the tree seen (F9).
    pub fn of_town(world: &sim::World) -> Option<Fog> {
        if !town(world) {
            return None;
        }
        let terrain = world.terrain();
        let volume = terrain.volume.as_ref()?;
        let center = sim::terrain::Terrain::pit_tile(terrain.center);
        let extent = terrain.extent.ceil().floor_to_int() as i32;
        let mut fog = Fog::new((center.0 - extent, center.1 - extent), 2 * extent);
        if let Some((_, tree)) = tree(world) {
            fog.remember(volume, Eye { range: START_RANGE, ..tree });
        }
        Some(fog)
    }
}

/// Whether `world` is the Sacred Tree's town: a settlement with buildings
/// to build, on a volume (F11). The Tunnel Lab has no building kinds.
pub fn town(world: &sim::World) -> bool {
    world.terrain().volume.is_some() && world.settlement().is_some_and(|s| !s.building_kinds.is_empty())
}

/// The level `tile`'s floor at layer `floor` is on: the surface when
/// nothing is over it.
fn level_of(volume: &Volume, tile: (i32, i32), floor: i32) -> Level {
    if volume.ceiling(tile, floor).is_none() {
        Level::Surface
    } else {
        Level::Layer(floor)
    }
}

/// What `eye` sees over `volume` (see the module doc), by tile and level,
/// and the rays it cast.
pub fn look(volume: &Volume, eye: Eye) -> (Vec<((i32, i32), Level)>, u64) {
    let surface = sight::open(volume, eye.tile, eye.floor);
    let head = eye.floor + sight::EYE;
    let opaque = |t: (i32, i32)| if surface { volume.top(t).is_some_and(|top| top > head) } else { sight::blocks(volume, t, eye.floor) };
    let clear = |x: i32, y: i32| !opaque((x, y));
    let reach = eye.range.ceil() as i32;
    let r2 = eye.range * eye.range;
    let from = (eye.tile.0 as f32 + 0.5, eye.tile.1 as f32 + 0.5);
    let (mut seen, mut rays) = (Vec::new(), 0);
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            if (dx * dx + dy * dy) as f32 > r2 {
                continue;
            }
            let tile = (eye.tile.0 + dx, eye.tile.1 + dy);
            let mut reached = (dx, dy) == (0, 0);
            for &(tx, ty) in &sight::TARGET_SAMPLES {
                if reached {
                    break;
                }
                rays += 1;
                reached = sight::reaches(from, (tile.0 as f32 + 0.5 + tx, tile.1 as f32 + 0.5 + ty), &clear);
            }
            if !reached {
                continue;
            }
            let level = if surface {
                Level::Surface
            } else {
                match volume.floor_near(tile, eye.floor) {
                    Some(floor) => level_of(volume, tile, floor),
                    // Out of a mouth, open ground off the eye's level...
                    None if sight::open(volume, tile, eye.floor) => Level::Surface,
                    // ...and a wall's face, on the eye's level.
                    None => Level::Layer(eye.floor),
                }
            };
            seen.push((tile, level));
        }
    }
    (seen, rays)
}

/// The tree's eye: on its tile's top floor at the arena's center, where
/// the town's tree stands.
fn tree(world: &sim::World) -> Option<(EyeId, Eye)> {
    let terrain = world.terrain();
    let tile = sim::terrain::Terrain::pit_tile(terrain.center);
    let floor = terrain.volume.as_ref()?.top(tile)?;
    Some((EyeId::Tree, Eye { tile, floor, range: TREE_RANGE }))
}

/// How far an elder or a sprite that sees `day` meters by day sees now: by
/// `NIGHT` of it at night or under a roof, where it is always night (F4).
pub fn range(day: f32, night: bool, roofed: bool) -> f32 {
    if night || roofed {
        day * NIGHT
    } else {
        day
    }
}

/// The team's eyes in `world` (F1, F8): every living elder and sprite, by
/// night or under a roof at `NIGHT` of their range; every built building;
/// the tree. In agent, then building order, the tree last.
pub fn eyes(world: &sim::World) -> Vec<(EyeId, Eye)> {
    let terrain = world.terrain();
    let (Some(volume), Some(settlement)) = (&terrain.volume, world.settlement()) else { return Vec::new() };
    let night = settlement.clock.is_some_and(|c| c.phase(world.tick()) == sim::settlement::clock::Phase::Night);
    let floor_of = |p: fx::FxVec2, z: fx::Fx| volume.floor_at(p, z).map_or_else(|| sim::volume::layer_at(z), |(floor, _)| floor);
    let mut eyes = Vec::new();
    for agent in &settlement.agents {
        let range = match &agent.role {
            sim::settlement::Role::Avatar(_) => ELDER_RANGE,
            sim::settlement::Role::Sprite(_) => SPRITE_RANGE,
            sim::settlement::Role::Hauler(_) => continue,
        };
        if !agent.body.alive() {
            continue;
        }
        let tile = sim::terrain::Terrain::pit_tile(agent.body.position);
        let floor = floor_of(agent.body.position, agent.height());
        eyes.push((EyeId::Agent(agent.id.clone()), Eye { tile, floor, range: self::range(range, night, !sight::open(volume, tile, floor)) }));
    }
    for (i, building) in settlement.buildings.iter().enumerate().filter(|(_, b)| b.built()) {
        let tile = sim::terrain::Terrain::pit_tile(building.position);
        eyes.push((EyeId::Building(i), Eye { tile, floor: floor_of(building.position, building.height), range: BUILDING_RANGE }));
    }
    eyes.extend(tree(world));
    eyes
}

/// The town's fog this frame: none outside the town.
#[derive(bevy::prelude::Resource, Default)]
pub struct FogView {
    pub fog: Option<Fog>,
}

impl FogView {
    /// A new world: its fog starts again (`Fog::of_town`).
    pub fn reset(&mut self) {
        self.fog = None;
    }
}

/// Brings the town's fog up to the world each frame (none elsewhere). It
/// draws nothing yet: M9.2 uploads it.
pub fn follow(service: Option<bevy::prelude::Res<crate::lockstep::SessionService>>, mut view: bevy::prelude::ResMut<FogView>) {
    let Some(world) = service.as_ref().and_then(|service| service.world()) else { return };
    if view.fog.is_none() {
        view.fog = Fog::of_town(world);
    }
    if let Some(fog) = view.fog.as_mut() {
        fog.follow(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::volume::Volume;

    /// Flat ground 40 tiles each way, its floor at layer 0.
    fn flat() -> Volume {
        let mut volume = Volume::default();
        volume.fill_box((-40, -40), (40, 40), -20, 0);
        volume
    }

    fn eye(tile: (i32, i32), floor: i32, range: f32) -> Eye {
        Eye { tile, floor, range }
    }

    /// A fog over the flat ground's square.
    fn fog() -> Fog {
        Fog::new((-40, -40), 81)
    }

    /// Brings `fog` up to `eyes` with no light.
    fn update(fog: &mut Fog, volume: &Volume, eyes: &[(EyeId, Eye)]) -> bool {
        fog.update(volume, 0, eyes, 0, Vec::new)
    }

    fn agent(id: &str) -> EyeId {
        EyeId::Agent(id.into())
    }

    fn surface(fog: &Fog, tile: (i32, i32)) -> State {
        fog.state(tile, Level::Surface)
    }

    /// Rows of a level's fog as text, north up, `step` tiles a character:
    /// ` ` never seen, `.` seen, `#` in sight. For failure messages and the
    /// town's picture.
    fn picture(fog: &Fog, level: Level, from: (i32, i32), to: (i32, i32), step: i32) -> String {
        let mut rows = Vec::new();
        let mut y = to.1;
        while y >= from.1 {
            let mut row = String::new();
            let mut x = from.0;
            while x <= to.0 {
                row.push(match fog.state((x, y), level) {
                    State::Never => ' ',
                    State::Seen => '.',
                    State::Now => '#',
                });
                x += step;
            }
            rows.push(row);
            y -= step;
        }
        rows.join("\n")
    }

    #[test]
    fn an_eye_sees_its_range_on_flat_ground() {
        let volume = flat();
        let mut fog = fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((0, 0), 0, ELDER_RANGE))]);
        let shown = picture(&fog, Level::Surface, (-16, -16), (16, 16), 1);
        assert_eq!(surface(&fog, (0, 0)), State::Now, "its own tile");
        assert_eq!(surface(&fog, (14, 0)), State::Now, "14 m out:\n{shown}");
        assert_eq!(surface(&fog, (0, -14)), State::Now);
        assert_eq!(surface(&fog, (9, 10)), State::Now, "13.5 m out");
        assert_eq!(surface(&fog, (15, 0)), State::Never, "15 m out:\n{shown}");
        assert_eq!(surface(&fog, (10, 10)), State::Never, "14.1 m out");
        // A sprite sees 6 m, a building 4 m.
        let mut fog = self::fog();
        update(&mut fog, &volume, &[(agent("ash"), eye((0, 0), 0, SPRITE_RANGE)), (EyeId::Building(0), eye((20, 0), 0, BUILDING_RANGE))]);
        assert_eq!(surface(&fog, (6, 0)), State::Now);
        assert_eq!(surface(&fog, (7, 0)), State::Never);
        assert_eq!(surface(&fog, (24, 0)), State::Now);
        assert_eq!(surface(&fog, (25, 0)), State::Never);
        // Nothing under ground is seen from the surface.
        assert_eq!(fog.state((3, 0), Level::Layer(-8)), State::Never);
    }

    #[test]
    fn ground_over_the_eye_blocks_and_the_hill_sees_over_it() {
        let mut volume = flat();
        // A ridge along x = 5, 4 m high; a low wall along x = -5, a step
        // under the head (1.5 m).
        volume.fill_box((5, -20), (5, 20), 0, 8);
        volume.fill_box((-5, -20), (-5, 20), 0, 3);
        let mut fog = fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((0, 0), 0, ELDER_RANGE))]);
        let shown = picture(&fog, Level::Surface, (-16, -8), (16, 8), 1);
        assert_eq!(surface(&fog, (5, 0)), State::Now, "the ridge's face:\n{shown}");
        for x in 6..=12 {
            assert_eq!(surface(&fog, (x, 0)), State::Never, "behind the ridge at x {x}:\n{shown}");
        }
        for x in -12..=-4 {
            assert_eq!(surface(&fog, (x, 0)), State::Now, "past the low wall at x {x}:\n{shown}");
        }
        // On a hill 4 m up, a 3 m ridge is under the eye: the ground past
        // it shows.
        let mut volume = flat();
        volume.fill_box((-2, -2), (2, 2), 0, 8);
        volume.fill_box((5, -20), (5, 20), 0, 6);
        let mut fog = self::fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((0, 0), 8, ELDER_RANGE))]);
        for x in 6..=12 {
            assert_eq!(surface(&fog, (x, 0)), State::Now, "over the ridge at x {x}");
        }
        // From the foot the hill hides what is behind it.
        let mut fog = self::fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((-10, 0), 0, ELDER_RANGE))]);
        assert_eq!(surface(&fog, (-2, 0)), State::Now, "the hill's face");
        assert_eq!(surface(&fog, (3, 0)), State::Never, "behind the hill");
    }

    #[test]
    fn night_and_the_depths_halve_units_sight() {
        assert_eq!(range(ELDER_RANGE, false, false), 14.0);
        assert_eq!(range(ELDER_RANGE, true, false), 7.0);
        assert_eq!(range(SPRITE_RANGE, true, false), 3.0);
        // Underground it is always night.
        assert_eq!(range(ELDER_RANGE, false, true), 7.0);
        assert_eq!(range(ELDER_RANGE, true, true), 7.0);
        let volume = flat();
        let mut fog = fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((0, 0), 0, range(ELDER_RANGE, true, false)))]);
        assert_eq!(surface(&fog, (7, 0)), State::Now);
        assert_eq!(surface(&fog, (8, 0)), State::Never);
        // The lanterns' light has no range to halve: it reveals what its
        // grid lights (`lanterns_reveal_the_floors_they_light`).
    }

    /// Ground 10 m deep, its floor at layer 0, and a tunnel along +x under
    /// it, its floor at layer -8 with headroom, from x 0 to 20 at y 0; a
    /// room at its east end.
    fn tunnel() -> Volume {
        let mut volume = flat();
        volume.clear_box((0, 0), (20, 0), -8, -4);
        volume.clear_box((21, -2), (25, 2), -8, -4);
        volume
    }

    #[test]
    fn under_ground_each_level_is_seen_on_its_own() {
        let volume = tunnel();
        let mut fog = fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((2, 0), -8, range(ELDER_RANGE, false, true)))]);
        let shown = picture(&fog, Level::Layer(-8), (-2, -4), (26, 4), 1);
        // Down the tunnel to its range (7 m), and its walls' faces.
        for x in 2..=9 {
            assert_eq!(fog.state((x, 0), Level::Layer(-8)), State::Now, "the tunnel at x {x}:\n{shown}");
        }
        assert_eq!(fog.state((5, 1), Level::Layer(-8)), State::Now, "its wall:\n{shown}");
        assert_eq!(fog.state((5, 3), Level::Layer(-8)), State::Never, "rock past the wall:\n{shown}");
        assert_eq!(fog.state((12, 0), Level::Layer(-8)), State::Never, "past its range");
        // The surface over it isn't seen from the tunnel; floors a layer
        // off read as the same level.
        assert_eq!(surface(&fog, (5, 0)), State::Never);
        assert_eq!(fog.near((5, 0), -8), State::Now);
        assert_eq!(fog.near((5, 0), -7), State::Now);
        assert_eq!(fog.near((5, 0), -4), State::Never);
        // From the surface over it, the tunnel stays dark.
        let mut fog = self::fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((5, 3), 0, ELDER_RANGE))]);
        assert_eq!(surface(&fog, (5, 0)), State::Now);
        assert_eq!(fog.near((5, 0), -8), State::Never);
    }

    #[test]
    fn out_of_a_mouth_an_eye_sees_the_surface() {
        // The tunnel opens west into a pit dug from the surface: its
        // floor is the tunnel's, open to the sky.
        let mut volume = tunnel();
        volume.clear_box((-6, -3), (-1, 3), -8, 0);
        let mut fog = fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((2, 0), -8, ELDER_RANGE))]);
        assert_eq!(surface(&fog, (-3, 0)), State::Now, "the pit's floor");
        assert_eq!(fog.state((-3, 0), Level::Layer(-8)), State::Never, "it is the surface, not the tunnel's level");
    }

    #[test]
    fn lanterns_reveal_the_floors_they_light() {
        let volume = tunnel();
        let lamp = sim::settlement::light::Lamp { tile: (22, 0), floor: -8, range: fx::Fx::from_int(6) };
        let lit = || sim::settlement::light::compute(&volume, &[lamp]).into_keys().collect::<Vec<_>>();
        let mut fog = fog();
        assert!(fog.update(&volume, 0, &[], 1, lit));
        // The room and the tunnel's end within 6 m, on the room's level.
        assert_eq!(fog.state((22, 0), Level::Layer(-8)), State::Now);
        assert_eq!(fog.state((24, 2), Level::Layer(-8)), State::Now);
        assert_eq!(fog.state((18, 0), Level::Layer(-8)), State::Now);
        assert_eq!(fog.state((15, 0), Level::Layer(-8)), State::Never, "past its range");
        assert_eq!(surface(&fog, (22, 0)), State::Never, "not the surface over it");
        // A lantern on the surface lights the surface.
        let lamp = sim::settlement::light::Lamp { tile: (-20, -20), floor: 0, range: fx::Fx::from_int(6) };
        let mut fog = self::fog();
        fog.update(&volume, 0, &[], 1, || sim::settlement::light::compute(&volume, &[lamp]).into_keys().collect::<Vec<_>>());
        assert_eq!(surface(&fog, (-17, -20)), State::Now);
        // The light isn't read again until its revision changes; then
        // what it no longer lights is remembered.
        assert!(!fog.update(&volume, 0, &[], 1, || -> Vec<((i32, i32), i32)> { panic!("read again") }));
        assert!(fog.update(&volume, 0, &[], 2, Vec::new));
        assert_eq!(surface(&fog, (-17, -20)), State::Seen);
    }

    #[test]
    fn seen_ground_is_remembered() {
        let volume = flat();
        let mut fog = fog();
        update(&mut fog, &volume, &[(agent("elder"), eye((-20, 0), 0, ELDER_RANGE))]);
        update(&mut fog, &volume, &[(agent("elder"), eye((20, 0), 0, ELDER_RANGE))]);
        assert_eq!(surface(&fog, (-20, 0)), State::Seen, "where it was");
        assert_eq!(surface(&fog, (20, 0)), State::Now, "where it is");
        assert_eq!(surface(&fog, (0, 30)), State::Never, "where it never looked");
        // The texture: 0 never, 128 seen, 255 in sight, row by row.
        let bytes = fog.surface_bytes();
        assert_eq!(bytes.len(), 81 * 81);
        let at = |tile: (i32, i32)| bytes[fog.index(tile).unwrap() as usize];
        assert_eq!((at((-20, 0)), at((20, 0)), at((0, 30))), (128, 255, 0));
        assert_eq!(fog.level_bytes(-8).len(), 81 * 81);
        // `remember` gives seen ground, not in sight (a new world's town).
        let mut fresh = self::fog();
        fresh.remember(&volume, eye((0, 0), 0, START_RANGE));
        assert_eq!(surface(&fresh, (25, 0)), State::Seen);
        assert_eq!(surface(&fresh, (0, 0)), State::Seen);
        assert_eq!(surface(&fresh, (35, 0)), State::Never);
    }

    #[test]
    fn the_teams_eyes_add_up() {
        let volume = flat();
        let mut fog = fog();
        let team = [(agent("elder 1"), eye((-20, 0), 0, ELDER_RANGE)), (agent("elder 2"), eye((20, 0), 0, ELDER_RANGE)), (agent("ash"), eye((0, 20), 0, SPRITE_RANGE))];
        update(&mut fog, &volume, &team);
        for tile in [(-20, 0), (-30, 0), (20, 0), (30, 0), (0, 20), (0, 25)] {
            assert_eq!(surface(&fog, tile), State::Now, "{tile:?}");
        }
        assert_eq!(surface(&fog, (0, 0)), State::Never, "between them");
        // An eye gone (the sprite died): its ground is remembered.
        update(&mut fog, &volume, &team[..2]);
        assert_eq!(surface(&fog, (0, 20)), State::Seen);
        assert_eq!(surface(&fog, (-20, 0)), State::Now);
    }

    #[test]
    fn an_eye_is_looked_again_only_when_its_key_changes() {
        let volume = flat();
        let mut fog = fog();
        let mut eyes = vec![(agent("elder"), eye((0, 0), 0, ELDER_RANGE)), (agent("ash"), eye((10, 0), 0, SPRITE_RANGE)), (EyeId::Tree, eye((-10, 0), 0, TREE_RANGE))];
        assert!(update(&mut fog, &volume, &eyes));
        assert_eq!(fog.looked, 3);
        assert!(fog.rays > 0);
        let revision = fog.revision;
        // Nothing moved: no rays, nothing rebuilt.
        assert!(!update(&mut fog, &volume, &eyes));
        assert_eq!((fog.looked, fog.rays, fog.revision), (0, 0, revision));
        // One moved a tile: only it.
        eyes[1].1.tile = (11, 0);
        assert!(update(&mut fog, &volume, &eyes));
        assert_eq!(fog.looked, 1);
        // Its range (dusk), its floor, and the ground: again.
        eyes[0].1.range = ELDER_RANGE * NIGHT;
        update(&mut fog, &volume, &eyes);
        assert_eq!(fog.looked, 1);
        eyes[0].1.floor = 1;
        update(&mut fog, &volume, &eyes);
        assert_eq!(fog.looked, 1);
        fog.update(&volume, 1, &eyes, 0, Vec::new);
        assert_eq!(fog.looked, 3, "the ground changed: every eye");
    }

    /// A scene as the game builds it, its world started.
    fn scene_app(id: &str) -> bevy::prelude::App {
        let scenes = crate::assets::SceneLibrary::new();
        let scene = scenes.index(id).unwrap();
        let options = crate::control::Options { simulation: true, headless_menus: true, ..bevy::prelude::default() };
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, scenes, hub, crate::flow::Launch::single_player(scene));
        crate::start_world(&mut app);
        app
    }

    fn world(app: &bevy::prelude::App) -> &sim::World {
        app.world().resource::<crate::lockstep::SessionService>().world().unwrap()
    }

    #[test]
    fn the_town_starts_with_the_tree_seen_and_its_team_in_sight() {
        let app = scene_app("sacred-tree");
        let world = world(&app);
        assert!(town(world));
        let terrain = world.terrain();
        let tree = sim::terrain::Terrain::pit_tile(terrain.center);
        let mut fog = Fog::of_town(world).expect("the town has fog");
        assert_eq!(fog.size, 256, "the whole town");
        // The team: the elder, four sprites, the tree; by day.
        let eyes = eyes(world);
        let ranges: Vec<f32> = eyes.iter().map(|(_, e)| e.range).collect();
        assert_eq!(eyes.iter().filter(|(id, _)| matches!(id, EyeId::Agent(_))).count(), 5, "{eyes:?}");
        assert_eq!(eyes.last().map(|(id, _)| id), Some(&EyeId::Tree));
        assert!(ranges.contains(&ELDER_RANGE) && ranges.contains(&SPRITE_RANGE) && ranges.contains(&TREE_RANGE), "{ranges:?}");
        // Before any eye looks, the clearing is remembered, not in sight.
        let off = |x: i32, y: i32| (tree.0 + x, tree.1 + y);
        assert_eq!(fog.state(off(0, 20), Level::Surface), State::Seen);
        assert_eq!(fog.state(off(0, 60), Level::Surface), State::Never);
        assert!(fog.follow(world));
        assert_eq!(fog.state(tree, Level::Surface), State::Now);
        let elder = world.settlement().unwrap().agents.iter().find(|a| a.avatar().is_some()).unwrap();
        assert_eq!(fog.state(sim::terrain::Terrain::pit_tile(elder.body.position), Level::Surface), State::Now);
        assert_eq!(fog.state(off(0, 20), Level::Surface), State::Seen, "past every eye: remembered");
        assert!(!fog.follow(world), "nothing moved");
        // The app keeps one for the town (`follow`, in rendered runs).
        eprintln!("the town at the start, 2 m a character, north up (# in sight, . seen):\n{}", picture(&fog, Level::Surface, off(-64, -64), off(64, 64), 2));
    }

    #[test]
    fn other_scenes_have_no_fog() {
        let app = scene_app("tunnel-lab");
        assert!(!town(world(&app)));
        assert!(Fog::of_town(world(&app)).is_none());
    }

    /// The town with every eye (8 elders, 16 sprites, 12 buildings, the
    /// tree and 6 lanterns), against a frame budget: the first frame looks
    /// for all of them; a frame where nothing moved casts no ray; a frame
    /// where every elder and sprite moved a tile stays within `BUDGET`.
    #[test]
    fn the_towns_fog_with_every_eye_stays_within_a_frame_budget() {
        /// The fog's share of a frame when every unit moved a tile, in the
        /// test's unoptimized build: a quarter of a 60 Hz frame. Measured
        /// (M9.1): 0.6 ms moved, 0.7 ms every eye looked, 7 us still.
        const BUDGET: std::time::Duration = std::time::Duration::from_millis(4);
        let app = scene_app("sacred-tree");
        let world = world(&app);
        let terrain = world.terrain();
        let volume = terrain.volume.as_ref().unwrap();
        let tree = sim::terrain::Terrain::pit_tile(terrain.center);
        let on = |(x, y): (i32, i32), range: f32| {
            let tile = (tree.0 + x, tree.1 + y);
            eye(tile, volume.top(tile).unwrap(), range)
        };
        let ring = |n: usize, radius: f32, turn: f32| {
            (0..n).map(move |i| {
                let a = (i as f32 + turn) / n as f32 * std::f32::consts::TAU;
                ((radius * a.cos()).round() as i32, (radius * a.sin()).round() as i32)
            })
        };
        let mut eyes: Vec<(EyeId, Eye)> = Vec::new();
        eyes.extend(ring(8, 30.0, 0.0).enumerate().map(|(i, t)| (agent(&format!("elder {i}")), on(t, ELDER_RANGE))));
        eyes.extend(ring(16, 16.0, 0.5).enumerate().map(|(i, t)| (agent(&format!("sprite {i}")), on(t, SPRITE_RANGE))));
        eyes.extend(ring(12, 24.0, 0.25).enumerate().map(|(i, t)| (EyeId::Building(i), on(t, BUILDING_RANGE))));
        eyes.push((EyeId::Tree, on((0, 0), TREE_RANGE)));
        let lamps: Vec<_> = ring(6, 36.0, 0.1)
            .map(|(x, y)| {
                let tile = (tree.0 + x, tree.1 + y);
                sim::settlement::light::Lamp { tile, floor: volume.top(tile).unwrap(), range: fx::Fx::from_int(6) }
            })
            .collect();
        let lit = sim::settlement::light::compute(volume, &lamps).into_keys().collect::<Vec<_>>();
        let mut fog = Fog::of_town(world).unwrap();
        let started = std::time::Instant::now();
        fog.update(volume, terrain.revision, &eyes, 1, || lit.clone());
        let (first, first_rays) = (started.elapsed(), fog.rays);
        assert_eq!(fog.looked, eyes.len());
        // Nothing moved.
        let started = std::time::Instant::now();
        assert!(!fog.update(volume, terrain.revision, &eyes, 1, || lit.clone()));
        let still = started.elapsed();
        assert_eq!((fog.looked, fog.rays), (0, 0), "an eye that doesn't move costs nothing");
        // Every elder and sprite moved a tile.
        for (id, eye) in eyes.iter_mut() {
            if matches!(id, EyeId::Agent(_)) {
                let tile = (eye.tile.0 + 1, eye.tile.1);
                *eye = Eye { tile, floor: volume.top(tile).unwrap(), ..*eye };
            }
        }
        let started = std::time::Instant::now();
        fog.update(volume, terrain.revision, &eyes, 1, || lit.clone());
        let moved = started.elapsed();
        assert_eq!(fog.looked, 24);
        eprintln!(
            "the town's fog, {} eyes and {} lit floors: all looked {first:?} ({first_rays} rays), nothing moved {still:?}, every unit moved {moved:?} ({} rays); {} tiles in sight",
            eyes.len(),
            lit.len(),
            fog.rays,
            fog.now.len()
        );
        eprintln!("every eye, 2 m a character, north up:\n{}", picture(&fog, Level::Surface, (tree.0 - 64, tree.1 - 64), (tree.0 + 64, tree.1 + 64), 2));
        assert!(moved < BUDGET, "every unit moved: {moved:?} over {BUDGET:?}");
        assert!(first < BUDGET * 2, "every eye looked: {first:?}");
    }
}

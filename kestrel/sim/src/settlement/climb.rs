//! Climbing walls with a stamina wheel (`kestrel/CLIMBING.md`, K2–K6 and
//! K11): a character with a `climb` section (the elder) grabs any face of
//! a volume's rock at least two layers (1 m) tall by pushing into it, moves
//! up, down and along it with the stick, wraps outside corners, turns at
//! inside ones, pulls over the top, and lets go when told to or when the
//! wheel runs out. It is part of the avatar's movement (stage 2,
//! `avatar::self_update`), so prediction replays it as it does walking.
//!
//! On a face the stick reads relative to the face, from the movement
//! direction the sim already gets (camera-independent, in world axes):
//! its component into the rock climbs up, out of it climbs down, and along
//! the face moves sideways.
//!
//! A face is a side of a solid tile (`Climb::tile`, `Face`). The climber
//! hangs with its body [`GAP`] off the face, its feet at
//! `AvatarState::height`, in the column in front of it.

use content::character::{CharacterDef, ClimbDef};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::{AvatarState, Intent, JumpPhase};
use super::Body;
use crate::nav;
use crate::terrain::{Terrain, LAYER};
use crate::volume::{height, layer_at, Volume, BODY_HEIGHT, ROUNDING};

/// Between the body and the face it holds, in meters.
pub const GAP: Fx = Fx::from_ratio(1, 50);
/// How far past the body a face may be to grab it: walking into a wall
/// stops the body within a tick's step of it.
pub const REACH: Fx = Fx::from_ratio(3, 20);
/// The share of the stick into a face that counts as pushing into it: 0.5,
/// within 60 degrees of straight in, so walking along a cliff doesn't.
pub const PUSH: Fx = Fx::HALF;
/// The hands over the feet: a climber holds the rock between the two.
pub const HANDS: Fx = Fx::from_ratio(3, 2);
/// Feet this far under a face's top pull over it (the hands are well over).
pub const MANTLE_BELOW: Fx = Fx::ONE;
/// After pulling over, the body's edge is this far in from the face's edge.
pub const MANTLE_IN: Fx = Fx::from_ratio(1, 20);

/// Which side of its tile a face is: the way it faces, out of the rock.
/// East is +x and south +y (scene z), as tiles are numbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Face {
    East,
    West,
    South,
    North,
}

impl Face {
    /// Out of the rock, a unit vector.
    pub fn normal(self) -> FxVec2 {
        let (x, y) = self.step();
        FxVec2::new(Fx::from_int(x), Fx::from_int(y))
    }

    /// Along the face: the normal turned a quarter, `(-n.y, n.x)`. A
    /// positive sideways move goes this way.
    pub fn tangent(self) -> FxVec2 {
        let n = self.normal();
        FxVec2::new(-n.y, n.x)
    }

    /// The tile out of the rock from the face's tile.
    pub fn step(self) -> (i32, i32) {
        match self {
            Face::East => (1, 0),
            Face::West => (-1, 0),
            Face::South => (0, 1),
            Face::North => (0, -1),
        }
    }

    /// The face whose normal is `v`, an axis's unit vector (either sign).
    fn of(v: FxVec2) -> Face {
        if v.x.abs() >= v.y.abs() {
            if v.x > Fx::ZERO { Face::East } else { Face::West }
        } else if v.y > Fx::ZERO {
            Face::South
        } else {
            Face::North
        }
    }
}

/// A climber on a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Climb {
    /// The solid tile whose face it holds.
    pub tile: (i32, i32),
    /// Which side of `tile`.
    pub face: Face,
    pub phase: ClimbPhase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClimbPhase {
    /// Hanging on, moved by the stick.
    #[default]
    Hold,
    /// A lunge on the face, or a jump's grab from the ground: `up` and
    /// `side` meters a tick along the face, until `until`.
    Lunge { started: u64, until: u64, up: Fx, side: Fx },
    /// Pulling over the top, from `from` (feet at `low`) to standing at
    /// `to` (feet at `top`), on `until`.
    Mantle { started: u64, until: u64, from: FxVec2, low: Fx, to: FxVec2, top: Fx },
}

/// An avatar's climbing (`AvatarState::climbing`): on a face or not, the
/// stamina wheel, and what keeps it from grabbing. The default is off any
/// face with a full wheel. Characters without a `climb` section keep it so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Climbing {
    pub climb: Option<Climb>,
    /// How much of the wheel is spent: 0 full, 1 empty.
    pub spent: Fx,
    /// The wheel ran out: no grabbing until it is full again, which only
    /// standing on a floor refills (K6).
    pub exhausted: bool,
    /// Let go of a face: no grabbing until the feet are on a floor again.
    pub released: bool,
    /// Ticks in a row the stick has pushed into a face, toward a grab.
    pub pushing: u64,
}

impl Climbing {
    /// What is left of the wheel: 1 full, 0 empty.
    pub fn stamina(&self) -> Fx {
        Fx::ONE - self.spent
    }
}

/// The five points a body's columns are read at, as `Terrain::footprint`.
fn points(p: FxVec2, radius: Fx) -> [FxVec2; 5] {
    [(Fx::ZERO, Fx::ZERO), (radius, Fx::ZERO), (-radius, Fx::ZERO), (Fx::ZERO, radius), (Fx::ZERO, -radius)].map(|(dx, dy)| FxVec2::new(p.x + dx, p.y + dy))
}

fn center(tile: (i32, i32)) -> FxVec2 {
    Terrain::pit_center(tile)
}

/// The face's plane, as a coordinate along its normal.
pub(super) fn plane(tile: (i32, i32), face: Face) -> Fx {
    center(tile).dot(face.normal()) + Fx::HALF
}

/// The layers a body with its feet at `feet` fills.
fn body_layers(feet: Fx) -> (i32, i32) {
    (layer_at(feet + ROUNDING), layer_at(feet + BODY_HEIGHT - ROUNDING))
}

/// Whether `tile` has no rock where a body with its feet at `feet` is.
fn clear(volume: &Volume, tile: (i32, i32), feet: Fx) -> bool {
    let (lo, hi) = body_layers(feet);
    volume.spans(tile).iter().all(|s| s.top <= lo || s.bottom > hi)
}

/// Whether a body of `radius` at `p` with its feet at `feet` is in no rock.
pub(super) fn body_clear(volume: &Volume, p: FxVec2, feet: Fx, radius: Fx) -> bool {
    points(p, radius).iter().all(|&q| clear(volume, Terrain::pit_tile(q), feet))
}

/// The top (as a layer) of the rock of `tile` a climber with its feet at
/// `feet` holds: the highest span reaching over the feet that starts at or
/// under the hands. Nothing: no rock to hold there.
pub(super) fn hold(volume: &Volume, tile: (i32, i32), feet: Fx) -> Option<i32> {
    let (lo, hands) = (layer_at(feet + ROUNDING), layer_at(feet + HANDS));
    volume.spans(tile).iter().rev().find(|s| s.bottom <= hands && s.top > lo).map(|s| s.top)
}

/// Whether `tile` is a face to grab for feet at `feet`: rock in the feet's
/// layer and the one over it, so at least 1 m of it over the feet (K2). A
/// one-layer step is walked up instead.
fn grabbable(volume: &Volume, tile: (i32, i32), feet: Fx) -> bool {
    let lo = layer_at(feet + ROUNDING);
    volume.is_solid(tile, lo) && volume.is_solid(tile, lo + 1)
}

/// The face a body of `radius` at `p` (feet at `feet`) pushes into going
/// `direction`: the tile next to its own on the axis `direction` leans
/// into most, at least [`PUSH`] of it, within [`REACH`] of the body and
/// [`grabbable`], with room in front of it to climb.
fn face_ahead(volume: &Volume, p: FxVec2, feet: Fx, radius: Fx, direction: FxVec2) -> Option<((i32, i32), Face)> {
    let here = Terrain::pit_tile(p);
    if !body_clear(volume, p, feet + LAYER, radius) {
        return None;
    }
    let axes = [(direction.x, FxVec2::new(direction.x.signum(), Fx::ZERO)), (direction.y, FxVec2::new(Fx::ZERO, direction.y.signum()))];
    let mut best: Option<(Fx, (i32, i32), Face)> = None;
    for (into, way) in axes {
        if into.abs() < PUSH {
            continue;
        }
        let tile = Terrain::pit_tile(p + way * (radius + REACH));
        let face = Face::of(-way);
        let (dx, dy) = face.step();
        if tile == here || (tile.0 + dx, tile.1 + dy) != here || !grabbable(volume, tile, feet) {
            continue;
        }
        if best.is_none_or(|(b, _, _)| into.abs() > b) {
            best = Some((into.abs(), tile, face));
        }
    }
    best.map(|(_, tile, face)| (tile, face))
}

/// The stick's direction on a face: `(up, along)`, each from -1 to 1.
fn on_face(face: Face, direction: FxVec2) -> (Fx, Fx) {
    (-direction.dot(face.normal()), direction.dot(face.tangent()))
}

fn stick(avatar: &AvatarState) -> FxVec2 {
    match &avatar.intent {
        Some(Intent::Walk { direction, .. }) => *direction,
        _ => FxVec2::ZERO,
    }
}

/// Puts a body on `tile`'s `face`: [`GAP`] off it, facing it.
fn grab(body: &mut Body, avatar: &mut AvatarState, terrain: &Terrain, volume: &Volume, tile: (i32, i32), face: Face, radius: Fx, phase: ClimbPhase) -> bool {
    let n = face.normal();
    let p = body.position + n * (plane(tile, face) + radius + GAP - body.position.dot(n));
    if !terrain.inside(p) || !body_clear(volume, p, avatar.height, radius) {
        return false;
    }
    body.position = p;
    body.velocity = FxVec2::ZERO;
    body.push = FxVec2::ZERO;
    body.route.clear();
    body.heading = nav::bearing(FxVec2::ZERO, -n);
    avatar.climbing.climb = Some(Climb { tile, face, phase });
    avatar.climbing.pushing = 0;
    avatar.jump = JumpPhase::Grounded;
    avatar.flight.airborne = false;
    true
}

/// Ends a climb standing at `at` with the feet at `feet`.
fn stand(body: &mut Body, avatar: &mut AvatarState, at: FxVec2, feet: Fx) {
    body.position = at;
    body.velocity = FxVec2::ZERO;
    avatar.height = feet;
    avatar.flight.support = feet;
    avatar.flight.airborne = false;
    avatar.jump = JumpPhase::Grounded;
    avatar.climbing.climb = None;
}

/// Stage 2, last for a climber: the top of the fall under way
/// (`AvatarState::fall_top`), the highest its feet have been since it
/// left a floor or a face; kept on the tick it lands, then cleared.
pub(super) fn track_fall(avatar: &mut AvatarState, tick: u64) {
    avatar.fall_top = match avatar.jump {
        JumpPhase::Airborne { .. } if avatar.climbing.climb.is_none() => Some(avatar.fall_top.map_or(avatar.height, |top| top.max(avatar.height))),
        JumpPhase::Landing { started } if started == tick => avatar.fall_top,
        _ => None,
    };
}

/// The health a fall of `height` meters (its top to the floor it lands on)
/// takes (K7): none under `safe_fall`, then 1, and 1 more for each further
/// `fall_step`. The elder's: 5 m takes 1, 7.5 m 2, 10 m 3, 15 m 5, and
/// 17.5 m its 6.
pub fn fall_damage(height: Fx, climb: &ClimbDef) -> u32 {
    if height < climb.safe_fall {
        return 0;
    }
    let steps = ((height - climb.safe_fall) / climb.fall_step).floor_to_int();
    (steps + 1).clamp(1, u32::MAX as i64) as u32
}

/// Lets go of the face, falling from where the climber is (the fall's
/// top). No grabbing again until it stands on a floor.
pub(super) fn let_go(body: &mut Body, avatar: &mut AvatarState, tick: u64) {
    if avatar.climbing.climb.take().is_none() {
        return;
    }
    avatar.climbing.released = true;
    avatar.climbing.pushing = 0;
    body.velocity = FxVec2::ZERO;
    avatar.jump = JumpPhase::Airborne { started: tick, velocity: Fx::ZERO };
    avatar.flight.airborne = true;
    // The fall starts here, though a let-go by command (stage 1) falls a
    // tick's way before `track_fall` sees it.
    avatar.fall_top = Some(avatar.height);
}

/// Where a climber at `p` pulling over the top `top` of `tile` stands, and
/// its feet's height: on the floor `top` (which needs headroom over it),
/// [`MANTLE_IN`] in from the edge, straight in or else in line with the
/// tile's middle.
fn mantle_spot(terrain: &Terrain, volume: &Volume, p: FxVec2, tile: (i32, i32), face: Face, top: i32, radius: Fx) -> Option<(FxVec2, Fx)> {
    if !volume.floors(tile).any(|f| f == top) {
        return None;
    }
    let n = face.normal();
    let inset = plane(tile, face) - radius - MANTLE_IN;
    let straight = p + n * (inset - p.dot(n));
    let middle = center(tile) + n * (inset - center(tile).dot(n));
    let floor = height(top);
    [straight, middle].into_iter().filter(|&q| terrain.inside(q)).find_map(|q| {
        let feet = terrain.footprint_at(q, floor, radius)?;
        (feet >= floor - LAYER - ROUNDING && body_clear(volume, q, feet, radius)).then_some((q, feet))
    })
}

/// The move `side` meters along the face from `p`, feet at `feet`: on along
/// it, round an outside corner onto the tile's other face, or at an inside
/// corner onto the rock ahead, if `turn` (the stick more along the face
/// than up or down it: pushed diagonally into an inside corner, each
/// face's sideways part points into the other, so it would turn back and
/// forth every tick; it climbs the corner instead). The new position,
/// tile and face, or nothing if the way is blocked.
#[allow(clippy::too_many_arguments)]
fn along(terrain: &Terrain, volume: &Volume, p: FxVec2, feet: Fx, climb: &Climb, radius: Fx, side: Fx, turn: bool) -> Option<(FxVec2, (i32, i32), Face)> {
    let (n, t) = (climb.face.normal(), climb.face.tangent());
    let way = t * side.signum();
    let q = p + t * side;
    if !terrain.inside(q) {
        return None;
    }
    // An inside corner: rock beside the body, ahead along the face. It
    // turns onto that rock's face, back to back with the old one.
    let ahead = Terrain::pit_tile(q + way * (radius + GAP));
    if ahead != Terrain::pit_tile(p) && !clear(volume, ahead, feet) {
        if !turn {
            return None;
        }
        hold(volume, ahead, feet)?;
        let face = Face::of(-way);
        let turned = p + face.normal() * (plane(ahead, face) + radius + GAP - p.dot(face.normal()));
        return (terrain.inside(turned) && body_clear(volume, turned, feet, radius)).then_some((turned, ahead, face));
    }
    let wall = Terrain::pit_tile(q - n * (radius + GAP + Fx::HALF));
    if wall == climb.tile || hold(volume, wall, feet).is_some() {
        return body_clear(volume, q, feet, radius).then_some((q, wall, climb.face));
    }
    // An outside corner: the face ends. Round it onto the tile's side.
    let face = Face::of(way);
    let c = center(climb.tile);
    let wrapped = c + way * (Fx::HALF + radius + GAP) + n * (Fx::HALF - radius - GAP);
    (terrain.inside(wrapped) && body_clear(volume, wrapped, feet, radius)).then_some((wrapped, climb.tile, face))
}

/// Moves a climber `up` and `side` meters on its face (`ClimbPhase::Hold`
/// or `Lunge`): up until an overhang (rock over the body) stops it or the
/// top comes within [`MANTLE_BELOW`] and it pulls over; down until it
/// stands on the floor under it; along by [`along`]. With nothing left to
/// hold it falls. Returns whether it moved.
#[allow(clippy::too_many_arguments)]
fn shift(body: &mut Body, avatar: &mut AvatarState, tick: u64, terrain: &Terrain, volume: &Volume, radius: Fx, def: &ClimbDef, up: Fx, side: Fx, turn: bool) -> bool {
    let Some(mut climb) = avatar.climbing.climb else { return false };
    let feet = avatar.height;
    let mut moved = false;
    if up > Fx::ZERO {
        let target = feet + up;
        if let Some(top) = hold(volume, climb.tile, feet) {
            if target >= height(top) - MANTLE_BELOW {
                if let Some((to, top)) = mantle_spot(terrain, volume, body.position, climb.tile, climb.face, top, radius) {
                    climb.phase = ClimbPhase::Mantle { started: tick, until: tick + def.mantle, from: body.position, low: feet, to, top };
                    avatar.climbing.climb = Some(climb);
                    return true;
                }
            } else if body_clear(volume, body.position, target, radius) {
                avatar.height = target;
                moved = true;
            }
        }
    } else if up < Fx::ZERO {
        let target = feet + up;
        if let Some(ground) = terrain.footprint_at(body.position, feet, radius).filter(|&g| target <= g) {
            stand(body, avatar, body.position, ground);
            return true;
        }
        if body_clear(volume, body.position, target, radius) {
            avatar.height = target;
            moved = true;
        }
    }
    if side != Fx::ZERO {
        if let Some((p, tile, face)) = along(terrain, volume, body.position, avatar.height, &climb, radius, side, turn) {
            body.position = p;
            climb.tile = tile;
            climb.face = face;
            moved = true;
        }
    }
    body.heading = nav::bearing(FxVec2::ZERO, -climb.face.normal());
    avatar.climbing.climb = Some(climb);
    if hold(volume, climb.tile, avatar.height).is_none() {
        let_go(body, avatar, tick);
    }
    moved
}

/// Stage 2 for a climber on a face, in place of walking: the pull-over,
/// the lunge or the stick's move, the wheel draining (faster moving than
/// hanging still), and letting go when it is empty.
pub(super) fn step(body: &mut Body, avatar: &mut AvatarState, tick: u64, terrain: &Terrain, def: &CharacterDef, climb_def: &ClimbDef) {
    let (Some(climb), Some(volume)) = (avatar.climbing.climb, terrain.volume.as_ref()) else {
        avatar.climbing.climb = None;
        return;
    };
    let radius = def.body.radius;
    body.velocity = FxVec2::ZERO;
    body.push = FxVec2::ZERO;
    match climb.phase {
        ClimbPhase::Mantle { started, until, from, low, to, top } => {
            let (done, total) = (tick + 1 - started, (until - started).max(1));
            if done >= total {
                stand(body, avatar, to, top);
            } else {
                // Up first, then over the edge: the feet reach the top at
                // two thirds of the way, and only then does the body move
                // over it, so it never cuts through the edge (CL.5's soak).
                let t = Fx::from_ratio(done as i64, total as i64);
                let rise = (t * Fx::from_ratio(3, 2)).min(Fx::ONE);
                let over = ((t - Fx::from_ratio(2, 3)) * Fx::from_int(3)).max(Fx::ZERO);
                avatar.height = Fx::lerp(low, top, rise);
                body.position = FxVec2::lerp(from, to, over);
            }
        }
        ClimbPhase::Lunge { until, up, side, .. } => {
            shift(body, avatar, tick, terrain, volume, radius, climb_def, up, side, true);
            if let Some(c) = avatar.climbing.climb.as_mut().filter(|c| matches!(c.phase, ClimbPhase::Lunge { .. }) && tick + 1 >= until) {
                c.phase = ClimbPhase::Hold;
            }
        }
        ClimbPhase::Hold => {
            let (into, sideways) = on_face(climb.face, stick(avatar));
            let up = if into > Fx::ZERO { into * climb_def.up } else { into * climb_def.down };
            let moved = shift(body, avatar, tick, terrain, volume, radius, climb_def, up, sideways * climb_def.side, sideways.abs() > into.abs());
            let c = &mut avatar.climbing;
            if c.climb.is_some_and(|c| c.phase == ClimbPhase::Hold) {
                c.spent = (c.spent + if moved { climb_def.drain } else { climb_def.hang }).min(Fx::ONE);
            }
        }
    }
    // Out of stamina, it lets go (K6).
    if avatar.climbing.climb.is_some_and(|c| c.phase == ClimbPhase::Hold) && avatar.climbing.spent >= Fx::ONE {
        avatar.climbing.exhausted = true;
        let_go(body, avatar, tick);
    }
}

/// Stage 2 for a climber off any face, after its walk and fall: the wheel
/// refills on a floor, and the stick pushing into a face for `grab` ticks
/// grabs on.
pub(super) fn reach(body: &mut Body, avatar: &mut AvatarState, tick: u64, terrain: &Terrain, def: &CharacterDef, climb_def: &ClimbDef) {
    let on_floor = matches!(avatar.jump, JumpPhase::Grounded | JumpPhase::Landing { .. }) && !avatar.flight.airborne;
    let c = &mut avatar.climbing;
    if on_floor {
        c.released = false;
        c.spent = (c.spent - climb_def.refill).max(Fx::ZERO);
        if c.spent == Fx::ZERO {
            c.exhausted = false;
        }
    }
    let Some(volume) = terrain.volume.as_ref() else {
        c.pushing = 0;
        return;
    };
    let direction = stick(avatar);
    let free = avatar.dig.is_none() && avatar.swing.as_ref().is_none_or(|s| tick >= s.until) && !matches!(avatar.jump, JumpPhase::Preparing { .. });
    let able = !avatar.climbing.exhausted && !avatar.climbing.released && free && direction != FxVec2::ZERO;
    let radius = def.body.radius;
    match face_ahead(volume, body.position, avatar.height, radius, direction).filter(|_| able) {
        None => avatar.climbing.pushing = 0,
        Some((tile, face)) => {
            avatar.climbing.pushing += 1;
            if avatar.climbing.pushing >= climb_def.grab {
                grab(body, avatar, terrain, volume, tile, face, radius, ClimbPhase::Hold);
            }
        }
    }
}

/// A jump for a climber (`Action::Jump`): on a face, a lunge of `lunge`
/// meters in the stick's direction (up without one) for `lunge_cost` of
/// the wheel (K5); on the ground at a face, a grab `leap` meters up it
/// (K3). Returns whether it took the jump, so the character's own jump
/// (if any) doesn't.
pub(super) fn jump(body: &mut Body, avatar: &mut AvatarState, tick: u64, terrain: &Terrain, def: &CharacterDef) -> bool {
    let (Some(climb_def), Some(volume)) = (def.climb.as_ref(), terrain.volume.as_ref()) else { return false };
    let ticks = climb_def.lunge_ticks.max(1);
    let pushed = stick(avatar);
    if let Some(climb) = avatar.climbing.climb.as_mut() {
        if climb.phase == ClimbPhase::Hold {
            let (up, side) = on_face(climb.face, pushed);
            let way = FxVec2::new(up, side).normalize_or_zero();
            let way = if way == FxVec2::ZERO { FxVec2::new(Fx::ONE, Fx::ZERO) } else { way };
            let per_tick = (way * climb_def.lunge).div_int(ticks as i64);
            climb.phase = ClimbPhase::Lunge { started: tick, until: tick + ticks, up: per_tick.x, side: per_tick.y };
            let c = &mut avatar.climbing;
            c.spent = (c.spent + climb_def.lunge_cost).min(Fx::ONE);
            if c.spent >= Fx::ONE {
                c.exhausted = true;
            }
        }
        return true;
    }
    let grounded = matches!(avatar.jump, JumpPhase::Grounded) && !avatar.flight.airborne;
    let busy = avatar.dig.is_some() || avatar.swing.as_ref().is_some_and(|s| tick < s.until) || avatar.seat.is_some();
    if !grounded || busy || avatar.climbing.exhausted {
        return false;
    }
    let direction = match pushed {
        d if d != FxVec2::ZERO => d,
        _ => nav::heading_vector(body.heading),
    };
    let radius = def.body.radius;
    let Some((tile, face)) = face_ahead(volume, body.position, avatar.height, radius, direction) else { return false };
    let up = climb_def.leap.div_int(ticks as i64);
    grab(body, avatar, terrain, volume, tile, face, radius, ClimbPhase::Lunge { started: tick, until: tick + ticks, up, side: Fx::ZERO })
}

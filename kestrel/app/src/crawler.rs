//! The crawler's rig and legs in the view (`kestrel/CRAWLER.md` C8, C12,
//! CR.3 in `kestrel/todo/vehicles.md`).
//!
//! The simulation places the feet (`sim::settlement::crawler`): each planted
//! foot's spot and floor, and a swinging one's start, landing and progress.
//! This draws what joins them to the body. The rig is data
//! (`actors/crawler.ron`): the body's pieces and a leg's hip, thigh, shin and
//! foot, with the segments' lengths. Each leg is a two-bone solve, hip to
//! knee to ankle, with the foot kept upright under the ankle, the knee
//! bending up and outward over the hip line. A swinging foot follows an arc
//! that rises first, crosses at the gait's `lift` over the higher floor and
//! comes down last, so it clears the step it climbs.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sim::settlement::crawler::{Foot, Legs};

/// The crawler's rig: the body's pieces and one leg's, the front right's;
/// the other legs mirror it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rig {
    /// In the body's frame: the origin on the deck's middle, x right, y up,
    /// z forward.
    pub body: Vec<Piece>,
    pub leg: LegRig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegRig {
    /// On the hip, upright, z toward the foot.
    pub hip: Vec<Piece>,
    /// Hip to knee, knee to ankle, ankle to the ground.
    pub thigh: Segment,
    pub shin: Segment,
    pub foot: Segment,
}

/// A leg segment: its pieces in its frame, the origin on the joint it starts
/// from, y along it, z toward the side the knee bends to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub length: f32,
    pub pieces: Vec<Piece>,
}

/// A box, or a cylinder along y, centered at `at`, `turn`ed by these
/// degrees about x, y and z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    pub shape: Shape,
    pub at: [f32; 3],
    pub size: [f32; 3],
    #[serde(default)]
    pub turn: [f32; 3],
    pub material: Finish,
}

impl Piece {
    pub fn transform(&self) -> Transform {
        let [x, y, z] = self.turn.map(f32::to_radians);
        Transform::from_translation(Vec3::from_array(self.at))
            .with_rotation(Quat::from_euler(EulerRot::XYZ, x, y, z))
            .with_scale(Vec3::from_array(self.size))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Box,
    Cylinder,
}

/// The Brasswood palette's finishes (`assets/styles/forest.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Finish {
    Timber,
    DarkTimber,
    Brass,
    Iron,
    Leather,
    Canvas,
    Cloth,
    Lamp,
}

impl Finish {
    pub const ALL: [Finish; 8] = [Finish::Timber, Finish::DarkTimber, Finish::Brass, Finish::Iron, Finish::Leather, Finish::Canvas, Finish::Cloth, Finish::Lamp];

    /// Its material: the style's wood, bark, trim, iron (darker: the
    /// style's reads blue on the lit machine), leather, plaster, cloth and
    /// lantern glow.
    pub fn material(self) -> StandardMaterial {
        let (r, g, b, metallic, roughness) = match self {
            Finish::Timber => (0.4, 0.23, 0.12, 0.0, 0.9),
            Finish::DarkTimber => (0.22, 0.14, 0.1, 0.0, 0.95),
            Finish::Brass => (0.68, 0.44, 0.22, 0.7, 0.4),
            Finish::Iron => (0.2, 0.21, 0.22, 0.5, 0.55),
            Finish::Leather => (0.26, 0.14, 0.09, 0.0, 0.9),
            Finish::Canvas => (0.7, 0.65, 0.49, 0.0, 0.95),
            Finish::Cloth => (0.19, 0.43, 0.47, 0.0, 0.9),
            Finish::Lamp => (0.95, 0.72, 0.42, 0.0, 0.6),
        };
        let emissive = if self == Finish::Lamp { LinearRgba::rgb(10.0, 5.0, 1.5) } else { LinearRgba::BLACK };
        StandardMaterial { base_color: Color::srgb(r, g, b), metallic, perceptual_roughness: roughness, emissive, ..default() }
    }
}

/// The built-in rig, until a style supplies a `crawler` skin. Panics if it
/// is invalid: it is compiled in, and `the_rig_loads_and_validates` checks it.
pub fn builtin() -> &'static Rig {
    static RIG: std::sync::LazyLock<Rig> = std::sync::LazyLock::new(|| parse(include_str!("../actors/crawler.ron")).unwrap_or_else(|e| panic!("crawler rig: {e}")));
    &RIG
}

fn parse(text: &str) -> Result<Rig, String> {
    let rig: Rig = ron::from_str(text).map_err(|e| e.to_string())?;
    let leg = &rig.leg;
    let finite = |v: &[f32; 3], most: f32| v.iter().all(|x| x.is_finite() && x.abs() <= most);
    let pieces = rig.body.iter().chain(&leg.hip).chain([&leg.thigh, &leg.shin, &leg.foot].into_iter().flat_map(|s| &s.pieces));
    for piece in pieces {
        if !finite(&piece.at, 8.0) || !finite(&piece.size, 8.0) || piece.size.iter().any(|s| *s <= 0.0) || !finite(&piece.turn, 360.0) {
            return Err(format!("invalid piece {piece:?}"));
        }
    }
    for (name, segment) in [("thigh", &leg.thigh), ("shin", &leg.shin), ("foot", &leg.foot)] {
        if !(segment.length > 0.0 && segment.length <= 8.0) {
            return Err(format!("the {name}'s length must be over 0 and at most 8 m"));
        }
    }
    Ok(rig)
}

/// Where a leg's joints are drawn: the hip, the knee, the ankle and the
/// foot's sole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegPose {
    pub hip: Vec3,
    pub knee: Vec3,
    pub ankle: Vec3,
    pub foot: Vec3,
}

/// How far out of reach a foot may be before the leg straightens fully: the
/// leg eases into it rather than snapping straight.
const SOFT_REACH: f32 = 0.25;
/// How far the knee leans outward from straight up.
const KNEE_OUT: f32 = 0.35;

/// Solves a leg from `hip` to a foot at `foot` (the sole, on the ground),
/// `outward` the horizontal away from the body on its side. The foot stays
/// upright, its ankle `foot.length` over the sole; the knee bends in the
/// plane through hip and ankle toward up and `outward`, so it never flips.
/// An ankle out of reach is drawn short of it, the leg nearly straight
/// (`SOFT_REACH`) or folded as far as it goes.
pub fn solve(leg: &LegRig, hip: Vec3, outward: Vec3, foot: Vec3) -> LegPose {
    let (a, b) = (leg.thigh.length, leg.shin.length);
    let target = foot + Vec3::Y * leg.foot.length;
    let to = target - hip;
    let d = to.length();
    let along = if d > 1e-4 { to / d } else { Vec3::NEG_Y };
    let longest = a + b;
    let soft = longest - SOFT_REACH;
    let reach = if d > soft { longest - SOFT_REACH * (-(d - soft) / SOFT_REACH).exp() } else { d };
    let reach = reach.clamp((a - b).abs() + 1e-3, longest - 1e-4);
    let ankle = hip + along * reach;
    let pole = (Vec3::Y + outward * KNEE_OUT).normalize();
    let side = (pole - along * pole.dot(along)).try_normalize().unwrap_or(outward);
    let cos = ((reach * reach + a * a - b * b) / (2.0 * reach * a)).clamp(-1.0, 1.0);
    let knee = hip + a * (along * cos + side * (1.0 - cos * cos).sqrt());
    LegPose { hip, knee, ankle, foot: ankle - Vec3::Y * leg.foot.length }
}

/// A segment's frame from `from` toward `to`: y along it, z toward `pole`'s
/// side of it.
pub fn segment_frame(from: Vec3, to: Vec3, pole: Vec3) -> Transform {
    let y = (to - from).try_normalize().unwrap_or(Vec3::Y);
    let z = (pole - y * pole.dot(y)).try_normalize().unwrap_or_else(|| y.any_orthonormal_vector());
    Transform::from_translation(from).with_rotation(Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)))
}

impl LegPose {
    /// The frames the hip and the three segments are drawn in.
    pub fn frames(&self, outward: Vec3) -> [Transform; 4] {
        let pole = (Vec3::Y + outward * KNEE_OUT).normalize();
        let flat = (self.ankle - self.hip).with_y(0.0).try_normalize().unwrap_or(outward);
        let upright = |at: Vec3, down: bool| {
            let y = if down { Vec3::NEG_Y } else { Vec3::Y };
            Transform::from_translation(at).with_rotation(Quat::from_mat3(&Mat3::from_cols(y.cross(flat), y, flat)))
        };
        [upright(self.hip, false), segment_frame(self.hip, self.knee, pole), segment_frame(self.knee, self.ankle, pole), upright(self.ankle, true)]
    }

    /// The knee stands off the line from hip to ankle toward up and
    /// `outward`, never under it or inward.
    #[cfg(test)]
    pub fn knee_over_hip_line(&self, outward: Vec3) -> bool {
        let line = (self.ankle - self.hip).normalize();
        let off = (self.knee - self.hip) - line * (self.knee - self.hip).dot(line);
        off.length() > 0.1 && off.dot(Vec3::Y + outward * KNEE_OUT) > 0.0
    }
}

/// How far into a swing (0 to 1) the foot lifts before moving across, and
/// comes down after: the arc rises first, so it clears a step by its start.
const LIFT_FIRST: f32 = 0.2;
/// How far into a swing it reaches its top.
const RISE: f32 = 0.4;

/// A swinging foot's sole at `t` (0 to 1) from `from` to `to` (each on its
/// floor): it rises to `lift` over the higher floor while barely moving,
/// crosses over, and comes down onto `to`, easing in and out.
pub fn swing_point(from: Vec3, to: Vec3, lift: f32, t: f32) -> Vec3 {
    let t = t.clamp(0.0, 1.0);
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let across = smooth((t - LIFT_FIRST) / (1.0 - 2.0 * LIFT_FIRST));
    let top = from.y.max(to.y) + lift;
    let y = if t < 0.5 { from.y + (top - from.y) * smooth(t / RISE) } else { to.y + (top - to.y) * smooth((1.0 - t) / RISE) };
    from.lerp(to, across).with_y(y)
}

/// Where a foot's sole is drawn in one tick's state: planted, on its spot;
/// swinging, along its arc. `view` turns a simulation point into the view.
pub fn foot_point(foot: &Foot, legs: &Legs, c: &content::character::CrawlerDef, view: impl Fn(fx::FxVec2) -> Vec3) -> Vec3 {
    let from = view(foot.spot).with_y(foot.floor.to_f32());
    match &foot.swing {
        Some(s) => {
            let gait = legs.gait.def(c);
            swing_point(from, view(s.to).with_y(s.floor.to_f32()), gait.lift.to_f32(), s.progress as f32 / gait.swing as f32)
        }
        None => from,
    }
}

/// A leg's hip in the body's frame (on the deck, the origin under the deck's
/// middle on the ground) and the horizontal outward on its side, for leg
/// `leg` in `VehicleState::wheels` order (front left, front right, rear
/// left, rear right).
pub fn hip_local(c: &content::character::CrawlerDef, leg: usize, deck: f32) -> (Vec3, Vec3) {
    let along = if leg < 2 { c.hip.along } else { -c.hip.along }.to_f32();
    let right = leg % 2 == 1;
    let across = if right { c.hip.across } else { -c.hip.across }.to_f32();
    (Vec3::new(across, deck, along), if right { Vec3::X } else { Vec3::NEG_X })
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::character::CrawlerDef;
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::{SceneSetup, VehicleSetup};
    use sim::{Command, CommandKind, PlayerId, Simulation};

    fn crawler() -> CrawlerDef {
        let content = crate::lockstep::builtin_content();
        let ix = content.names.character_index("crawler").unwrap();
        content.rules.characters[ix as usize].crawler.unwrap()
    }

    #[test]
    fn the_rig_loads_and_validates() {
        let rig = builtin();
        assert!(!rig.body.is_empty() && !rig.leg.hip.is_empty());
        let c = crawler();
        let leg = &rig.leg;
        // The farthest foot in reach, its ankle too.
        assert!(leg.thigh.length + leg.shin.length > c.reach.to_f32() + SOFT_REACH, "a leg reaches {}", c.reach.to_f32());
        // The body hangs no lower than its belly (`crawler::BELLY`).
        let belly = sim::settlement::crawler::BELLY.to_f32();
        for piece in &rig.body {
            assert!(piece.at[1] - piece.size[1] / 2.0 >= -belly - 1e-4, "{piece:?} hangs under the belly");
        }
        let bad = include_str!("../actors/crawler.ron").replacen("length: 1.8", "length: -1", 1);
        assert!(parse(&bad).unwrap_err().contains("thigh"));
        let bad = include_str!("../actors/crawler.ron").replacen("size: (2.2, 1.0, 5.3)", "size: (2.2, 0, 5.3)", 1);
        assert!(parse(&bad).unwrap_err().contains("invalid piece"));
        assert!(parse("(body: [], leg: ())").is_err());
    }

    /// The riders' places on the deck (`crawler::RIDERS`, CR.4) are clear:
    /// no piece standing on the deck within an elder's body of one, and
    /// each place on the deck, off the cab and the pilot's helm.
    #[test]
    fn the_rig_keeps_the_riders_places_clear() {
        let c = crawler();
        let body = 0.3;
        for (i, o) in sim::settlement::crawler::RIDERS.iter().enumerate() {
            let (x, z) = (o.across.to_f32(), o.along.to_f32());
            assert!(x.abs() < c.width.to_f32() / 2.0 - body && z.abs() < c.length.to_f32() / 2.0 - body, "rider {i} on the deck");
            assert!(z < -0.3, "rider {i} behind the pilot and the helm");
            for piece in builtin().body.iter().filter(|p| p.at[1] + p.size[1] / 2.0 > 0.05) {
                let (dx, dz) = ((x - piece.at[0]).abs() - piece.size[0] / 2.0, (z - piece.at[2]).abs() - piece.size[2] / 2.0);
                assert!(dx > body || dz > body, "rider {i} at ({x}, {z}) inside {piece:?}");
            }
        }
    }

    /// The solve puts the sole on the foot it is given, upright, its knee
    /// over the hip and over the hip line; out of reach it eases straight,
    /// never past the target, never snapping.
    #[test]
    fn the_solve_reaches_its_foot_with_the_knee_high() {
        let leg = &builtin().leg;
        let c = crawler();
        let (hip, out) = hip_local(&c, 1, c.deck.to_f32());
        let rest = Vec3::new(c.stance.across.to_f32(), 0.0, c.stance.along.to_f32());
        let pose = solve(leg, hip, out, rest);
        assert!(pose.foot.distance(rest) < 1e-4);
        assert!((pose.ankle - pose.foot - Vec3::Y * leg.foot.length).length() < 1e-4, "upright");
        assert!((pose.knee.distance(hip) - leg.thigh.length).abs() < 1e-4 && (pose.knee.distance(pose.ankle) - leg.shin.length).abs() < 1e-4);
        assert!(pose.knee.y > hip.y + 0.5 && pose.knee.x > hip.x && pose.knee_over_hip_line(out), "a high knee, outward: {pose:?}");
        // Pulled away past reach, the foot follows short of it, smoothly.
        let mut last = pose.foot;
        for i in 1..=60 {
            let target = rest + out * (i as f32 * 0.05);
            let p = solve(leg, hip, out, target);
            assert!(p.foot.distance(last) < 0.06, "no snap at {i}: {last} to {}", p.foot);
            assert!(p.foot.distance(hip) <= target.distance(hip) + 1e-4);
            assert!(p.knee_over_hip_line(out));
            last = p.foot;
        }
    }

    /// The Crawler Lab with the crawler parked at `at` (scene meters) facing
    /// `heading` on a floor `floor` m up, its pilot on board.
    fn piloted(at: [f32; 2], floor: f32, heading: f32) -> Simulation {
        let library = crate::assets::SceneLibrary::new();
        let scene = &library.scenes[library.index("crawler-lab").unwrap()].scene;
        let q = Fx::quantize;
        // The pilot starts beside its left side.
        let (s, c) = heading.sin_cos();
        let spawn = [at[0] - 2.5 * c, at[1] + 2.5 * s];
        let setup = SceneSetup {
            scenario: scene.scenario.clone(),
            player: scene.player.clone(),
            extent: q(crate::assets::extent(&library.scenes[library.index("crawler-lab").unwrap()])),
            sloped: true,
            volume: scene.volume.as_ref().map(|v| v.build()),
            spawn: [q(spawn[0]), q(floor), q(spawn[1])],
            vehicles: vec![VehicleSetup { character: "crawler".into(), position: [q(at[0]), q(at[1])], heading: q(heading) }],
            ..SceneSetup::default()
        };
        let world = sim::World::new_scene(1, crate::lockstep::builtin_content(), &setup).unwrap();
        let mut sim = Simulation::from_world(world, 1);
        let agent = sim.world().settlement().unwrap().player().unwrap();
        sim.step(&[Command { player: PlayerId(0), kind: CommandKind::Board { agent } }]);
        assert_eq!(sim.world().settlement().unwrap().driving(agent as usize), Some(0), "boarded");
        sim
    }

    /// Holds the stick (scene x and z) for `seconds`, calling `each` with
    /// the crawler after every tick.
    fn drive(sim: &mut Simulation, direction: [f32; 2], run: bool, seconds: f32, mut each: impl FnMut(&sim::settlement::VehicleState, &sim::terrain::Terrain)) {
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let direction = FxVec2::new(Fx::quantize(direction[0]), Fx::quantize(direction[1]));
        let intent = if direction == FxVec2::ZERO { Intent::Stop } else { Intent::Walk { direction, run } };
        sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MoveAvatar { agent, intent } }]);
        for _ in 0..(seconds * content::TICKS_PER_SECOND as f32) as u32 {
            sim.step(&[]);
            let world = sim.world();
            each(&world.settlement().unwrap().vehicles[0], world.terrain());
        }
    }

    /// The level top floor over a view point (the ground elsewhere).
    fn ground(terrain: &sim::terrain::Terrain, p: Vec3) -> f32 {
        let at = terrain.center + FxVec2::new(Fx::quantize(p.x), Fx::quantize(p.z));
        let volume = terrain.volume.as_ref().expect("the lab's volume");
        volume.top(sim::terrain::Terrain::pit_tile(at)).map_or(f32::MIN, |top| sim::volume::height(top).to_f32())
    }

    #[derive(Default)]
    struct Counts {
        planted: usize,
        knee_over_hip: usize,
        swings: usize,
    }

    /// Every tick: every planted foot reached exactly with the knee over the
    /// line from hip to ankle (and counted over the hip itself), and every
    /// swing's whole arc over the ground.
    fn check(v: &sim::settlement::VehicleState, terrain: &sim::terrain::Terrain, c: &CrawlerDef, counts: &mut Counts) {
        let legs = v.legs.as_ref().expect("a crawler's legs");
        let leg_rig = &builtin().leg;
        let view = |p: FxVec2| Vec3::new((p.x - terrain.center.x).to_f32(), 0.0, (p.y - terrain.center.y).to_f32());
        let root = Transform::from_translation(view(v.position)).with_rotation(Quat::from_rotation_y(v.heading.to_f32()));
        for (i, foot) in legs.feet.iter().enumerate() {
            let (hip, out) = hip_local(c, i, (v.height + c.deck).to_f32());
            let (hip, out) = (root.transform_point(hip), root.rotation * out);
            match &foot.swing {
                None => {
                    let spot = foot_point(foot, legs, c, view);
                    let pose = solve(leg_rig, hip, out, spot);
                    assert!(pose.foot.distance(spot) < 1e-3, "leg {i} reaches its foot: {pose:?}, {spot}");
                    assert!(pose.knee_over_hip_line(out), "leg {i}'s knee over its hip line: {pose:?}");
                    counts.planted += 1;
                    counts.knee_over_hip += (pose.knee.y > hip.y) as usize;
                }
                Some(s) if s.progress == 1 => {
                    counts.swings += 1;
                    let (from, to) = (view(foot.spot).with_y(foot.floor.to_f32()), view(s.to).with_y(s.floor.to_f32()));
                    let lift = legs.gait.def(c).lift.to_f32();
                    for k in 1..100 {
                        let p = swing_point(from, to, lift, k as f32 / 100.0);
                        assert!(p.y > ground(terrain, p), "leg {i}'s swing from {from} to {to} in the ground at {p}");
                    }
                }
                Some(_) => {}
            }
        }
    }

    #[test]
    fn the_legs_reach_their_feet_and_swing_over_the_lab() {
        let c = crawler();
        let mut counts = Counts::default();
        // Up the plateau's 1.5 m north face, south (+z).
        let mut up = piloted([23.0, -16.0], 0.0, 0.0);
        drive(&mut up, [0.0, 1.0], false, 14.0, |v, t| check(v, t, &c, &mut counts));
        assert!(up.world().settlement().unwrap().vehicles[0].height > Fx::ONE, "it climbed the step");
        // Down it, north.
        let mut down = piloted([23.0, 2.0], 1.5, std::f32::consts::PI);
        drive(&mut down, [0.0, -1.0], false, 12.0, |v, t| check(v, t, &c, &mut counts));
        assert_eq!(down.world().settlement().unwrap().vehicles[0].height, Fx::ZERO, "it came down");
        // West across the trench toward the pit, trotting the last stretch.
        let mut west = piloted([-4.0, 0.0], 0.0, -std::f32::consts::FRAC_PI_2);
        drive(&mut west, [-1.0, 0.0], false, 12.0, |v, t| check(v, t, &c, &mut counts));
        let s = west.world().settlement().unwrap();
        assert!((s.vehicles[0].position - west.world().terrain().center).x < Fx::from_int(-12), "past the trench: {:?}", s.vehicles[0].position - west.world().terrain().center);
        drive(&mut west, [-1.0, 0.0], true, 3.0, |v, t| check(v, t, &c, &mut counts));
        // Turning in place on open ground by the spawn, then trotting.
        let mut turn = piloted([2.5, 0.0], 0.0, 0.0);
        drive(&mut turn, [1.0, 0.0], false, 6.0, |v, t| check(v, t, &c, &mut counts));
        let heading = turn.world().settlement().unwrap().vehicles[0].heading.to_f32().to_degrees();
        assert!(heading.abs() > 60.0, "turned in place to {heading}");
        drive(&mut turn, [1.0, 0.0], true, 4.0, |v, t| check(v, t, &c, &mut counts));
        let Counts { planted, knee_over_hip, swings } = counts;
        println!("{planted} planted feet, {knee_over_hip} with the knee over the hip; {swings} swings");
        assert!(swings > 40, "{swings} swings");
        // The knee stands over the hip but for a far foot under a raised
        // body (the rear feet below the step as the front ones climb it).
        assert!(knee_over_hip as f32 > 0.95 * planted as f32, "{knee_over_hip} of {planted} knees over the hip");
    }
}

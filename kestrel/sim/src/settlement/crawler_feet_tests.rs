//! Feet and bodies (CR.5 of the crawler, `kestrel/CRAWLER.md` C10): in the
//! Crawler Lab, a foot coming down shoves the bodies under it off its spot
//! and stomps the ghouls there; seated elders are out of it; a foot never
//! comes down on an elder or a sprite standing where it would; bodies path
//! round the feet.

use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::crawler;
use super::crawler_lab_tests::{board, copy, crawler_lab, crawler_lab_with_guests, owner, seconds, SOUTH};
use super::tunnel_lab_tests::{command, me};
use super::{AgentState, Event, Impact, Settlement};
use crate::messages::FootLands;
use crate::nav::heading_vector;
use crate::{Command, CommandKind, Simulation, World};

/// The pilot (`agents[0]`) holds the stick south.
fn walk_south(agents: &[u16]) -> Vec<Command> {
    vec![Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Walk { direction: heading_vector(SOUTH), run: false } } }]
}

/// The foot one tick from coming down, if one is: its leg, where it lands
/// and the floor there.
fn about_to_land(sim: &Simulation) -> Option<(usize, FxVec2, Fx)> {
    let s = sim.world().settlement().unwrap();
    let v = &s.vehicles[0];
    let legs = v.legs.as_ref().unwrap();
    let swing = legs.gait.def(s.characters[v.character as usize].crawler.as_ref().unwrap()).swing;
    legs.feet.iter().enumerate().find_map(|(leg, f)| f.swing.as_ref().filter(|w| w.progress + 1 == swing).map(|w| (leg, w.to, w.floor)))
}

/// The last tick's footfalls: leg, spot, bodies shoved, threats stomped.
fn footfalls(sim: &Simulation) -> Vec<(u8, FxVec2, u8, u8)> {
    let s = sim.world().settlement().unwrap();
    s.events
        .iter()
        .filter_map(|e| match *e {
            Event::Footfall { leg, position, shoved, stomped, .. } => Some((leg, position, shoved, stomped)),
            _ => None,
        })
        .collect()
}

/// A foot that came down with something put under it.
struct Landing {
    /// Just after it came down: this tick's events are the landing's.
    sim: Simulation,
    leg: usize,
    to: FxVec2,
    floor: Fx,
    /// Every tick's hash, from boarding to a second after.
    hashes: Vec<u64>,
}

/// The pilot walks the crawler south from the spawn and, a tick before a
/// foot comes down after `after` seconds, `put` places something on its
/// spot; then it comes down. On `threads` threads, and saved and loaded
/// just before it comes down if `save`.
fn put_under_a_foot(threads: usize, after: f64, save: bool, put: impl Fn(&mut Settlement, FxVec2, Fx)) -> Landing {
    let (sim, agents) = crawler_lab_with_guests();
    let mut sim = Simulation::from_world(sim.into_world(), threads);
    board(&mut sim, &agents, &[0]);
    let mut hashes = vec![sim.step(&walk_south(&agents)).total];
    for tick in 0..seconds(after + 10.0) {
        if tick >= seconds(after) {
            if let Some((leg, to, floor)) = about_to_land(&sim) {
                put(sim.world_mut().settlement_mut(), to, floor);
                if save {
                    let world = sim.into_world();
                    let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
                    let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), world.rules().clone()).unwrap();
                    sim = Simulation::from_world(loaded, threads);
                }
                hashes.push(sim.step(&[]).total);
                let mut on = Simulation::from_world(copy(sim.world()), threads);
                for _ in 0..seconds(1.0) {
                    hashes.push(on.step(&[]).total);
                }
                return Landing { sim, leg, to, floor, hashes };
            }
        }
        hashes.push(sim.step(&[]).total);
    }
    panic!("no foot came down");
}

/// A guest elder steps onto a landing foot's spot after it lifted: the
/// foot comes down and shoves them just clear of it, the way they stood
/// off its middle, on the floor, and says so (`Event::Footfall`). The same
/// on 4 threads and across a save made just before.
#[test]
fn a_landing_foot_shoves_a_body_off_its_spot() {
    let guest = crawler_lab_with_guests().1[1];
    let put = move |s: &mut Settlement, to: FxVec2, floor: Fx| {
        let a = &mut s.agents[guest as usize];
        a.body.position = to + FxVec2::new(Fx::from_ratio(1, 10), Fx::ZERO);
        a.avatar_mut().unwrap().height = floor;
    };
    let one = put_under_a_foot(1, 3.0, false, put);
    let (leg, to, floor) = (one.leg, one.to, one.floor);
    let s = one.sim.world().settlement().unwrap();
    let a = &s.agents[guest as usize];
    let foot = &s.vehicles[0].legs.as_ref().unwrap().feet[leg];
    assert_eq!((foot.spot, foot.floor, foot.swing.is_none()), (to, floor, true), "the foot came down");
    let gap = (a.body.position - to).length();
    assert!(gap >= a.radius() + crawler::FOOT_RADIUS, "shoved clear: {} m from the foot", gap.to_f64());
    assert!(gap <= a.radius() + crawler::FOOT_RADIUS + Fx::from_ratio(1, 5), "just clear: {} m", gap.to_f64());
    assert!((a.body.position - to).x > Fx::ZERO, "away from the foot");
    assert_eq!(a.avatar().unwrap().height, floor, "on the floor");
    assert_eq!(footfalls(&one.sim), vec![(leg as u8, to, 1, 0)]);
    s.validate().unwrap();
    s.validate_footholds(one.sim.world().terrain()).unwrap();
    assert_eq!(one.hashes, put_under_a_foot(4, 3.0, false, put).hashes, "thread count changed the shove");
    assert_eq!(one.hashes, put_under_a_foot(2, 3.0, true, put).hashes, "a save before the landing changed it");
}

/// A ghoul under a landing foot is stomped: the lab's have 2 hit points
/// against `STOMP_DAMAGE`, and it dies as a strike's kill would
/// (`Impact::Stomp`). One with more health lives, thrown off its spot and
/// staggered. The same on 4 threads and across a save.
#[test]
fn a_landing_foot_stomps_a_ghoul() {
    let put = |hp: u32| {
        move |s: &mut Settlement, to: FxVec2, floor: Fx| {
            let t = s.threats.first_mut().expect("a ghoul out");
            t.position = to;
            t.height = floor;
            t.hp = hp;
            t.windup = None;
        }
    };
    let one = put_under_a_foot(1, 4.0, false, put(2));
    let s = one.sim.world().settlement().unwrap();
    assert!(s.characters[s.waves[0].character as usize].health.as_ref().unwrap().hp < crawler::STOMP_DAMAGE, "the lab's ghouls die of one");
    assert_eq!(footfalls(&one.sim), vec![(one.leg as u8, one.to, 0, 1)]);
    let killed = s.events.iter().filter(|e| matches!(e, Event::Hit { impact: Impact::Stomp, killed: true, .. })).count();
    assert_eq!(killed, 1, "stomped dead");
    assert_eq!(s.combat.kills, 1);
    assert!(s.threats.iter().all(|t| (t.position - one.to).length() > Fx::ONE), "no ghoul left under it");
    assert_eq!(one.hashes, put_under_a_foot(4, 4.0, false, put(2)).hashes, "thread count changed the stomp");
    assert_eq!(one.hashes, put_under_a_foot(2, 4.0, true, put(2)).hashes, "a save before the landing changed it");

    let strong = put_under_a_foot(1, 4.0, false, put(10));
    let s = strong.sim.world().settlement().unwrap();
    let t = s.threats.iter().find(|t| t.hp == 10 - crawler::STOMP_DAMAGE).expect("the strong one lives, hurt");
    assert!(t.stagger_until > strong.sim.world().tick() && t.knockback.length() > Fx::ONE, "staggered and thrown off");
    assert!(t.knockback.dot(t.position - strong.to) >= Fx::ZERO, "away from the foot");
    assert!(s.events.iter().any(|e| matches!(e, Event::Hit { impact: Impact::Stomp, killed: false, .. })));
    assert_eq!(footfalls(&strong.sim), vec![(strong.leg as u8, strong.to, 0, 1)]);
}

/// The crawler walks south through four elders and a sprite standing
/// where its feet came down on a walk with nobody about: no foot ever
/// comes down on one, so none is shoved, and it walks on.
#[test]
fn a_foot_never_lands_on_an_elder_or_sprite_among_them() {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0]);
    // Where the feet come down walking south with nobody in the way.
    let mut free = Simulation::from_world(copy(sim.world()), 1);
    free.step(&walk_south(&agents));
    let mut spots: Vec<FxVec2> = Vec::new();
    for tick in 0..seconds(9.0) {
        free.step(&[]);
        for (_, at, _, _) in footfalls(&free) {
            if tick > seconds(1.0) && spots.iter().all(|s| (*s - at).length() > Fx::from_ratio(6, 5)) {
                spots.push(at);
            }
        }
    }
    assert!(spots.len() >= 5, "feet came down: {}", spots.len());
    // Four guests and a sprite stand there.
    let content = Content::builtin();
    let ix = content.names.character_index("sprite").unwrap();
    let s = sim.world_mut().settlement_mut();
    for (&agent, &spot) in agents[1..].iter().zip(&spots) {
        s.agents[agent as usize].body.position = spot;
    }
    let def = s.characters[ix as usize].clone();
    let sprite = content::scenario::Sprite { id: "sprite".into(), label: "SPRITE".into(), character: "sprite".into(), position: [Fx::ZERO; 2], follow: false };
    let items = s.items.len();
    s.agents.push(AgentState::new_sprite(&sprite, ix, &def, spots[4], items));
    let bodies: Vec<u16> = agents[1..].iter().copied().chain([s.agents.len() as u16 - 1]).collect();
    let start = s.vehicles[0].position;
    sim.step(&walk_south(&agents));
    let mut landed = 0;
    for tick in 0..seconds(9.0) {
        sim.step(&[]);
        let s = sim.world().settlement().unwrap();
        for (_, at, shoved, _) in footfalls(&sim) {
            landed += 1;
            assert_eq!(shoved, 0, "tick {tick}: nobody under a foot");
            for &b in &bodies {
                let a = &s.agents[b as usize];
                assert!((a.body.position - at).length() >= a.radius() + crawler::FOOT_RADIUS, "tick {tick}: a foot on {}", a.id);
            }
        }
    }
    let s = sim.world().settlement().unwrap();
    assert!(s.agents.last().unwrap().sprite().is_some_and(|_| s.agents.last().unwrap().body.alive()), "the sprite stood there");
    assert!(landed >= 8, "it stepped: {landed}");
    assert!((s.vehicles[0].position - start).y > Fx::from_int(3), "it walked on among them");
}

/// The pilot and riders sit on the deck: a foot coming down where one sits
/// (a spot forced under them) lists none of them, and a message naming
/// them all moves nobody.
#[test]
fn seated_elders_are_never_shoved() {
    let (mut sim, agents) = crawler_lab_with_guests();
    board(&mut sim, &agents, &[0, 1, 2, 3]);
    let terrain = sim.world().terrain().clone();
    let tick = sim.world().tick();
    for &seated in &agents[..4] {
        let s = sim.world_mut().settlement_mut();
        assert!(s.seat(seated as usize).is_some(), "elder {seated} aboard");
        let (at, height) = (s.agents[seated as usize].body.position, s.agents[seated as usize].height());
        let foot = &mut s.vehicles[0].legs.as_mut().unwrap().feet[0];
        (foot.spot, foot.floor) = (at, height);
        s.landings = vec![(0, 0)];
        let read = s.read_footfalls(&terrain);
        let message = read[0].1.foot_lands[0].clone();
        assert!(message.agents.is_empty(), "seated elder {seated} isn't under the foot");
        let before = s.agents.clone();
        s.foot_lands(tick, FootLands { agents: agents[..4].to_vec(), ..message }, &terrain);
        assert_eq!(s.agents, before, "nobody moved");
        s.landings.clear();
    }
}

/// An elder sent straight across a planted foot walks round it, and gets
/// there.
#[test]
fn bodies_path_round_planted_feet() {
    let mut sim = crawler_lab();
    let agent = sim.world().settlement().unwrap().player().unwrap();
    let foot = sim.world().settlement().unwrap().vehicles[0].legs.as_ref().unwrap().feet[0].spot;
    let (from, to) = (foot - FxVec2::new(Fx::from_int(2), Fx::ZERO), foot + FxVec2::new(Fx::from_int(2), Fx::ZERO));
    sim.world_mut().settlement_mut().agents[agent as usize].body.position = from;
    command(&mut sim, CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target: to, run: false } });
    let radius = me(&sim).radius();
    let mut nearest = Fx::MAX;
    for _ in 0..seconds(6.0) {
        sim.step(&[]);
        nearest = nearest.min((me(&sim).body.position - foot).length());
    }
    assert!(nearest >= radius + crawler::FOOT_RADIUS, "round the foot: {} m from it", nearest.to_f64());
    assert!((me(&sim).body.position - to).length() < Fx::from_ratio(1, 5), "got there: {:?}", me(&sim).body.position - to);
    assert_eq!(sim.world().settlement().unwrap().vehicles[0].legs.as_ref().unwrap().feet[0].spot, foot, "the foot stayed down");
}

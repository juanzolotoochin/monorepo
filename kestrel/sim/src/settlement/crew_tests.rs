//! A crawler's crew (TU.4 of `kestrel/CRAWLER_TURRET.md`, T3, T4): in the
//! Crawler Lab a sprite on the crew job walks to the crawler, climbs to its
//! gun while it stands still and shoots; it waits while the crawler walks;
//! another job climbs it down; an elder boarding takes the gun from it, and
//! it rides or climbs down, and takes the gun back when the elder leaves.
//! Saves, hashes, four threads and prediction hold with a crew aboard, and
//! `validate` holds every tick.

use content::character::Offset;
use fx::FxVec2;

use super::avatar::Intent;
use super::crawler::{self, Pose};
use super::crawler_lab_tests::{board, copy, owner, seconds};
use super::sprites::{SpriteJob, SpriteState, Step};
use super::turret_tests::{ash as ash_of, fx, ghoul, lab, settlement, shots, south};
use super::{AgentIx, Seat, Settlement};
use crate::{Command, CommandKind, Simulation, World};

fn set_job(sprite: AgentIx, job: SpriteJob) -> Command {
    Command { player: owner(0), kind: CommandKind::SetSpriteJob { agent: sprite, job } }
}

/// The pilot lets go of the stick.
fn stop(agents: &[u16]) -> Command {
    Command { player: owner(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Stop } }
}

fn sprite(s: &Settlement, a: AgentIx) -> &SpriteState {
    s.agents[a as usize].sprite().unwrap()
}

/// The lab's second sprite (`sprite_fern`).
fn fern(sim: &Simulation) -> AgentIx {
    settlement(sim).agents.iter().position(|a| a.id == "sprite_fern").unwrap() as AgentIx
}

/// Steps once with `commands` and checks the settlement.
fn step(sim: &mut Simulation, commands: &[Command]) {
    sim.step(commands);
    settlement(sim).validate().unwrap();
}

/// Steps with no commands until `done`, at most `ticks`, checking every
/// tick; returns the ticks it took.
fn until(sim: &mut Simulation, ticks: u64, what: &str, done: impl Fn(&Settlement) -> bool) -> u64 {
    for t in 0..ticks {
        if done(settlement(sim)) {
            return t;
        }
        step(sim, &[]);
    }
    panic!("not within {ticks} ticks: {what}");
}

/// How far body `a` stands past the crawler's box.
fn off_box(s: &Settlement, a: AgentIx) -> f64 {
    let v = &s.vehicles[0];
    let c = s.characters[v.character as usize].crawler.as_ref().unwrap();
    crawler::box_distance_squared(Pose::of(v), c, s.agents[a as usize].body.position).to_f64().sqrt()
}

/// Where `place` on the crawler's deck is now.
fn deck(s: &Settlement, place: Offset) -> FxVec2 {
    let v = &s.vehicles[0];
    v.seat_at(&s.characters[v.character as usize], Some(place)).0
}

/// Ash on the crew job, at the gun of the still crawler.
fn manned() -> (Simulation, Vec<u16>, AgentIx) {
    let (mut sim, agents) = lab();
    let ash = ash_of(&sim);
    step(&mut sim, &[set_job(ash, SpriteJob::Crew)]);
    until(&mut sim, seconds(30.0), "ash at the gun", |s| s.sprite_gunner_of(0) == Some(ash as usize));
    (sim, agents, ash)
}

/// A sprite told to crew walks from the stone pile to the still crawler,
/// round to the free spot beside its box nearest it, climbs to the gun and
/// shoots a ghoul in range on its own.
#[test]
fn a_crew_sprite_boards_a_stopped_crawler_and_shoots() {
    let (mut sim, _agents) = lab();
    let ash = ash_of(&sim);
    let start = settlement(&sim).agents[ash as usize].body.position;
    assert!(off_box(settlement(&sim), ash) > 3.0, "it starts well away");
    step(&mut sim, &[set_job(ash, SpriteJob::Crew)]);
    step(&mut sim, &[]);
    assert!(matches!(sprite(settlement(&sim), ash).plan[..], [Step::GoTo(_), Step::Crew(0)]), "{:?}", sprite(settlement(&sim), ash).plan);
    let took = until(&mut sim, seconds(30.0), "ash at the gun", |s| s.sprite_gunner_of(0) == Some(ash as usize));
    assert!(took > seconds(2.0), "it walked there ({took} ticks)");
    let s = settlement(&sim);
    let a = &s.agents[ash as usize];
    assert_ne!(a.body.position, start);
    assert_eq!(a.body.position, deck(s, crawler::GUNNER), "at the gunner's place");
    assert!(sprite(s, ash).sheltered() && s.gun_crew(0) == Some(ash));
    assert_eq!(s.seat(ash as usize), None, "a sprite has no elder's seat");
    // It shoots on its own.
    ghoul(&mut sim, 9001, (-2.0, 5.0), 3, false);
    let mut fired = 0;
    for _ in 0..seconds(3.0) {
        step(&mut sim, &[]);
        fired += shots(settlement(&sim));
    }
    assert!(fired >= 2, "the sprite shot {fired}");
    assert_eq!(settlement(&sim).sprite_gunner_of(0), Some(ash as usize), "and stays");
}

/// A crawler that walks waits for nobody: a sprite on its way stops and
/// waits, planning again, and never climbs aboard while it walks; once it
/// stops, the sprite walks to it and climbs up.
#[test]
fn a_crew_sprite_waits_while_the_crawler_walks() {
    let (mut sim, agents) = lab();
    let ash = ash_of(&sim);
    step(&mut sim, &[set_job(ash, SpriteJob::Crew)]);
    for _ in 0..seconds(1.0) {
        step(&mut sim, &[]);
    }
    assert!(matches!(sprite(settlement(&sim), ash).current(), Some(Step::GoTo(_))), "on its way");
    step(&mut sim, &[south(&agents)]);
    step(&mut sim, &[]);
    let s = settlement(&sim);
    assert!(crawler::walking(&s.vehicles[0]));
    assert_eq!(sprite(s, ash).current(), Some(Step::Wait), "it waits");
    assert!(s.agents[ash as usize].body.route.is_empty());
    let mut planned = 0;
    for _ in 0..seconds(6.0) {
        let before = sprite(settlement(&sim), ash).retry_at;
        step(&mut sim, &[]);
        let s = settlement(&sim);
        assert!(crawler::walking(&s.vehicles[0]), "the pilot holds the stick");
        assert_eq!(sprite(s, ash).aboard(), None, "a walking crawler waits for nobody");
        assert_eq!(sprite(s, ash).current(), Some(Step::Wait));
        planned += usize::from(sprite(s, ash).retry_at != before);
    }
    assert!(planned > 10, "it plans again while it waits ({planned})");
    step(&mut sim, &[stop(&agents)]);
    let took = until(&mut sim, seconds(30.0), "ash at the gun", |s| {
        let aboard = s.sprite_gunner_of(0).is_some();
        assert!(!aboard || !crawler::walking(&s.vehicles[0]));
        aboard
    });
    assert!(took > seconds(1.0), "it walked to where the crawler stopped");
    assert_eq!(settlement(&sim).sprite_gunner_of(0), Some(ash as usize));
}

/// Another job climbs a sprite down beside the crawler's box, from the gun
/// or from a rider's place, and it carries on with its job: idle, it walks
/// home.
#[test]
fn a_job_change_climbs_it_down() {
    let (mut sim, _agents, ash) = manned();
    step(&mut sim, &[set_job(ash, SpriteJob::Idle)]);
    let s = settlement(&sim);
    assert_eq!(sprite(s, ash).aboard(), None, "down");
    assert_eq!(s.gun_crew(0), None);
    let radius = s.agents[ash as usize].radius().to_f64();
    let off = off_box(s, ash);
    assert!(off > radius && off < radius + 0.8, "beside the box: {off}");
    let home = s.agents[ash as usize].body.home;
    let near = |s: &Settlement| (s.agents[ash as usize].body.position - home).length().to_f64();
    let far = near(s);
    for _ in 0..seconds(3.0) {
        step(&mut sim, &[]);
    }
    assert!(near(settlement(&sim)) < far - 1.0, "walking home");

    // From a rider's place too.
    let (mut sim, agents, ash) = manned();
    board(&mut sim, &agents, &[1]);
    assert!(sprite(settlement(&sim), ash).rider.is_some());
    step(&mut sim, &[set_job(ash, SpriteJob::Idle)]);
    let s = settlement(&sim);
    assert_eq!(sprite(s, ash).aboard(), None);
    assert!(off_box(s, ash) < s.agents[ash as usize].radius().to_f64() + 0.8);
}

/// An elder boarding takes the gun from a sprite there (T3): the sprite
/// moves to the free rider's place; the deck then holds one more elder.
/// With both rider's places taken, the sprite climbs down instead, and
/// waits beside the crawler, its gun taken.
#[test]
fn an_elder_bumps_a_sprite_to_a_riders_place_or_down() {
    let (mut sim, agents, ash) = manned();
    board(&mut sim, &agents, &[1]);
    settlement(&sim).validate().unwrap();
    let s = settlement(&sim);
    assert_eq!(s.seat(agents[1] as usize), Some(Seat::Gunner { vehicle: 0 }), "the elder takes the gun");
    assert_eq!(s.sprite_gunner_of(0), None);
    assert_eq!(sprite(s, ash).rider, Some((0, 0)), "the sprite rides in the first place");
    assert_eq!(s.agents[ash as usize].body.position, deck(s, crawler::RIDERS[0]));
    // One more elder fits, in the other place; the next doesn't.
    board(&mut sim, &agents, &[2]);
    board(&mut sim, &agents, &[3]);
    let s = settlement(&sim);
    assert_eq!(s.seat(agents[2] as usize), Some(Seat::Rider { vehicle: 0, place: 1 }));
    assert_eq!(s.seat(agents[3] as usize), None, "the deck is full");
    // It rides, and nothing else happens to it while the crawler walks.
    for _ in 0..seconds(3.0) {
        step(&mut sim, &[south(&agents)]);
        let s = settlement(&sim);
        assert_eq!(sprite(s, ash).rider, Some((0, 0)));
        assert_eq!(s.agents[ash as usize].body.position, deck(s, crawler::RIDERS[0]), "in its place");
        assert!(sprite(s, ash).sheltered());
    }

    // The deck full of elders: the sprite climbs down.
    let (mut sim, agents) = lab();
    let ash = ash_of(&sim);
    board(&mut sim, &agents, &[1, 2, 3]);
    board(&mut sim, &agents, &[1]);
    let s = settlement(&sim);
    assert_eq!(s.seat(agents[1] as usize), None, "the gunner climbed down");
    assert!(s.rider_of(0, 0).is_some() && s.rider_of(0, 1).is_some(), "both places taken");
    assert!(sim.world_mut().settlement_mut().man_gun(ash, 0));
    step(&mut sim, &[]);
    board(&mut sim, &agents, &[1]);
    settlement(&sim).validate().unwrap();
    let s = settlement(&sim);
    assert_eq!(s.seat(agents[1] as usize), Some(Seat::Gunner { vehicle: 0 }));
    assert_eq!(sprite(s, ash).aboard(), None, "the sprite climbed down");
    assert_eq!(sprite(s, ash).job, SpriteJob::Crew, "still on the crew job");
    let off = off_box(s, ash);
    assert!(off > 0.0 && off < s.agents[ash as usize].radius().to_f64() + 0.8, "beside the box: {off}");
    for _ in 0..seconds(2.0) {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        assert_eq!(sprite(s, ash).aboard(), None, "the gun is taken: it waits");
    }
    assert_eq!(sprite(settlement(&sim), ash).current(), Some(Step::Wait));
}

/// When the elder leaves the gun, the sprite still on the crew job takes
/// it back: at once from its rider's place, or by climbing up again from
/// beside the crawler.
#[test]
fn it_takes_the_gun_back_when_the_elder_leaves() {
    let (mut sim, agents, ash) = manned();
    board(&mut sim, &agents, &[1]);
    assert_eq!(sprite(settlement(&sim), ash).rider, Some((0, 0)));
    board(&mut sim, &agents, &[1]);
    settlement(&sim).validate().unwrap();
    let s = settlement(&sim);
    assert_eq!(s.seat(agents[1] as usize), None, "the elder climbed down");
    assert_eq!(s.sprite_gunner_of(0), Some(ash as usize), "the rider took the gun");
    assert_eq!(sprite(s, ash).rider, None);
    assert_eq!(s.agents[ash as usize].body.position, deck(s, crawler::GUNNER));

    // From the ground: the deck full, bumped down, then the elder leaves.
    let (mut sim, agents) = lab();
    let ash = ash_of(&sim);
    board(&mut sim, &agents, &[1, 2, 3]);
    board(&mut sim, &agents, &[1]);
    assert!(sim.world_mut().settlement_mut().man_gun(ash, 0));
    step(&mut sim, &[]);
    board(&mut sim, &agents, &[1]);
    assert_eq!(sprite(settlement(&sim), ash).aboard(), None);
    for _ in 0..seconds(1.0) {
        step(&mut sim, &[]);
    }
    board(&mut sim, &agents, &[1]);
    assert_eq!(settlement(&sim).seat(agents[1] as usize), None);
    until(&mut sim, seconds(10.0), "ash back at the gun", |s| s.sprite_gunner_of(0) == Some(ash as usize));
}

/// A crew sprite killed on its way comes back at home with its job and
/// goes again.
#[test]
fn a_fallen_crew_sprite_comes_back_and_boards() {
    let (mut sim, _agents) = lab();
    let ash = ash_of(&sim);
    step(&mut sim, &[set_job(ash, SpriteJob::Crew)]);
    for _ in 0..seconds(1.0) {
        step(&mut sim, &[]);
    }
    let tick = sim.world().tick();
    let s = sim.world_mut().settlement_mut();
    let respawn = s.characters[s.agents[ash as usize].character as usize].sprite.unwrap().respawn;
    assert!(s.hurt_sprite(ash as usize, 100, tick, respawn), "it falls");
    step(&mut sim, &[]);
    assert_eq!(sprite(settlement(&sim), ash).aboard(), None);
    until(&mut sim, respawn + seconds(30.0), "ash at the gun after coming back", |s| s.sprite_gunner_of(0) == Some(ash as usize));
}

/// Saves, hashes and four threads agree with crew sprites walking, climbing
/// up, bumped and riding; and an elder gunner's client predicts them
/// aboard with a sprite riding.
#[test]
fn a_crew_is_saved_hashed_and_predicted() {
    let (mut sim, agents) = lab();
    let (ash, fern) = (ash_of(&sim), fern(&sim));
    step(&mut sim, &[set_job(ash, SpriteJob::Crew), set_job(fern, SpriteJob::Crew)]);
    for _ in 0..seconds(1.0) {
        step(&mut sim, &[]);
    }
    let world = sim.into_world();
    let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
    let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), world.rules().clone()).unwrap();
    let sprites = |w: &World| -> Vec<SpriteState> { w.settlement().unwrap().agents.iter().filter_map(|a| a.sprite().cloned()).collect() };
    assert_eq!(sprites(&loaded), sprites(&world), "the crew round-trips");
    let threaded = copy(&world);
    let (mut a, mut b, mut c) = (Simulation::from_world(world, 1), Simulation::from_world(loaded, 1), Simulation::from_world(threaded, 4));
    let mut was_riding = false;
    for t in 0..seconds(40.0) {
        // An elder takes the gun at 20 s, the pilot walks south from 22 s
        // to 24 s, and the elder leaves at 30 s.
        let commands: Vec<Command> = match t {
            t if t == seconds(20.0) || t == seconds(30.0) => vec![Command { player: owner(1), kind: CommandKind::Board { agent: agents[1] } }],
            t if (seconds(22.0)..seconds(24.0)).contains(&t) => vec![south(&agents)],
            t if t == seconds(24.0) => vec![stop(&agents)],
            _ => vec![],
        };
        let h = a.step(&commands).total;
        assert_eq!(h, b.step(&commands).total, "the save changed it at {t}");
        assert_eq!(h, c.step(&commands).total, "four threads changed it at {t}");
        let s = settlement(&a);
        s.validate().unwrap();
        was_riding |= [ash, fern].iter().any(|&x| sprite(s, x).rider.is_some());
        if t == seconds(20.0) - 1 {
            assert!(s.sprite_gunner_of(0).is_some(), "a sprite climbed up within 20 s");
        }
    }
    assert!(was_riding, "a sprite was bumped to ride");
    assert!(settlement(&a).sprite_gunner_of(0).is_some(), "and took the gun back");

    // Prediction: an elder at the gun, a sprite riding, the crawler walking.
    let (mut sim, agents, ash) = manned();
    board(&mut sim, &agents, &[1]);
    assert!(sprite(settlement(&sim), ash).rider.is_some());
    step(&mut sim, &[south(&agents)]);
    let gunner = agents[1];
    let confirmed = copy(sim.world());
    let mut replay = confirmed.settlement().unwrap().replay_avatar(gunner, confirmed.tick(), confirmed.terrain()).unwrap();
    let mut ahead = Simulation::from_world(copy(&confirmed), 1);
    for t in 0..120u64 {
        let mine = vec![CommandKind::AimAvatar { agent: gunner, aim: FxVec2::new(fx(-2.0), fx(4.0 + t as f64 / 60.0)) }];
        replay.step_with(mine.iter());
        ahead.step(&mine.into_iter().map(|kind| Command { player: owner(1), kind }).collect::<Vec<_>>());
        let real = settlement(&ahead);
        let (ix, predicted) = replay.vehicle().expect("aboard");
        let v = &real.vehicles[ix as usize];
        assert_eq!((predicted.position, predicted.heading), (v.position, v.heading), "the crawler at {t}");
        assert_eq!(replay.state().body.position, real.agents[gunner as usize].body.position, "the gunner at {t}");
        assert_eq!(sprite(real, ash).rider, Some((0, 0)), "the sprite rides");
    }
}

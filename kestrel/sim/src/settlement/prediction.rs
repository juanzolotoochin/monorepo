//! Client-side prediction: one avatar run ahead of the confirmed world with
//! its owner's commands that haven't applied yet.

use content::scenario::Weapon;

use super::action::{self, Action};
use super::avatar::{self, character_of};
use super::combat::{strike_event, Event};
use super::{AgentIx, AgentState, Role, Settlement};
use crate::messages::{ActionMessages, Messages};
use crate::terrain::Terrain;
use crate::CommandKind;

/// One player avatar replayed tick by tick from a confirmed settlement. It
/// runs the simulation's own per-avatar code on a copy of the avatar and
/// never changes the world.
///
/// Only the avatar's own part of each tick runs: its step (stage 2) and its
/// reading of the world (stage 3), whose messages become the tick's
/// predicted events and are never applied. Pile jobs, transfer units moving
/// at a dock, hits and knockback, damage, terrain changes (its own digs too)
/// and other players stay as confirmed, so a replay matches the simulation
/// exactly while none of those touch the avatar, and its events match while
/// what its strikes read of the world didn't change.
#[derive(Clone)]
pub struct AvatarReplay<'a> {
    settlement: &'a Settlement,
    terrain: &'a Terrain,
    me: AgentIx,
    state: AgentState,
    tick: u64,
    /// The events the last tick run showed.
    events: Vec<Event>,
}

impl Settlement {
    /// Starts replaying `agent` from this settlement, whose next tick is
    /// `tick`. `None` unless `agent` is a player avatar.
    pub fn replay_avatar<'a>(&'a self, agent: AgentIx, tick: u64, terrain: &'a Terrain) -> Option<AvatarReplay<'a>> {
        let state = self.agents.get(agent as usize).filter(|a| a.avatar().is_some())?.clone();
        Some(AvatarReplay { settlement: self, terrain, me: agent, state, tick, events: Vec::new() })
    }
}

impl<'a> AvatarReplay<'a> {
    /// The next tick to run.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The avatar as of `tick`, before that tick runs.
    pub fn state(&self) -> &AgentState {
        &self.state
    }

    /// The confirmed settlement and terrain the replay started from.
    pub fn settlement(&self) -> &'a Settlement {
        self.settlement
    }

    pub fn terrain(&self) -> &'a Terrain {
        self.terrain
    }

    /// The events the last tick run predicted, in the order the simulation
    /// shows them: those built from the avatar's stage 3 messages alone
    /// (`Event::prediction` says `Predict` for exactly these). A strike reads
    /// the confirmed settlement and terrain.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Applies one of the owner's commands on `tick`: its action's part on
    /// the avatar (`action::act`), as `World::step` applies it. Commands that
    /// don't drive this avatar change nothing. Returns false for a pile haul,
    /// which plans its walk from pile lab state the replay doesn't run.
    pub fn apply(&mut self, command: &CommandKind) -> bool {
        let Some((agent, action)) = Action::of(command) else {
            return !matches!(*command, CommandKind::Pile { agent, .. } if agent == self.me);
        };
        if agent == self.me {
            let view = self.settlement.view(self.me, self.tick, self.terrain);
            let def = character_of(&self.settlement.characters, &self.state);
            // What the action does to the world is never applied (D1).
            action::act(&mut self.state, &action, &view, def, &mut ActionMessages::default());
        }
        true
    }

    /// Runs `tick`: the avatar's step (without a pile job), its reading of
    /// the confirmed world and the events that gives, its swing resolving,
    /// and its housekeeping.
    pub fn step(&mut self) {
        let (s, terrain, tick) = (self.settlement, self.terrain, self.tick);
        let def = character_of(&s.characters, &self.state);
        let AgentState { body, role: Role::Avatar(state), .. } = &mut self.state else { unreachable!("replays start from avatars") };
        avatar::self_update(body, state, self.me, tick, terrain, None, def);
        let mut out = Messages::default();
        s.read_avatar(self.me, &self.state, tick, terrain, &mut out);
        // Manual transfer units and pile steps show nothing until stage 5
        // moves them.
        self.events = out.strike.iter().filter_map(strike_event).collect();
        let a = &mut self.state;
        // As `Settlement::strike` would, assuming the swing hits nothing (a
        // bow shot always counts as one hit).
        if let Some(weapon) = out.strike.first().map(|strike| strike.weapon) {
            if let Some(state) = a.avatar_mut() {
                state.attack_ready = tick + s.tuning(weapon).cooldown();
            }
            avatar::swing_resolved(a, tick, u32::from(weapon == Weapon::Bow), def);
        }
        if a.body.alive() {
            avatar::start_queued(a, tick, def);
        }
        avatar::finish_dig(a, tick);
        self.tick += 1;
    }

    /// Applies one tick's commands, in order, and runs it. Returns false if
    /// any of them can't be predicted (see `apply`).
    pub fn step_with<'c>(&mut self, commands: impl IntoIterator<Item = &'c CommandKind>) -> bool {
        let mut predicted = true;
        for command in commands {
            predicted &= self.apply(command);
        }
        self.step();
        predicted
    }
}

#[cfg(test)]
mod tests {
    use content::scenario::Weapon;
    use content::Content;
    use fx::{Fx, FxVec2};

    use super::super::avatar::Intent;
    use super::super::piles::PileAction;
    use super::super::{AgentState, Event, OnMiss, Prediction, SceneSetup, Settlement};
    use crate::terrain::{Destructible, Disc, Surface, Terrain};
    use crate::{Command, CommandKind, PlayerId, Simulation, World};

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }

        fn direction(&mut self) -> FxVec2 {
            let mut c = || Fx::from_ratio(self.below(201) as i64 - 100, 100);
            FxVec2::new(c(), c()).normalize_or_zero()
        }
    }

    /// A small walled arena with a few rocks, every weapon, and `player` as
    /// the avatar.
    fn arena(player: &str, diggable: bool) -> Simulation {
        let mut scenario = Settlement::player_only(Fx::from_int(5), [Fx::ZERO, Fx::ZERO], player);
        scenario.weapons = vec![Weapon::Sword, Weapon::Laser, Weapon::Bow, Weapon::Spear];
        let rock = |x: i64, y: i64, r: i64| Disc { center: FxVec2::new(Fx::from_ratio(x, 10), Fx::from_ratio(y, 10)), radius: Fx::from_ratio(r, 10) };
        let setup = SceneSetup {
            scenario: Some(scenario),
            player: player.into(),
            extent: Fx::from_int(5),
            obstacles: vec![rock(20, 0, 5), rock(-15, 15, 4), rock(0, -25, 6)],
            diggable,
            ..SceneSetup::default()
        };
        Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
    }

    /// Something the player might do on a tick, as the app sends it. Walks
    /// to a point go somewhere within 4.5 u of `center`.
    fn random_command(rng: &mut Rng, agent: u16, center: FxVec2) -> CommandKind {
        let weapons = [Weapon::Sword, Weapon::Laser, Weapon::Bow, Weapon::Spear];
        let muzzle = [Fx::ZERO, Fx::from_ratio(6, 10), Fx::from_ratio(1, 4)];
        match rng.below(24) {
            0..=6 => CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: rng.direction(), run: rng.below(2) == 0 } },
            7..=8 => CommandKind::MoveAvatar { agent, intent: Intent::Stop },
            9..=10 => CommandKind::AimAvatar { agent, aim: rng.direction() },
            11..=13 => CommandKind::Attack { agent, weapon: weapons[rng.below(4) as usize], aim: rng.direction(), muzzle },
            14 => CommandKind::Jump { agent },
            15 => CommandKind::Jetpack { agent, enabled: rng.below(2) == 0 },
            16 => CommandKind::Equip { agent, weapon: weapons[rng.below(4) as usize] },
            17 => CommandKind::ReleaseDirectControl { agent },
            18 => CommandKind::Respawn { agent },
            19..=21 => {
                let target = center + rng.direction() * Fx::from_ratio(rng.below(46) as i64, 10);
                CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: rng.below(2) == 0 } }
            }
            22 => CommandKind::RepairCover { agent, asset: Some(0) },
            _ => CommandKind::Join,
        }
    }

    fn muzzle() -> [Fx; 3] {
        [Fx::ZERO, Fx::from_ratio(6, 10), Fx::from_ratio(1, 4)]
    }

    /// The arena with what a laser stops at: rocks, cover with health to
    /// spare, a ledge above the beam, every weapon, and a second player to
    /// hit. The second player joined on tick 0.
    fn range(player: &str) -> Simulation {
        let mut scenario = Settlement::player_only(Fx::from_int(5), [Fx::ZERO, Fx::ZERO], player);
        scenario.weapons = vec![Weapon::Laser, Weapon::Sword, Weapon::Bow, Weapon::Spear];
        scenario.player_health = 6;
        let tenths = |x: i64, y: i64| FxVec2::new(Fx::from_ratio(x, 10), Fx::from_ratio(y, 10));
        let rock = |x: i64, y: i64, r: i64| Disc { center: tenths(x, y), radius: Fx::from_ratio(r, 10) };
        let cover = |x: i64, y: i64, r: i64| Destructible {
            asset: 0,
            disc: rock(x, y, r),
            health: 10_000,
            max_health: 10_000,
            repairable: false,
            repair_progress: 0,
        };
        let setup = SceneSetup {
            scenario: Some(scenario),
            player: player.into(),
            extent: Fx::from_int(5),
            obstacles: vec![rock(20, 0, 5), rock(0, -25, 6)],
            destructibles: vec![cover(-20, 10, 4), cover(15, 25, 3), cover(-5, -35, 5)],
            surfaces: vec![Surface { min: tenths(-45, -15), max: tenths(-35, 15), height: Fx::from_ratio(3, 2) }],
            ..SceneSetup::default()
        };
        let mut sim = Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1);
        sim.step(&[Command { player: PlayerId(1), kind: CommandKind::Join }]);
        sim
    }

    /// What a strike's targeting reads of everything but the striker: the
    /// other players' positions and heights, who is alive, and which cover
    /// stands.
    fn targets(settlement: &Settlement, terrain: &Terrain, striker: u16) -> (Vec<(FxVec2, Fx)>, Vec<bool>, Vec<bool>) {
        let others = || settlement.agents.iter().enumerate().filter(|&(i, a)| i != striker as usize && a.avatar().is_some()).map(|(_, a)| a);
        (
            others().map(|a| (a.body.position, a.avatar().unwrap().height)).collect(),
            others().map(|a| a.body.health > 0).collect(),
            terrain.destructibles.iter().map(|piece| piece.health > 0).collect(),
        )
    }

    /// The events of `agent` that a replay predicts: those
    /// `Event::prediction` says to.
    fn predictable(settlement: &Settlement, agent: u16) -> Vec<Event> {
        let mine = |event: &&Event| matches!(event.prediction(), Prediction::Predict { agent: a, .. } if a == agent);
        settlement.events.iter().filter(mine).cloned().collect()
    }

    /// The avatar without its laser swing's hit count: the replay assumes a
    /// swing hits nothing, and here a laser can hit the other player. The
    /// count changes nothing else for a laser.
    fn unhit(a: &AgentState) -> AgentState {
        let mut a = a.clone();
        if let Some(swing) = a.avatar_mut().and_then(|avatar| avatar.swing.as_mut()).filter(|swing| swing.weapon == Weapon::Laser) {
            swing.hits = 0;
        }
        a
    }

    /// Each event's kind and prediction key.
    fn keys(events: &[Event]) -> Vec<(std::mem::Discriminant<Event>, Prediction)> {
        events.iter().map(|event| (std::mem::discriminant(event), event.prediction())).collect()
    }

    /// Runs the world on random commands, the avatar often firing the laser
    /// while the second player walks now and then, and replays the avatar
    /// from many confirmed ticks. At every tick of every replay the replayed
    /// avatar equals the simulated one, and its predicted events equal the
    /// simulation's predictable events of that avatar: the same kinds and
    /// keys always, and the same in full whenever what a strike reads of the
    /// rest of the world didn't change in between. Returns how many compared
    /// laser shots ended on cover, on the other player, and short of their
    /// reach on a rock, the ledge or the wall, how many arrows were
    /// compared, and how many laser shots a press queued during a swing.
    fn replay_matches_simulation(player: &str, seed: u64) -> [usize; 5] {
        const TICKS: usize = 1500;
        const AHEAD: usize = 40;
        let mut sim = range(player);
        let (agent, other, center, first) = {
            let s = sim.world().settlement().unwrap();
            (s.player().unwrap(), s.avatar_of(PlayerId(1)).unwrap(), s.spawn, sim.world().tick())
        };
        let mut rng = Rng(seed);
        let schedule: Vec<Vec<CommandKind>> = (0..TICKS + AHEAD)
            .map(|_| match rng.below(10) {
                0 => vec![random_command(&mut rng, agent, center)],
                1 => vec![random_command(&mut rng, agent, center), random_command(&mut rng, agent, center)],
                2 => vec![CommandKind::Attack { agent, weapon: Weapon::Laser, aim: rng.direction(), muzzle: muzzle() }],
                _ => Vec::new(),
            })
            .collect();
        // Index `i`: before tick `first + i` (`confirmed`, `states`), and
        // during it (`events`).
        let mut confirmed: Vec<(Settlement, Terrain)> = Vec::new();
        let mut states = Vec::new();
        let mut events = Vec::new();
        for (i, turn) in schedule.iter().enumerate() {
            let world = sim.world();
            confirmed.push((world.settlement().unwrap().clone(), world.terrain().clone()));
            states.push(world.settlement().unwrap().agents[agent as usize].clone());
            let mut turn: Vec<_> = turn.iter().map(|kind| Command { player: PlayerId(0), kind: kind.clone() }).collect();
            let walk = match i % 150 {
                0 => Some(Intent::Walk { direction: rng.direction(), run: false }),
                20 => Some(Intent::Stop),
                _ => None,
            };
            if let Some(intent) = walk {
                turn.push(Command { player: PlayerId(1), kind: CommandKind::MoveAvatar { agent: other, intent } });
            }
            sim.step(&turn);
            events.push(predictable(sim.world().settlement().unwrap(), agent));
        }
        let world = sim.world();
        confirmed.push((world.settlement().unwrap().clone(), world.terrain().clone()));
        states.push(world.settlement().unwrap().agents[agent as usize].clone());
        let reach = world.settlement().unwrap().tuning(Weapon::Laser).reach;
        let (mut swings, mut routed, mut compared, mut skipped) = (0, 0, 0, 0);
        let mut ends = [0; 5];
        for start in (0..TICKS).step_by(3) {
            let (settlement, terrain) = &confirmed[start];
            let mut replay = settlement.replay_avatar(agent, first + start as u64, terrain).unwrap();
            for i in start..start + AHEAD {
                let tick = first + i as u64;
                let context = || format!("{player}, seed {seed}: replayed from tick {}, tick {tick}", first + start as u64);
                assert!(replay.step_with(&schedule[i]));
                assert_eq!(unhit(replay.state()), unhit(&states[i + 1]), "{}: the avatar differs ({:?})", context(), &schedule[start..=i]);
                swings += usize::from(replay.state().avatar().unwrap().swing.as_ref().is_some_and(|s| s.resolved));
                routed += usize::from(matches!(replay.state().avatar().unwrap().intent, Some(Intent::GoTo { .. })) && !replay.state().body.route.is_empty());
                let (mine, real) = (replay.events(), events[i].as_slice());
                assert_eq!(keys(mine), keys(real), "{}: predicted {mine:?}, simulated {real:?}", context());
                let before = targets(settlement, terrain, agent);
                let at = targets(&confirmed[i].0, &confirmed[i].1, agent);
                let after = targets(&confirmed[i + 1].0, &confirmed[i + 1].1, agent);
                if before != at || at.0 != after.0 {
                    skipped += mine.len();
                    continue;
                }
                assert_eq!(mine, real, "{}", context());
                for event in mine {
                    match *event {
                        Event::LaserFired { from, to, hit_actor, cover, started, .. } => {
                            compared += 1;
                            ends[0] += usize::from(cover.is_some());
                            ends[1] += usize::from(hit_actor);
                            ends[2] += usize::from(cover.is_none() && !hit_actor && (to - from).length() < reach - Fx::from_ratio(1, 100));
                            // A laser swing resolves 2 ticks after it starts.
                            ends[4] += usize::from(started + 2 < tick);
                        }
                        Event::BowFired { .. } => ends[3] += 1,
                        _ => {}
                    }
                }
            }
        }
        println!("{player}, seed {seed}: {compared} laser shots compared, ends {ends:?} (cover, player, elsewhere, arrows, queued); {skipped} events skipped");
        assert!(swings > 0, "no swing resolved during a replay");
        assert!(routed > 0, "no replay walked a planned route");
        assert!(compared > 100, "{player}, seed {seed}: only {compared} laser shots compared");
        ends
    }

    /// Runs three seeds; between them, shots end on cover, on the other
    /// player, and short of their reach on a rock, the ledge or the wall,
    /// arrows are loosed, and a queued press fires.
    fn replay_matches_simulation_on_every_target(player: &str) {
        let ends = (1..=3).map(|seed| replay_matches_simulation(player, seed)).fold([0; 5], |a, b| [0, 1, 2, 3, 4].map(|k| a[k] + b[k]));
        assert!(ends.iter().all(|&n| n > 0), "{player}: compared shots ending on cover, a player, elsewhere, arrows, queued: {ends:?}");
    }

    #[test]
    fn replayed_avatar_matches_the_simulation_with_a_jetpack() {
        replay_matches_simulation_on_every_target("exosuit");
    }

    #[test]
    fn replayed_avatar_matches_the_simulation_with_jumps() {
        replay_matches_simulation_on_every_target("exosuit_moonlit");
    }

    #[test]
    fn replay_holds_a_dig_but_leaves_the_ground_to_the_simulation() {
        let mut sim = arena("exosuit", true);
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let (settlement, terrain) = (sim.world().settlement().unwrap().clone(), sim.world().terrain().clone());
        let mut replay = settlement.replay_avatar(agent, 0, &terrain).unwrap();
        let sword = CommandKind::Attack { agent, weapon: Weapon::Sword, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3] };
        let turns = [vec![CommandKind::DigDown { agent }], vec![CommandKind::MoveAvatar { agent, intent: Intent::Stop }], vec![sword]];
        let step = |sim: &mut Simulation, replay: &mut super::AvatarReplay| {
            let turn = turns.get(sim.world().tick() as usize).cloned().unwrap_or_default();
            assert!(replay.step_with(&turn));
            let turn: Vec<_> = turn.into_iter().map(|kind| Command { player: PlayerId(0), kind }).collect();
            sim.step(&turn);
        };
        loop {
            step(&mut sim, &mut replay);
            let real = &sim.world().settlement().unwrap().agents[agent as usize];
            if real.avatar().unwrap().dig.is_none() {
                break;
            }
            assert_eq!(replay.state(), real, "tick {}: a stop or an attack doesn't end a dig", sim.world().tick() - 1);
        }
        assert!(sim.world().tick() > 3 && replay.state().avatar().unwrap().dig.is_none());
        // The layer came out: on the next tick the simulation lowers the
        // avatar into the pit; the replay never changes the ground.
        step(&mut sim, &mut replay);
        let real = &sim.world().settlement().unwrap().agents[agent as usize];
        assert_eq!(replay.state().body.position, real.body.position);
        assert!(real.avatar().unwrap().height < replay.state().avatar().unwrap().height);
    }

    #[test]
    fn pile_hauls_and_other_avatars_are_left_alone() {
        let sim = arena("exosuit", false);
        let world = sim.world();
        let settlement = world.settlement().unwrap();
        let agent = settlement.player().unwrap();
        let mut replay = settlement.replay_avatar(agent, world.tick(), world.terrain()).unwrap();
        let haul = CommandKind::Pile { agent, action: PileAction::Deliver, pace: Fx::ZERO, approach: None };
        assert!(!replay.apply(&haul), "a pile haul is not predicted");
        let goto = CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target: settlement.spawn + FxVec2::new(Fx::ONE, Fx::ZERO), run: false } };
        assert!(replay.apply(&goto), "a GoTo walk is predicted");
        assert!(!replay.state().body.route.is_empty());
        let piano = CommandKind::PlayPiano { agent, piano: 0, pitch: 60, velocity: 100 };
        assert!(replay.apply(&piano));
        let other = CommandKind::Jetpack { agent: agent + 1, enabled: true };
        assert!(replay.apply(&other));
        assert!(!replay.state().avatar().unwrap().flight.requested);
        assert!(settlement.replay_avatar(agent + 1, 0, world.terrain()).is_none());
    }

    #[test]
    fn a_sword_swing_predicts_no_event() {
        let sim = range("exosuit");
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let sword = CommandKind::Attack { agent, weapon: Weapon::Sword, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3] };
        let world = sim.world();
        let mut replay = world.settlement().unwrap().replay_avatar(agent, world.tick(), world.terrain()).unwrap();
        replay.step_with([&sword]);
        for _ in 0..30 {
            replay.step();
            assert_eq!(replay.events(), []);
        }
        let swing = replay.state().avatar().unwrap().swing.as_ref();
        assert!(swing.is_some_and(|s| s.weapon == Weapon::Sword && s.resolved), "the sword swing resolved");
    }

    /// A press buffered during a swing starts its own swing later, but its
    /// shot is keyed by the tick it was pressed on, in the simulation and in
    /// the replay alike.
    #[test]
    fn a_queued_attack_is_keyed_by_its_press() {
        let mut sim = range("exosuit");
        let world = sim.world();
        let agent = world.settlement().unwrap().player().unwrap();
        let first = world.tick();
        let (settlement, terrain) = (world.settlement().unwrap().clone(), world.terrain().clone());
        let mut replay = settlement.replay_avatar(agent, first, &terrain).unwrap();
        let laser = |y: i32| CommandKind::Attack { agent, weapon: Weapon::Laser, aim: FxVec2::new(Fx::ONE, Fx::from_int(y)), muzzle: muzzle() };
        let mut shots = Vec::new();
        // The first shot fires 2 ticks in; the laser is ready again 24
        // ticks later. A press at 20 swings without firing, until 28; the
        // press at 24 waits for that swing, swings from 28 and fires at 30.
        for tick in first..first + 40 {
            let turn = match tick - first {
                0 => vec![laser(0)],
                20 => vec![laser(1)],
                24 => vec![laser(-1)],
                _ => Vec::new(),
            };
            replay.step_with(&turn);
            sim.step(&turn.iter().map(|kind| Command { player: PlayerId(0), kind: kind.clone() }).collect::<Vec<_>>());
            let real = predictable(sim.world().settlement().unwrap(), agent);
            assert_eq!(replay.events(), real, "tick {tick}");
            shots.extend(real.iter().map(|event| (tick, event.prediction())));
            if tick == first + 30 {
                let swing = sim.world().settlement().unwrap().agents[agent as usize].avatar().unwrap().swing.clone().unwrap();
                assert_eq!((swing.started, swing.pressed()), (first + 28, first + 24));
                assert_eq!(replay.state().avatar().unwrap().swing, Some(swing));
            }
        }
        let fired = |delay: u64, pressed: u64| (first + delay, Prediction::Predict { agent, started: first + pressed, on_miss: OnMiss::Keep });
        assert_eq!(shots, [fired(2, 0), fired(30, 24)]);
    }
}

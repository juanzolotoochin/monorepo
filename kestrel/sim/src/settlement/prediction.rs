//! Client-side prediction: one avatar run ahead of the confirmed world with
//! its owner's commands that haven't applied yet.

use content::scenario::Weapon;
use fx::{Fx, FxVec2};

use super::action::Action;
use super::avatar::{self, character_of};
use super::{AgentIx, AgentState, Settlement};
use crate::terrain::Terrain;
use crate::CommandKind;

/// One player avatar replayed tick by tick from a confirmed settlement. It
/// runs the simulation's own per-avatar code on a copy of the avatar and
/// never changes the world.
///
/// Only the avatar's own part of each tick runs. Pile jobs, transfer units
/// moving at a dock, hits and knockback, damage, terrain changes (its own
/// digs too) and other players stay as confirmed, so a replay matches the
/// simulation exactly while none of those touch the avatar.
#[derive(Clone)]
pub struct AvatarReplay<'a> {
    settlement: &'a Settlement,
    terrain: &'a Terrain,
    me: AgentIx,
    state: AgentState,
    tick: u64,
    /// The strike that resolved on the last tick run.
    struck: Option<ReplayedStrike>,
}

/// A swing the replayed avatar resolved: what the simulation's targeting
/// reads when the swing resolves (`read_avatar`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayedStrike {
    /// The tick it resolved on.
    pub tick: u64,
    pub weapon: Weapon,
    pub aim: FxVec2,
    pub muzzle: [Fx; 3],
    /// The avatar's position and height after its own step on `tick`.
    pub origin: FxVec2,
    pub height: Fx,
}

/// A laser shot as the simulation fires it: `Event::LaserFired`'s fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PredictedLaser {
    pub from: FxVec2,
    pub to: FxVec2,
    pub hit_actor: bool,
    pub cover: Option<u16>,
}

impl Settlement {
    /// Starts replaying `agent` from this settlement, whose next tick is
    /// `tick`. `None` unless `agent` is a player avatar.
    pub fn replay_avatar<'a>(&'a self, agent: AgentIx, tick: u64, terrain: &'a Terrain) -> Option<AvatarReplay<'a>> {
        let state = self.agents.get(agent as usize).filter(|a| a.controlled)?.clone();
        Some(AvatarReplay { settlement: self, terrain, me: agent, state, tick, struck: None })
    }

    /// The laser shot `agent` would fire with `strike` against this
    /// settlement: the simulation's own targeting (`strike_targets`) from
    /// the strike's position and height, whatever this settlement says about
    /// the avatar itself. `None` unless the strike is a laser's.
    pub fn predict_laser(&self, agent: AgentIx, strike: &ReplayedStrike, terrain: &Terrain) -> Option<PredictedLaser> {
        if strike.weapon != Weapon::Laser {
            return None;
        }
        let (targets, beam_end, cover) = self.strike_targets_from(agent, strike.height, strike.origin, strike.aim, Weapon::Laser, terrain);
        let to = beam_end?;
        let hit_actor = self.laser_hits_actor(agent as usize, &targets, to);
        Some(PredictedLaser { from: strike.origin, to, hit_actor, cover })
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

    /// The strike that resolved on the last tick run, if any.
    pub fn struck(&self) -> Option<&ReplayedStrike> {
        self.struck.as_ref()
    }

    /// The laser shot the last tick run fired, aimed against the confirmed
    /// settlement (see `Settlement::predict_laser`).
    pub fn laser(&self) -> Option<PredictedLaser> {
        self.settlement.predict_laser(self.me, self.struck.as_ref()?, self.terrain)
    }

    /// Applies one of the owner's commands on `tick`: its action's part on
    /// the avatar (`avatar::act`), as `World::step` applies it. Commands that
    /// don't drive this avatar change nothing. Returns false for a pile haul,
    /// which plans its walk from pile lab state the replay doesn't run.
    pub fn apply(&mut self, command: &CommandKind) -> bool {
        let Some((agent, action)) = Action::of(command) else {
            return !matches!(*command, CommandKind::Pile { agent, .. } if agent == self.me);
        };
        if agent == self.me {
            let view = self.settlement.view(self.me, self.tick, self.terrain);
            let def = character_of(&self.settlement.characters, &self.state);
            avatar::act(&mut self.state, &action, &view, def);
        }
        true
    }

    /// Runs `tick`: the avatar's step (without a pile job), its swing
    /// resolving, and its housekeeping.
    pub fn step(&mut self) {
        let (s, terrain, tick) = (self.settlement, self.terrain, self.tick);
        let a = &mut self.state;
        let def = character_of(&s.characters, a);
        avatar::self_update(a, self.me, tick, terrain, None, def);
        self.struck = avatar::resolving(a, tick).map(|swing| ReplayedStrike {
            tick,
            weapon: swing.weapon,
            aim: swing.aim,
            muzzle: swing.muzzle,
            origin: a.position,
            height: a.avatar.height,
        });
        // As `Settlement::strike` would, assuming the swing hits nothing (a
        // bow shot always counts as one hit).
        if let Some(weapon) = self.struck.as_ref().map(|strike| strike.weapon) {
            a.attack_ready = tick + s.tuning(weapon).cooldown();
            avatar::swing_resolved(a, tick, u32::from(weapon == Weapon::Bow), def);
        }
        if avatar::alive(a) {
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
    use super::super::{Event, SceneSetup, Settlement};
    use super::PredictedLaser;
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

    /// Runs the world on random commands and replays the avatar from many
    /// confirmed ticks: at every tick of every replay, the replayed avatar
    /// equals the simulated one.
    fn replay_matches_simulation(player: &str, seed: u64) {
        const TICKS: usize = 1500;
        const AHEAD: usize = 40;
        let mut sim = arena(player, false);
        let (agent, center) = {
            let s = sim.world().settlement().unwrap();
            (s.player().unwrap(), s.spawn)
        };
        let mut rng = Rng(seed);
        let schedule: Vec<Vec<CommandKind>> = (0..TICKS + AHEAD)
            .map(|_| match rng.below(10) {
                0 => vec![random_command(&mut rng, agent, center)],
                1 => vec![random_command(&mut rng, agent, center), random_command(&mut rng, agent, center)],
                _ => Vec::new(),
            })
            .collect();
        let mut confirmed: Vec<(Settlement, Terrain)> = Vec::new();
        let mut states = Vec::new();
        for turn in &schedule {
            let world = sim.world();
            confirmed.push((world.settlement().unwrap().clone(), world.terrain().clone()));
            states.push(world.settlement().unwrap().agents[agent as usize].clone());
            let turn: Vec<_> = turn.iter().map(|kind| Command { player: PlayerId(0), kind: kind.clone() }).collect();
            sim.step(&turn);
        }
        states.push(sim.world().settlement().unwrap().agents[agent as usize].clone());
        let (mut swings, mut routed) = (0, 0);
        for start in (0..TICKS).step_by(5) {
            let (settlement, terrain) = &confirmed[start];
            let mut replay = settlement.replay_avatar(agent, start as u64, terrain).unwrap();
            for tick in start..start + AHEAD {
                assert!(replay.step_with(&schedule[tick]));
                assert_eq!(
                    replay.state(),
                    &states[tick + 1],
                    "{player}, seed {seed}: replayed from tick {start}, differs after tick {tick} ({:?})",
                    &schedule[start..=tick]
                );
                swings += usize::from(replay.state().avatar.swing.as_ref().is_some_and(|s| s.resolved));
                routed += usize::from(matches!(replay.state().avatar.intent, Some(Intent::GoTo { .. })) && !replay.state().route.is_empty());
            }
        }
        assert!(swings > 0, "no swing resolved during a replay");
        assert!(routed > 0, "no replay walked a planned route");
    }

    #[test]
    fn replayed_avatar_matches_the_simulation_with_a_jetpack() {
        for seed in 1..=3 {
            replay_matches_simulation("exosuit", seed);
        }
    }

    #[test]
    fn replayed_avatar_matches_the_simulation_with_jumps() {
        for seed in 1..=3 {
            replay_matches_simulation("exosuit_moonlit", seed);
        }
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
            if real.avatar.dig.is_none() {
                break;
            }
            assert_eq!(replay.state(), real, "tick {}: a stop or an attack doesn't end a dig", sim.world().tick() - 1);
        }
        assert!(sim.world().tick() > 3 && replay.state().avatar.dig.is_none());
        // The layer came out: on the next tick the simulation lowers the
        // avatar into the pit; the replay never changes the ground.
        step(&mut sim, &mut replay);
        let real = &sim.world().settlement().unwrap().agents[agent as usize];
        assert_eq!(replay.state().position, real.position);
        assert!(real.avatar.height < replay.state().avatar.height);
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
        assert!(!replay.state().route.is_empty());
        let piano = CommandKind::PlayPiano { agent, piano: 0, pitch: 60, velocity: 100 };
        assert!(replay.apply(&piano));
        let other = CommandKind::Jetpack { agent: agent + 1, enabled: true };
        assert!(replay.apply(&other));
        assert!(!replay.state().avatar.flight.requested);
        assert!(settlement.replay_avatar(agent + 1, 0, world.terrain()).is_none());
    }

    /// The arena with what a laser stops at: rocks, cover with health to
    /// spare, a ledge above the beam, and a second player to hit.
    fn laser_range(player: &str) -> Simulation {
        let mut scenario = Settlement::player_only(Fx::from_int(5), [Fx::ZERO, Fx::ZERO], player);
        scenario.weapons = vec![Weapon::Laser, Weapon::Sword];
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

    /// What the laser's targeting reads of everything but the shooter:
    /// the other players' positions and heights, who is alive, and which
    /// cover stands.
    fn targets(settlement: &Settlement, terrain: &Terrain, shooter: u16) -> (Vec<(FxVec2, Fx)>, Vec<bool>, Vec<bool>) {
        let others = || settlement.agents.iter().enumerate().filter(|&(i, a)| i != shooter as usize && a.controlled).map(|(_, a)| a);
        (
            others().map(|a| (a.position, a.avatar.height)).collect(),
            others().map(|a| a.avatar.health > 0).collect(),
            terrain.destructibles.iter().map(|piece| piece.health > 0).collect(),
        )
    }

    /// The shooter fires the laser in random directions while walking,
    /// jumping or flying; the second player walks a little now and then.
    /// Every predicted shot (the replay's strike, aimed against the
    /// settlement the replay started from) equals the simulation's
    /// `LaserFired` whenever what the targeting reads of the rest of the
    /// world didn't change in between.
    fn predicted_laser_matches_simulation(player: &str, seed: u64) -> [usize; 3] {
        const TICKS: usize = 1200;
        const AHEAD: usize = 30;
        let mut sim = laser_range(player);
        let (shooter, other, center) = {
            let s = sim.world().settlement().unwrap();
            (s.player().unwrap(), s.avatar_of(PlayerId(1)).unwrap(), s.spawn)
        };
        let mut rng = Rng(seed);
        let muzzle = [Fx::ZERO, Fx::from_ratio(6, 10), Fx::from_ratio(1, 4)];
        let mine: Vec<Vec<CommandKind>> = (0..TICKS + AHEAD)
            .map(|_| match rng.below(24) {
                0..=2 => vec![CommandKind::Attack { agent: shooter, weapon: Weapon::Laser, aim: rng.direction(), muzzle }],
                3 => vec![CommandKind::MoveAvatar { agent: shooter, intent: Intent::Walk { direction: rng.direction(), run: rng.below(2) == 0 } }],
                4 => vec![CommandKind::MoveAvatar { agent: shooter, intent: Intent::Stop }],
                5 => vec![CommandKind::Jump { agent: shooter }],
                6 => vec![CommandKind::Jetpack { agent: shooter, enabled: rng.below(2) == 0 }],
                7 => vec![random_command(&mut rng, shooter, center)],
                _ => Vec::new(),
            })
            .collect();
        let mut confirmed: Vec<(Settlement, Terrain)> = Vec::new();
        let mut fired = Vec::new();
        for (tick, turn) in mine.iter().enumerate() {
            let world = sim.world();
            let start = world.tick() as usize;
            assert_eq!(start, tick + 1, "the join took tick 0");
            confirmed.push((world.settlement().unwrap().clone(), world.terrain().clone()));
            let mut turn: Vec<_> = turn.iter().map(|kind| Command { player: PlayerId(0), kind: kind.clone() }).collect();
            let walk = match tick % 150 {
                0 => Some(Intent::Walk { direction: rng.direction(), run: false }),
                20 => Some(Intent::Stop),
                _ => None,
            };
            if let Some(intent) = walk {
                turn.push(Command { player: PlayerId(1), kind: CommandKind::MoveAvatar { agent: other, intent } });
            }
            sim.step(&turn);
            let shots: Vec<_> = sim.world().settlement().unwrap().events.iter().filter_map(|event| match *event {
                Event::LaserFired { agent, from, to, hit_actor, cover } if agent == shooter => Some(PredictedLaser { from, to, hit_actor, cover }),
                _ => None,
            }).collect();
            assert!(shots.len() <= 1);
            fired.push(shots.first().copied());
        }
        let world = sim.world();
        confirmed.push((world.settlement().unwrap().clone(), world.terrain().clone()));
        // `confirmed[i]` and `fired[i]` are before and during tick `i + 1`.
        let (mut compared, mut skipped, mut cover, mut actor, mut blocked) = (0, 0, 0, 0, 0);
        let mut predicted = vec![false; TICKS];
        let reach = sim.world().settlement().unwrap().tuning(Weapon::Laser).reach;
        for start in (0..TICKS).step_by(3) {
            let (settlement, terrain) = &confirmed[start];
            let mut replay = settlement.replay_avatar(shooter, start as u64 + 1, terrain).unwrap();
            for i in start..(start + AHEAD).min(TICKS) {
                assert!(replay.step_with(&mine[i]));
                let Some(shot) = replay.laser() else {
                    assert_eq!(fired[i], None, "{player}, seed {seed}: tick {} fired, the replay from {} didn't", i + 1, start + 1);
                    continue;
                };
                assert_eq!(replay.struck().unwrap().tick, i as u64 + 1);
                let real = fired[i].unwrap_or_else(|| panic!("{player}, seed {seed}: the replay fired on tick {}, the simulation didn't", i + 1));
                predicted[i] = true;
                let before = targets(settlement, terrain, shooter);
                let at = targets(&confirmed[i].0, &confirmed[i].1, shooter);
                let after = targets(&confirmed[i + 1].0, &confirmed[i + 1].1, shooter);
                if before != at || at.0 != after.0 {
                    skipped += 1;
                    continue;
                }
                assert_eq!(shot, real, "{player}, seed {seed}: shot on tick {} predicted from tick {}", i + 1, start + 1);
                compared += 1;
                cover += usize::from(shot.cover.is_some());
                actor += usize::from(shot.hit_actor);
                let strike = replay.struck().unwrap();
                blocked += usize::from(shot.cover.is_none() && !shot.hit_actor && shot.to != strike.origin + strike.aim * reach);
            }
        }
        let shots = fired[..TICKS].iter().flatten().count();
        assert_eq!(predicted.iter().filter(|p| **p).count(), shots, "every shot was predicted");
        println!("{player}, seed {seed}: {shots} shots, {compared} predictions compared ({cover} cover, {actor} player, {blocked} rock, ledge or wall), {skipped} skipped");
        assert!(compared > 100, "{player}, seed {seed}: only {compared} predictions compared");
        [cover, actor, blocked]
    }

    /// Runs three seeds; between them, shots end on cover, on the other
    /// player, and short of their reach on a rock, the ledge or the wall.
    fn predicted_laser_matches_simulation_on_every_target(player: &str) {
        let ends = (1..=3).map(|seed| predicted_laser_matches_simulation(player, seed)).fold([0; 3], |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2]]);
        assert!(ends.iter().all(|&n| n > 0), "{player}: compared shots ending on cover, a player, elsewhere: {ends:?}");
    }

    #[test]
    fn predicted_laser_matches_the_simulation_with_a_jetpack() {
        predicted_laser_matches_simulation_on_every_target("exosuit");
    }

    #[test]
    fn predicted_laser_matches_the_simulation_with_jumps() {
        predicted_laser_matches_simulation_on_every_target("exosuit_moonlit");
    }

    #[test]
    fn only_a_laser_strike_predicts_a_shot() {
        let sim = laser_range("exosuit");
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let sword = CommandKind::Attack { agent, weapon: Weapon::Sword, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3] };
        let world = sim.world();
        let (settlement, terrain) = (world.settlement().unwrap().clone(), world.terrain().clone());
        let mut replay = settlement.replay_avatar(agent, world.tick(), &terrain).unwrap();
        replay.step_with([&sword]);
        let mut struck = None;
        for _ in 0..30 {
            replay.step();
            struck = struck.or(replay.struck().cloned());
            assert_eq!(replay.laser(), None);
        }
        let struck = struck.expect("the sword swing resolved");
        assert_eq!(struck.weapon, Weapon::Sword);
        assert_eq!(settlement.predict_laser(agent, &struck, &terrain), None);
    }
}

//! Client-side prediction: one avatar run ahead of the confirmed world with
//! its owner's commands that haven't applied yet.

use content::scenario::Weapon;

use super::avatar::{self, character_of, Intent};
use super::{AgentIx, AgentState, Settlement};
use crate::terrain::Terrain;
use crate::CommandKind;

/// One player avatar replayed tick by tick from a confirmed settlement. It
/// runs the simulation's own per-avatar code on a copy of the avatar and
/// never changes the world.
///
/// Only the avatar's own part of each tick runs. Pile jobs, hits and
/// knockback, damage, terrain changes (its own digs too) and other players
/// stay as confirmed, so a replay matches the simulation exactly while none
/// of those touch the avatar.
#[derive(Clone)]
pub struct AvatarReplay<'a> {
    settlement: &'a Settlement,
    terrain: &'a Terrain,
    me: AgentIx,
    state: AgentState,
    tick: u64,
}

impl Settlement {
    /// Starts replaying `agent` from this settlement, whose next tick is
    /// `tick`. `None` unless `agent` is a player avatar.
    pub fn replay_avatar<'a>(&'a self, agent: AgentIx, tick: u64, terrain: &'a Terrain) -> Option<AvatarReplay<'a>> {
        let state = self.agents.get(agent as usize).filter(|a| a.controlled)?.clone();
        Some(AvatarReplay { settlement: self, terrain, me: agent, state, tick })
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

    /// Applies one of the owner's commands on `tick`, as `World::step` does
    /// for this avatar. Commands that don't drive it change nothing. Returns
    /// false for commands that plan a route (click-to-move, pile hauls),
    /// which the replay doesn't predict.
    pub fn apply(&mut self, command: &CommandKind) -> bool {
        if command.avatar() != Some(self.me) {
            return true;
        }
        let (s, terrain, tick) = (self.settlement, self.terrain, self.tick);
        let a = &mut self.state;
        let def = character_of(&s.characters, a);
        match *command {
            CommandKind::MoveAvatar { intent: Intent::GoTo { .. }, .. } | CommandKind::Pile { .. } => return false,
            CommandKind::MoveAvatar { ref intent, .. } => {
                if avatar::alive(a) {
                    avatar::set_intent(a, intent.clone(), terrain, def);
                }
            }
            CommandKind::AimAvatar { aim, .. } => avatar::aim_at(a, aim),
            CommandKind::Attack { weapon, aim, muzzle, .. } => {
                if s.has_weapon(weapon) && avatar::alive(a) {
                    avatar::attack(a, tick, weapon, aim, muzzle, def);
                }
            }
            CommandKind::Equip { weapon, .. } => {
                if s.has_weapon(weapon) {
                    a.avatar.weapon = weapon;
                }
            }
            CommandKind::Jetpack { enabled, .. } => avatar::jetpack(a, enabled, def),
            CommandKind::Jump { .. } => avatar::jump(a, tick, def),
            CommandKind::DigDown { .. } => {
                if s.can_dig_at(a.position, terrain) {
                    avatar::dig(a, tick, terrain, def);
                }
            }
            CommandKind::ReleaseDirectControl { .. } => {
                let hauling = s.piles.as_ref().is_some_and(|lab| lab.hauler(self.me).is_some_and(|h| h.job.is_some()));
                avatar::release(a, hauling);
            }
            CommandKind::Respawn { .. } => avatar::respawn(a, s.spawn, s.spawn_height),
            _ => {}
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
        // As `Settlement::strike` would, assuming the swing hits nothing (a
        // bow shot always counts as one hit).
        if let Some(weapon) = avatar::resolving(a, tick).map(|swing| swing.weapon) {
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
    use super::super::{SceneSetup, Settlement};
    use crate::terrain::{Disc, Terrain};
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

    /// Something the player might do on a tick, as the app sends it.
    fn random_command(rng: &mut Rng, agent: u16) -> CommandKind {
        let weapons = [Weapon::Sword, Weapon::Laser, Weapon::Bow, Weapon::Spear];
        let muzzle = [Fx::ZERO, Fx::from_ratio(6, 10), Fx::from_ratio(1, 4)];
        match rng.below(20) {
            0..=6 => CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: rng.direction(), run: rng.below(2) == 0 } },
            7..=8 => CommandKind::MoveAvatar { agent, intent: Intent::Stop },
            9..=10 => CommandKind::AimAvatar { agent, aim: rng.direction() },
            11..=13 => CommandKind::Attack { agent, weapon: weapons[rng.below(4) as usize], aim: rng.direction(), muzzle },
            14 => CommandKind::Jump { agent },
            15 => CommandKind::Jetpack { agent, enabled: rng.below(2) == 0 },
            16 => CommandKind::Equip { agent, weapon: weapons[rng.below(4) as usize] },
            17 => CommandKind::ReleaseDirectControl { agent },
            18 => CommandKind::Respawn { agent },
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
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let mut rng = Rng(seed);
        let schedule: Vec<Vec<CommandKind>> = (0..TICKS + AHEAD)
            .map(|_| match rng.below(10) {
                0 => vec![random_command(&mut rng, agent)],
                1 => vec![random_command(&mut rng, agent), random_command(&mut rng, agent)],
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
        let mut swings = 0;
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
            }
        }
        assert!(swings > 0, "no swing resolved during a replay");
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
    fn click_to_move_and_other_avatars_are_left_alone() {
        let sim = arena("exosuit", false);
        let world = sim.world();
        let settlement = world.settlement().unwrap();
        let agent = settlement.player().unwrap();
        let mut replay = settlement.replay_avatar(agent, world.tick(), world.terrain()).unwrap();
        let goto = CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target: FxVec2::ZERO, run: false } };
        assert!(!replay.apply(&goto), "click-to-move is not predicted");
        let other = CommandKind::Jetpack { agent: agent + 1, enabled: true };
        assert!(replay.apply(&other));
        assert!(!replay.state().avatar.flight.requested);
        assert!(settlement.replay_avatar(agent + 1, 0, world.terrain()).is_none());
    }
}

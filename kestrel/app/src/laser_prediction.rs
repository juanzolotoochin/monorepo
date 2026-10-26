//! Predicted laser shots of a joined player's own avatar.
//!
//! The avatar is drawn about an input lead ahead of the confirmed world
//! (`prediction`). When the drawn time reaches the tick a laser swing
//! resolves on in the replay, the shot is aimed with the simulation's own
//! targeting against the confirmed world and drawn at once: beam, laser
//! sound, impact spark and impact sound. Damage, flinches and cover breaking
//! wait for the confirmed `LaserFired`, which is matched to the drawn shot
//! in order and not drawn again. A confirmed shot that ends elsewhere, or a
//! drawn one that never fires, is a laser misprediction.

use std::collections::VecDeque;

use bevy::prelude::Resource;
use fx::{Fx, FxVec2};
use sim::settlement::{PredictedLaser, ReplayedStrike};

/// A drawn shot the confirmed world is this many ticks past without firing
/// it is given up. The confirmed world reaches the predicted tick about an
/// input lead after the shot was drawn; while the replay still predicts the
/// shot (an input the host moved later) its tick moves with it.
pub const TIMEOUT_TICKS: u64 = 10;

#[derive(Resource, Default)]
pub struct LaserPrediction {
    /// The (world, agent) predicted; anything else starts over.
    key: Option<(u32, u16)>,
    /// Drawn shots the confirmed world hasn't fired yet, oldest first.
    outstanding: VecDeque<Drawn>,
    next_id: u64,
    /// For each of the avatar's confirmed shots this frame, in order: the
    /// drawn shot it matched, which already showed it.
    pub confirmed: VecDeque<Option<u64>>,
}

struct Drawn {
    id: u64,
    /// The tick it is predicted to fire on.
    tick: u64,
    shot: PredictedLaser,
    /// Frame time it was drawn at, in seconds.
    at: f64,
    /// The `Attack` command's aim and muzzle, which tell its strike apart
    /// from another command's.
    aim: FxVec2,
    muzzle: [Fx; 3],
}

impl Drawn {
    fn from_same_command(&self, strike: &ReplayedStrike) -> bool {
        (self.aim, self.muzzle) == (strike.aim, strike.muzzle)
    }
}

/// What a frame's replay changed.
#[derive(Debug, Default, PartialEq)]
pub struct Predicted {
    /// Shots to draw now.
    pub new: Vec<NewShot>,
    /// Drawn shots the replay no longer resolves.
    pub dropped: Vec<Outcome>,
}

/// A shot to draw now.
#[derive(Debug, PartialEq)]
pub struct NewShot {
    pub id: u64,
    pub tick: u64,
    pub shot: PredictedLaser,
}

/// How a drawn shot turned out.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The confirmed world fired it, `lead` seconds after it was drawn.
    /// `exact` when it ended where it was drawn ending.
    Fired { predicted: FxVec2, confirmed: FxVec2, lead: f64, exact: bool },
    /// The confirmed world never fired it.
    NeverFired { predicted: FxVec2 },
}

impl LaserPrediction {
    pub fn reset(&mut self) {
        *self = LaserPrediction::default();
    }

    /// Starts a frame predicting `agent` in world `world`.
    pub fn begin(&mut self, world: u32, agent: u16) {
        if self.key != Some((world, agent)) {
            self.reset();
            self.key = Some((world, agent));
        }
        self.confirmed.clear();
    }

    /// The avatar's next confirmed shot, ending at `to`, at frame time `now`:
    /// the oldest drawn shot, if any, is the same one.
    pub fn confirm(&mut self, to: FxVec2, now: f64) -> Option<Outcome> {
        let drawn = self.outstanding.pop_front();
        self.confirmed.push_back(drawn.as_ref().map(|d| d.id));
        let drawn = drawn?;
        Some(Outcome::Fired { predicted: drawn.shot.to, confirmed: to, lead: now - drawn.at, exact: drawn.shot.to == to })
    }

    /// Gives up drawn shots the confirmed world, about to run `tick`, is
    /// well past without firing.
    pub fn expire(&mut self, tick: u64) -> Vec<Outcome> {
        let mut given_up = Vec::new();
        self.outstanding.retain(|d| {
            let late = tick > d.tick + TIMEOUT_TICKS;
            if late {
                given_up.push(Outcome::NeverFired { predicted: d.shot.to });
            }
            !late
        });
        given_up
    }

    /// The strikes the replay resolved between the confirmed tick and the
    /// drawn one, oldest first. None of them has been confirmed, so a laser
    /// one is the next drawn shot still outstanding from the same command,
    /// if any (an input the host moved later moves its shot's tick); the
    /// rest are new and drawn now, aimed by `aim`. Drawn shots a strike
    /// passes over no longer fire in the replay (its gating changed): inputs
    /// apply in the order they were sent, so an older command's shot can't
    /// come after. They are given up. Two commands with exactly the same aim
    /// and muzzle can't be told apart; the older shot is taken.
    pub fn predict(&mut self, strikes: &[ReplayedStrike], now: f64, aim: impl Fn(&ReplayedStrike) -> Option<PredictedLaser>) -> Predicted {
        let mut predicted = Predicted::default();
        // Outstanding shots before this are the strikes' so far.
        let mut matched = 0;
        let lasers = strikes.iter().filter(|strike| strike.weapon == content::scenario::Weapon::Laser);
        for strike in lasers {
            let same = self.outstanding.iter().skip(matched).position(|drawn| drawn.from_same_command(strike));
            let passed = same.unwrap_or(self.outstanding.len() - matched);
            for drawn in self.outstanding.drain(matched..matched + passed) {
                predicted.dropped.push(Outcome::NeverFired { predicted: drawn.shot.to });
            }
            if same.is_some() {
                self.outstanding[matched].tick = strike.tick;
                matched += 1;
                continue;
            }
            let Some(shot) = aim(strike) else { continue };
            let id = self.next_id;
            self.next_id += 1;
            self.outstanding.push_back(Drawn { id, tick: strike.tick, shot, at: now, aim: strike.aim, muzzle: strike.muzzle });
            matched += 1;
            predicted.new.push(NewShot { id, tick: strike.tick, shot });
        }
        predicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::scenario::Weapon;

    fn strike(tick: u64, weapon: Weapon) -> ReplayedStrike {
        ReplayedStrike { tick, weapon, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3], origin: FxVec2::ZERO, height: Fx::ZERO }
    }

    /// A laser from the strike's origin to `x` units along its aim.
    fn ending_at(x: i32) -> impl Fn(&ReplayedStrike) -> Option<PredictedLaser> {
        move |s| Some(PredictedLaser { from: s.origin, to: FxVec2::new(Fx::from_int(x), Fx::ZERO), hit_actor: false, cover: None })
    }

    #[test]
    fn a_shot_is_drawn_once_and_its_confirmation_matched() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        let sword = strike(12, Weapon::Sword);
        // The replay runs the same ticks every frame; the shot is new once.
        let first = lasers.predict(&[sword.clone(), strike(14, Weapon::Laser)], 1.0, ending_at(5));
        assert_eq!(first.new.len(), 1);
        assert_eq!(first.new[0].tick, 14);
        assert!(lasers.predict(&[sword, strike(14, Weapon::Laser)], 1.1, ending_at(5)).new.is_empty());
        // A second press, then the first one is confirmed where it was drawn.
        assert_eq!(lasers.predict(&[strike(14, Weapon::Laser), strike(22, Weapon::Laser)], 1.2, ending_at(6)).new.len(), 1);
        lasers.begin(1, 0);
        let fired = lasers.confirm(FxVec2::new(Fx::from_int(5), Fx::ZERO), 1.25).unwrap();
        assert!(matches!(fired, Outcome::Fired { exact: true, lead, .. } if (lead - 0.25).abs() < 1e-9));
        assert_eq!(lasers.confirmed, [Some(0)]);
        assert!(lasers.predict(&[strike(22, Weapon::Laser)], 1.3, ending_at(6)).new.is_empty(), "the second shot is still drawn");
        // It ends elsewhere: a misprediction, still drawn once.
        let fired = lasers.confirm(FxVec2::new(Fx::from_int(4), Fx::ZERO), 1.4).unwrap();
        assert!(matches!(fired, Outcome::Fired { exact: false, .. }));
        // A confirmed shot nobody predicted is drawn as it arrives.
        assert_eq!(lasers.confirm(FxVec2::ZERO, 1.5), None);
        assert_eq!(lasers.confirmed, [Some(0), Some(1), None]);
        lasers.begin(1, 0);
        assert!(lasers.confirmed.is_empty());
    }

    #[test]
    fn a_late_input_moves_the_drawn_shot_instead_of_drawing_another() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        assert_eq!(lasers.predict(&[strike(30, Weapon::Laser)], 0.0, ending_at(5)).new.len(), 1);
        // The confirmed world passed tick 30 without the input: the replay
        // applies it on each next tick, so the shot keeps moving.
        for tick in 31..60 {
            assert!(lasers.predict(&[strike(tick + 2, Weapon::Laser)], 0.0, ending_at(5)).new.is_empty());
            assert!(lasers.expire(tick).is_empty(), "tick {tick}");
        }
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(5), Fx::ZERO), 1.0), Some(Outcome::Fired { exact: true, .. })));
    }

    #[test]
    fn a_shot_that_never_fires_is_given_up_once() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        let drawn = lasers.predict(&[strike(30, Weapon::Laser)], 0.0, ending_at(5));
        assert_eq!(drawn.new.len(), 1);
        // The confirmed world never fires it; the replay stops showing it.
        for tick in 25..=30 + TIMEOUT_TICKS {
            assert!(lasers.predict(&[], 0.0, ending_at(5)).new.is_empty());
            assert!(lasers.expire(tick).is_empty(), "given up early, on tick {tick}");
        }
        assert_eq!(lasers.expire(31 + TIMEOUT_TICKS), [Outcome::NeverFired { predicted: FxVec2::new(Fx::from_int(5), Fx::ZERO) }]);
        assert!(lasers.expire(200).is_empty(), "logged once");
        // The next shot is drawn and matched as usual.
        assert_eq!(lasers.predict(&[strike(50, Weapon::Laser)], 2.0, ending_at(7)).new.len(), 1);
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(7), Fx::ZERO), 2.2), Some(Outcome::Fired { exact: true, .. })));
    }

    #[test]
    fn another_world_or_avatar_starts_over() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        assert_eq!(lasers.predict(&[strike(30, Weapon::Laser)], 0.0, ending_at(5)).new.len(), 1);
        lasers.begin(2, 0);
        assert_eq!(lasers.confirm(FxVec2::ZERO, 1.0), None);
        assert_eq!(lasers.predict(&[strike(40, Weapon::Laser)], 0.0, ending_at(5)).new.len(), 1);
        lasers.begin(2, 1);
        assert!(lasers.expire(1000).is_empty());
    }

    #[test]
    fn a_shot_that_drops_out_of_the_replay_does_not_take_the_next_ones_place() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        let aimed = |tick, y| ReplayedStrike { aim: FxVec2::new(Fx::ONE, Fx::from_int(y)), ..strike(tick, Weapon::Laser) };
        assert_eq!(lasers.predict(&[aimed(30, 0)], 0.0, ending_at(5)).new.len(), 1);
        // The replay no longer fires it (its gating changed), and the next
        // press resolves before it would time out: that one is drawn, and
        // the first given up.
        let predicted = lasers.predict(&[aimed(35, 1)], 0.1, ending_at(7));
        assert_eq!(predicted.new, [NewShot { id: 1, tick: 35, shot: ending_at(7)(&aimed(35, 1)).unwrap() }]);
        assert_eq!(predicted.dropped, [Outcome::NeverFired { predicted: FxVec2::new(Fx::from_int(5), Fx::ZERO) }]);
        assert_eq!(lasers.predict(&[aimed(35, 1)], 0.2, ending_at(7)), Predicted::default());
        // Its confirmation is matched to it, not to the first.
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(7), Fx::ZERO), 0.3), Some(Outcome::Fired { exact: true, .. })));
        assert_eq!(lasers.confirmed, [Some(1)]);
        assert!(lasers.expire(100).is_empty(), "given up once");

        // A shot the host moved later keeps its command's aim: still one shot.
        assert_eq!(lasers.predict(&[aimed(50, 2), aimed(60, 3)], 1.0, ending_at(5)).new.len(), 2);
        let moved = lasers.predict(&[aimed(52, 2), aimed(62, 3)], 1.1, ending_at(5));
        assert_eq!(moved, Predicted::default());
        // The first of two drops out: only it is given up.
        let predicted = lasers.predict(&[aimed(62, 3)], 1.2, ending_at(5));
        assert!(predicted.new.is_empty());
        assert_eq!(predicted.dropped.len(), 1);
        assert!(lasers.expire(70).is_empty());
        lasers.begin(1, 0);
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(5), Fx::ZERO), 1.3), Some(Outcome::Fired { exact: true, .. })));
        assert_eq!(lasers.confirmed, [Some(3)]);
    }
}

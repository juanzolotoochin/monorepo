//! Predicted laser shots of a joined player's own avatar.
//!
//! The avatar is drawn about an input lead ahead of the confirmed world
//! (`prediction`). When the drawn time reaches the tick a laser swing
//! resolves on in the replay, the replay's `LaserFired`, aimed with the
//! simulation's own targeting against the confirmed world, is drawn at once:
//! beam, laser sound, impact spark and impact sound. Damage, flinches and
//! cover breaking wait for the confirmed `LaserFired`, which is matched to
//! the drawn shot in order and not drawn again. A confirmed shot that ends
//! elsewhere, or a drawn one that never fires, is a laser misprediction.

use std::collections::VecDeque;

use bevy::prelude::Resource;
use fx::{Fx, FxVec2};
use sim::settlement::Event;

use crate::prediction::PredictedEvent;

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

/// A predicted shot: `Event::LaserFired`'s fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shot {
    pub from: FxVec2,
    pub to: FxVec2,
    pub hit_actor: bool,
    pub cover: Option<u16>,
}

impl Shot {
    /// The shot a predicted `LaserFired` draws.
    fn of(event: &Event) -> Option<Shot> {
        match *event {
            Event::LaserFired { from, to, hit_actor, cover, .. } => Some(Shot { from, to, hit_actor, cover }),
            _ => None,
        }
    }
}

struct Drawn {
    id: u64,
    /// The tick it is predicted to fire on.
    tick: u64,
    shot: Shot,
    /// Frame time it was drawn at, in seconds.
    at: f64,
    /// The `Attack` command's aim and muzzle, which tell its shot apart
    /// from another command's.
    aim: FxVec2,
    muzzle: [Fx; 3],
}

impl Drawn {
    fn from_same_command(&self, event: &PredictedEvent) -> bool {
        (self.aim, self.muzzle) == (event.aim, event.muzzle)
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
    pub shot: Shot,
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


    /// The events the replay showed between the confirmed tick and the
    /// drawn one, oldest first. None of them has been confirmed, so a
    /// `LaserFired` is the next drawn shot still outstanding from the same
    /// command, if any (an input the host moved later moves its shot's
    /// tick); the rest are new and drawn now. Drawn shots a shot passes over
    /// no longer fire in the replay (its gating changed): inputs apply in
    /// the order they were sent, so an older command's shot can't come
    /// after. They are given up. Two commands with exactly the same aim and
    /// muzzle can't be told apart; the older shot is taken.
    pub fn predict(&mut self, events: &[PredictedEvent], now: f64) -> Predicted {
        let mut predicted = Predicted::default();
        // Outstanding shots before this are the events' so far.
        let mut matched = 0;
        let lasers = events.iter().filter_map(|event| Some((event, Shot::of(&event.event)?)));
        for (event, shot) in lasers {
            let same = self.outstanding.iter().skip(matched).position(|drawn| drawn.from_same_command(event));
            let passed = same.unwrap_or(self.outstanding.len() - matched);
            for drawn in self.outstanding.drain(matched..matched + passed) {
                predicted.dropped.push(Outcome::NeverFired { predicted: drawn.shot.to });
            }
            if same.is_some() {
                self.outstanding[matched].tick = event.tick;
                matched += 1;
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            self.outstanding.push_back(Drawn { id, tick: event.tick, shot, at: now, aim: event.aim, muzzle: event.muzzle });
            matched += 1;
            predicted.new.push(NewShot { id, tick: event.tick, shot });
        }
        predicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the replay shows on `tick` for a swing aimed along x: a laser
    /// shot ending `x` units along it, or for `None` an arrow.
    fn shown(tick: u64, x: Option<i32>) -> PredictedEvent {
        let event = match x {
            Some(x) => Event::LaserFired { agent: 0, from: FxVec2::ZERO, to: FxVec2::new(Fx::from_int(x), Fx::ZERO), hit_actor: false, cover: None, started: tick - 2 },
            None => Event::BowFired { agent: 0, started: tick - 10 },
        };
        PredictedEvent { tick, event, aim: FxVec2::new(Fx::ONE, Fx::ZERO), muzzle: [Fx::ZERO; 3] }
    }

    fn laser(tick: u64, x: i32) -> PredictedEvent {
        shown(tick, Some(x))
    }

    #[test]
    fn a_shot_is_drawn_once_and_its_confirmation_matched() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        let arrow = shown(12, None);
        // The replay runs the same ticks every frame; the shot is new once.
        let first = lasers.predict(&[arrow.clone(), laser(14, 5)], 1.0);
        assert_eq!(first.new.len(), 1);
        assert_eq!(first.new[0].tick, 14);
        assert!(lasers.predict(&[arrow, laser(14, 5)], 1.1).new.is_empty());
        // A second press, then the first one is confirmed where it was drawn.
        assert_eq!(lasers.predict(&[laser(14, 5), laser(22, 6)], 1.2).new.len(), 1);
        lasers.begin(1, 0);
        let fired = lasers.confirm(FxVec2::new(Fx::from_int(5), Fx::ZERO), 1.25).unwrap();
        assert!(matches!(fired, Outcome::Fired { exact: true, lead, .. } if (lead - 0.25).abs() < 1e-9));
        assert_eq!(lasers.confirmed, [Some(0)]);
        assert!(lasers.predict(&[laser(22, 6)], 1.3).new.is_empty(), "the second shot is still drawn");
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
        assert_eq!(lasers.predict(&[laser(30, 5)], 0.0).new.len(), 1);
        // The confirmed world passed tick 30 without the input: the replay
        // applies it on each next tick, so the shot keeps moving.
        for tick in 31..60 {
            assert!(lasers.predict(&[laser(tick + 2, 5)], 0.0).new.is_empty());
            assert!(lasers.expire(tick).is_empty(), "tick {tick}");
        }
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(5), Fx::ZERO), 1.0), Some(Outcome::Fired { exact: true, .. })));
    }

    #[test]
    fn a_shot_that_never_fires_is_given_up_once() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        let drawn = lasers.predict(&[laser(30, 5)], 0.0);
        assert_eq!(drawn.new.len(), 1);
        // The confirmed world never fires it; the replay stops showing it.
        for tick in 25..=30 + TIMEOUT_TICKS {
            assert!(lasers.predict(&[], 0.0).new.is_empty());
            assert!(lasers.expire(tick).is_empty(), "given up early, on tick {tick}");
        }
        assert_eq!(lasers.expire(31 + TIMEOUT_TICKS), [Outcome::NeverFired { predicted: FxVec2::new(Fx::from_int(5), Fx::ZERO) }]);
        assert!(lasers.expire(200).is_empty(), "logged once");
        // The next shot is drawn and matched as usual.
        assert_eq!(lasers.predict(&[laser(50, 7)], 2.0).new.len(), 1);
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(7), Fx::ZERO), 2.2), Some(Outcome::Fired { exact: true, .. })));
    }

    #[test]
    fn another_world_or_avatar_starts_over() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        assert_eq!(lasers.predict(&[laser(30, 5)], 0.0).new.len(), 1);
        lasers.begin(2, 0);
        assert_eq!(lasers.confirm(FxVec2::ZERO, 1.0), None);
        assert_eq!(lasers.predict(&[laser(40, 5)], 0.0).new.len(), 1);
        lasers.begin(2, 1);
        assert!(lasers.expire(1000).is_empty());
    }

    #[test]
    fn a_shot_that_drops_out_of_the_replay_does_not_take_the_next_ones_place() {
        let mut lasers = LaserPrediction::default();
        lasers.begin(1, 0);
        let aimed = |tick, y, x| PredictedEvent { aim: FxVec2::new(Fx::ONE, Fx::from_int(y)), ..laser(tick, x) };
        assert_eq!(lasers.predict(&[aimed(30, 0, 5)], 0.0).new.len(), 1);
        // The replay no longer fires it (its gating changed), and the next
        // press resolves before it would time out: that one is drawn, and
        // the first given up.
        let predicted = lasers.predict(&[aimed(35, 1, 7)], 0.1);
        assert_eq!(predicted.new, [NewShot { id: 1, tick: 35, shot: Shot::of(&laser(35, 7).event).unwrap() }]);
        assert_eq!(predicted.dropped, [Outcome::NeverFired { predicted: FxVec2::new(Fx::from_int(5), Fx::ZERO) }]);
        assert_eq!(lasers.predict(&[aimed(35, 1, 7)], 0.2), Predicted::default());
        // Its confirmation is matched to it, not to the first.
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(7), Fx::ZERO), 0.3), Some(Outcome::Fired { exact: true, .. })));
        assert_eq!(lasers.confirmed, [Some(1)]);
        assert!(lasers.expire(100).is_empty(), "given up once");

        // A shot the host moved later keeps its command's aim: still one shot.
        assert_eq!(lasers.predict(&[aimed(50, 2, 5), aimed(60, 3, 5)], 1.0).new.len(), 2);
        let moved = lasers.predict(&[aimed(52, 2, 5), aimed(62, 3, 5)], 1.1);
        assert_eq!(moved, Predicted::default());
        // The first of two drops out: only it is given up.
        let predicted = lasers.predict(&[aimed(62, 3, 5)], 1.2);
        assert!(predicted.new.is_empty());
        assert_eq!(predicted.dropped.len(), 1);
        assert!(lasers.expire(70).is_empty());
        lasers.begin(1, 0);
        assert!(matches!(lasers.confirm(FxVec2::new(Fx::from_int(5), Fx::ZERO), 1.3), Some(Outcome::Fired { exact: true, .. })));
        assert_eq!(lasers.confirmed, [Some(3)]);
    }
}

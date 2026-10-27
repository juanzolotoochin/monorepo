//! Predicted events of a joined player's own avatar, matched to the
//! confirmed ones (D7 and D8 in `ABILITY_PREDICTION_HANDOFF.md`).
//!
//! The avatar is drawn about an input lead ahead of the confirmed world
//! (`prediction`). The replay shows the events its stage 3 messages give
//! (`Event::prediction` says `Predict`). When the drawn time first reaches
//! one of a kind the app presents early (`PRESENTED_EARLY`), it is presented
//! at once: a laser's beam, sound and impact, an arc coil's arc to its
//! first target and its sound, or a bow's arrow in flight and its sound.
//! Damage, the arc's chain to further targets, an arrow's hit, flinches and
//! cover breaking wait for the confirmed event, which is matched to the
//! presented one and not presented again (an arrow is handed over to its
//! confirmed projectile).
//!
//! An event is told apart from every other one by the command that caused
//! it and its kind. A predicted event's `started` is the tick its command
//! applied on: in the replay for a command still pending, which applies on
//! the tick it asked for or, if the confirmed world is already past that,
//! the next one; in the confirmed world, as the client's drain record says
//! (`net::Client::applied_commands`), for one that applied. An input the
//! host moved later keeps its command, so its event moves instead of being
//! presented twice.
//!
//! A presented event that never confirms, or confirms differently, is an
//! event misprediction. Its kind's miss policy (`OnMiss`) says what then:
//! `Keep` leaves what was presented, `Cancel` asks the presenter to stop it.

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::Resource;
use fx::FxVec2;
use net::CommandId;
use sim::settlement::{Event, OnMiss, Prediction};

use crate::prediction::PredictedEvent;

/// A presented event the confirmed world is this many ticks past without
/// confirming it is given up. The confirmed world reaches the predicted tick
/// about an input lead after it was presented; while the replay still
/// predicts it (an input the host moved later) its tick moves with it.
pub const TIMEOUT_TICKS: u64 = 10;

/// The predictable kinds a client presents before they are confirmed, each
/// by `lockstep::predict_events` (and `stop_presented` for a `Cancel` kind).
/// A matched confirmed arc coil shot isn't drawn again, but still starts the
/// chain its `Hit`s draw (`settlement::ArcChain`). A bow shot draws its arrow
/// in flight with its sound, and the matched confirmed shot's projectile
/// takes that arrow over (`settlement::PredictedArrows`); one that never
/// confirms withdraws it, though the bow's miss policy is `Keep`.
pub const PRESENTED_EARLY: &[&str] = &["laser", "arc", "bow"];

/// A predictable event's kind, as telemetry names it.
pub fn kind_of(event: &Event) -> Option<&'static str> {
    match event {
        Event::LaserFired { .. } => Some("laser"),
        Event::ArcFired { .. } => Some("arc"),
        Event::BowFired { .. } => Some("bow"),
        _ => None,
    }
}

/// Where a predictable event ends in the world, if it has an end: a beam's,
/// or where an arrow comes down unless it hits something.
pub fn end_of(event: &Event) -> Option<FxVec2> {
    match *event {
        Event::LaserFired { to, .. } | Event::ArcFired { to, .. } | Event::BowFired { to, .. } => Some(to),
        _ => None,
    }
}

/// Whether a confirmed event shows what the predicted one did. Their start
/// ticks only key them: an input the host moved later starts later.
fn same_shown(predicted: &Event, confirmed: &Event) -> bool {
    let mut moved = predicted.clone();
    if let (
        Event::LaserFired { started, .. } | Event::ArcFired { started, .. } | Event::BowFired { started, .. },
        Event::LaserFired { started: actual, .. } | Event::ArcFired { started: actual, .. } | Event::BowFired { started: actual, .. },
    ) = (&mut moved, confirmed)
    {
        *started = *actual;
    }
    moved == *confirmed
}

/// How the ledger treats an event of `agent`'s, if it predicts it at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub kind: &'static str,
    /// The tick its action's command applied on.
    pub started: u64,
    pub on_miss: OnMiss,
}

/// The app's policy: `Event::prediction`, for the kinds presented early.
pub fn policy(event: &Event, agent: u16) -> Option<Policy> {
    let Prediction::Predict { agent: owner, started, on_miss } = event.prediction() else { return None };
    let kind = kind_of(event).filter(|kind| PRESENTED_EARLY.contains(kind))?;
    (owner == agent).then_some(Policy { kind, started, on_miss })
}

/// What caused an event: the command, or when the client has no record of
/// one applying on its start tick (a swing loaded with the world), that
/// tick. `nth` tells apart events of one kind started on the same tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cause {
    Unknown { started: u64, nth: usize },
    Command(CommandId),
}

/// The client's commands, each with the tick it applies on, oldest first:
/// the drain record for the confirmed ticks, then the pending commands on
/// the ticks the replay applies them on.
pub type Applied = [(u64, CommandId)];

/// The `nth` event of a kind started on `started` is the `nth` command that
/// applied on that tick. Several commands on one tick are told apart only
/// by order (their events of one kind are presented and confirmed in order).
fn cause(applied: &Applied, started: u64, nth: usize) -> Cause {
    let mut on_tick = applied.iter().filter(|(tick, _)| *tick == started).map(|(_, id)| *id);
    on_tick.nth(nth).map_or(Cause::Unknown { started, nth }, Cause::Command)
}

#[derive(Resource)]
pub struct EventPrediction {
    /// The (world, agent) predicted; anything else starts over.
    key: Option<(u32, u16)>,
    /// Which events it predicts. Tests swap it for a kind with another
    /// miss policy.
    policy: fn(&Event, u16) -> Option<Policy>,
    /// Presented events the confirmed world hasn't confirmed yet, oldest first.
    outstanding: VecDeque<Presented>,
    next_id: u64,
    /// Confirmed events so far by start tick and kind, for `nth`.
    confirmed_on: BTreeMap<(u64, &'static str), usize>,
    /// For each of the avatar's predicted kinds' confirmed events this
    /// frame, in order: the presented event it matched, which already showed
    /// it, if it is to stay.
    pub confirmed: Confirmations,
}

impl Default for EventPrediction {
    fn default() -> Self {
        EventPrediction { key: None, policy, outstanding: VecDeque::new(), next_id: 0, confirmed_on: BTreeMap::new(), confirmed: Confirmations::default() }
    }
}

/// This frame's confirmed events of the avatar's predicted kinds, in order.
#[derive(Debug, Default, PartialEq)]
pub struct Confirmations(VecDeque<(&'static str, Option<u64>)>);

impl Confirmations {
    /// The avatar's next confirmed event of `kind`: the presented event that
    /// already showed it, or `None` to present it now.
    pub fn next(&mut self, kind: &str) -> Option<u64> {
        let at = self.0.iter().position(|(k, _)| *k == kind)?;
        self.0.remove(at).and_then(|(_, id)| id)
    }

    #[cfg(test)]
    fn ids(&self) -> Vec<Option<u64>> {
        self.0.iter().map(|(_, id)| *id).collect()
    }
}

struct Presented {
    id: u64,
    kind: &'static str,
    cause: Cause,
    /// The tick it is predicted on.
    tick: u64,
    event: Event,
    on_miss: OnMiss,
    /// Frame time it was presented at, in seconds.
    at: f64,
}

impl Presented {
    fn missed(self) -> Outcome {
        Outcome { kind: self.kind, id: self.id, predicted: self.event, confirmed: None, lead: None, exact: false, cancel: self.on_miss == OnMiss::Cancel }
    }
}

/// An event to present now.
#[derive(Clone, Debug, PartialEq)]
pub struct NewEvent {
    pub id: u64,
    pub kind: &'static str,
    /// The tick it is predicted on.
    pub tick: u64,
    pub event: Event,
}

/// How a presented event turned out.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub kind: &'static str,
    pub id: u64,
    pub predicted: Event,
    /// The confirmed event; `None` if it never confirmed.
    pub confirmed: Option<Event>,
    /// Seconds from presenting it to its confirmation.
    pub lead: Option<f64>,
    /// It confirmed exactly as presented.
    pub exact: bool,
    /// Its kind is `Cancel` and it missed: the presenter stops it.
    pub cancel: bool,
}

/// What a frame's replay changed.
#[derive(Debug, Default, PartialEq)]
pub struct Predicted {
    /// Events to present now.
    pub new: Vec<NewEvent>,
    /// Presented events the replay no longer shows.
    pub dropped: Vec<Outcome>,
}

impl EventPrediction {
    pub fn reset(&mut self) {
        *self = EventPrediction { policy: self.policy, ..EventPrediction::default() };
    }

    /// Starts a frame predicting `agent` in world `world`.
    pub fn begin(&mut self, world: u32, agent: u16) {
        if self.key != Some((world, agent)) {
            self.reset();
            self.key = Some((world, agent));
        }
        self.confirmed.0.clear();
    }

    /// One of this frame's confirmed events, at frame time `now`. `applied`
    /// holds the commands that applied on its start tick. Returns how the
    /// presented event of the same command and kind, if any, turned out.
    pub fn confirm(&mut self, event: &Event, applied: &Applied, now: f64) -> Option<Outcome> {
        let (_, agent) = self.key?;
        let Policy { kind, started, .. } = (self.policy)(event, agent)?;
        let seen = self.confirmed_on.entry((started, kind)).or_default();
        let cause = cause(applied, started, *seen);
        *seen += 1;
        let Some(at) = self.outstanding.iter().position(|p| p.kind == kind && p.cause == cause) else {
            self.confirmed.0.push_back((kind, None));
            return None;
        };
        let presented = self.outstanding.remove(at).unwrap();
        let exact = same_shown(&presented.event, event);
        // A one-shot stays; a lasting effect that confirmed differently is
        // stopped, and the confirmed event presented instead.
        let cancel = !exact && presented.on_miss == OnMiss::Cancel;
        self.confirmed.0.push_back((kind, (!cancel).then_some(presented.id)));
        Some(Outcome {
            kind,
            id: presented.id,
            predicted: presented.event,
            confirmed: Some(event.clone()),
            lead: Some(now - presented.at),
            exact,
            cancel,
        })
    }

    /// Gives up presented events the confirmed world, about to run `tick`,
    /// is well past without confirming.
    pub fn expire(&mut self, tick: u64) -> Vec<Outcome> {
        let (late, kept): (Vec<_>, Vec<_>) = self.outstanding.drain(..).partition(|p| tick > p.tick + TIMEOUT_TICKS);
        self.outstanding = kept.into();
        self.confirmed_on.retain(|(started, _), _| started + net::APPLIED_HISTORY >= tick);
        late.into_iter().map(Presented::missed).collect()
    }

    /// The events the replay showed between the confirmed tick and the
    /// drawn one, oldest first, at frame time `now`. `applied` holds the
    /// commands applied on those events' start ticks. None of them has been
    /// confirmed, so one already presented for the same command and kind is
    /// that one, moved if its command was; the rest are presented now. A
    /// presented event that no longer shows while a later command's event of
    /// its kind does won't happen any more (its gating changed): inputs apply
    /// in the order they were sent, so an older command's event can't come
    /// after. It is given up.
    pub fn predict(&mut self, events: &[PredictedEvent], applied: &Applied, now: f64) -> Predicted {
        let mut predicted = Predicted::default();
        let Some((_, agent)) = self.key else { return predicted };
        // Per kind, the latest command that showed an event this frame.
        let mut shown = BTreeMap::<&'static str, Cause>::new();
        let mut on_tick = BTreeMap::<(u64, &'static str), usize>::new();
        let mut seen = Vec::new();
        for PredictedEvent { tick, event } in events {
            let Some(Policy { kind, started, on_miss }) = (self.policy)(event, agent) else { continue };
            let earlier = on_tick.entry((started, kind)).or_default();
            let nth = self.confirmed_on.get(&(started, kind)).copied().unwrap_or(0) + *earlier;
            *earlier += 1;
            let cause = cause(applied, started, nth);
            shown.entry(kind).and_modify(|latest| *latest = (*latest).max(cause)).or_insert(cause);
            seen.push((kind, cause));
            if let Some(presented) = self.outstanding.iter_mut().find(|p| p.kind == kind && p.cause == cause) {
                presented.tick = *tick;
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            self.outstanding.push_back(Presented { id, kind, cause, tick: *tick, event: event.clone(), on_miss, at: now });
            predicted.new.push(NewEvent { id, kind, tick: *tick, event: event.clone() });
        }
        let passed = |p: &Presented| !seen.contains(&(p.kind, p.cause)) && shown.get(p.kind).is_some_and(|latest| p.cause < *latest);
        let (dropped, kept): (Vec<_>, Vec<_>) = self.outstanding.drain(..).partition(passed);
        self.outstanding = kept.into();
        predicted.dropped = dropped.into_iter().map(Presented::missed).collect();
        predicted
    }
}

#[cfg(test)]
mod tests {
    use fx::Fx;

    use super::*;

    const AGENT: u16 = 0;

    fn at(x: i32) -> FxVec2 {
        FxVec2::new(Fx::from_int(x), Fx::ZERO)
    }

    /// A laser shot ending `x` units along x, fired 2 ticks after its press.
    fn laser_event(started: u64, x: i32) -> Event {
        Event::LaserFired { agent: AGENT, from: FxVec2::ZERO, to: at(x), hit_actor: false, cover: None, started }
    }

    /// What the replay shows on `tick` for an `Attack` that applied 2 ticks
    /// before.
    fn laser(tick: u64, x: i32) -> PredictedEvent {
        PredictedEvent { tick, event: laser_event(tick - 2, x) }
    }

    fn arrow(tick: u64) -> PredictedEvent {
        PredictedEvent { tick, event: bow_event(tick - 10, 9) }
    }

    fn command(n: u64) -> CommandId {
        CommandId(n)
    }

    fn ledger() -> EventPrediction {
        let mut ledger = EventPrediction::default();
        ledger.begin(1, AGENT);
        ledger
    }

    #[test]
    fn an_event_is_presented_once_and_its_confirmation_matched() {
        let mut ledger = ledger();
        let applied = [(2, command(0)), (12, command(1)), (20, command(2))];
        // The replay runs the same ticks every frame; the shot is new once.
        let first = ledger.predict(&[laser(14, 5)], &applied, 1.0);
        assert_eq!(first.new.iter().map(|e| (e.kind, e.tick)).collect::<Vec<_>>(), [("laser", 14)]);
        assert!(ledger.predict(&[laser(14, 5)], &applied, 1.1).new.is_empty());
        // A second press, then the first shot is confirmed as presented.
        assert_eq!(ledger.predict(&[laser(14, 5), laser(22, 6)], &applied, 1.2).new.len(), 1);
        ledger.begin(1, AGENT);
        let fired = ledger.confirm(&laser_event(12, 5), &applied, 1.25).unwrap();
        assert!(fired.exact && !fired.cancel && (fired.lead.unwrap() - 0.25).abs() < 1e-9);
        assert_eq!((fired.kind, fired.id), ("laser", 0));
        assert_eq!(ledger.confirmed.ids(), [Some(0)]);
        assert!(ledger.predict(&[laser(22, 6)], &applied, 1.3).new.is_empty(), "the second shot is still presented");
        // It ends elsewhere: a misprediction, still presented once.
        let fired = ledger.confirm(&laser_event(20, 4), &applied, 1.4).unwrap();
        assert!(!fired.exact && !fired.cancel);
        assert_eq!(fired.confirmed, Some(laser_event(20, 4)));
        // A confirmed shot nobody predicted is presented as it arrives.
        assert_eq!(ledger.confirm(&laser_event(40, 0), &applied, 1.5), None);
        // Another agent's isn't the ledger's.
        let theirs = Event::LaserFired { agent: 1, from: FxVec2::ZERO, to: at(5), hit_actor: false, cover: None, started: 12 };
        assert_eq!(ledger.confirm(&theirs, &applied, 1.5), None);
        // A bow shot nobody predicted is presented as it is confirmed.
        assert_eq!(ledger.confirm(&arrow(12).event, &applied, 1.6), None);
        assert_eq!(ledger.confirmed.ids(), [Some(0), Some(1), None, None]);
        assert_eq!(ledger.confirmed.next("bow"), None);
        assert_eq!(ledger.confirmed.next("laser"), Some(0));
        ledger.begin(1, AGENT);
        assert_eq!(ledger.confirmed, Confirmations::default());
    }

    /// An arc coil shot ending `x` units along x, fired 2 ticks after its press.
    fn arc_event(started: u64, x: i32) -> Event {
        Event::ArcFired { agent: AGENT, from: FxVec2::ZERO, to: at(x), started }
    }

    fn arc(tick: u64, x: i32) -> PredictedEvent {
        PredictedEvent { tick, event: arc_event(tick - 2, x) }
    }

    #[test]
    fn an_arc_shot_is_presented_once_and_kept_however_it_confirms() {
        let mut ledger = ledger();
        let applied = [(10, command(0)), (20, command(1)), (30, command(2))];
        let first = ledger.predict(&[arc(12, 3)], &applied, 0.0);
        assert_eq!(first.new, [NewEvent { id: 0, kind: "arc", tick: 12, event: arc_event(10, 3) }]);
        assert_eq!(ledger.predict(&[arc(12, 3), arc(22, 4)], &applied, 0.1).new.len(), 1, "the first shot is drawn once");
        // Confirmed as drawn: settlement skips drawing it, but its hits
        // still chain from it.
        ledger.begin(1, AGENT);
        assert!(ledger.confirm(&arc_event(10, 3), &applied, 0.2).unwrap().exact);
        // The second reaches another ghoul: a misprediction, but the arc
        // already drawn stays (`Keep`), and isn't drawn again.
        let elsewhere = ledger.confirm(&arc_event(20, 6), &applied, 0.3).unwrap();
        assert!(!elsewhere.exact && !elsewhere.cancel);
        assert_eq!(ledger.confirmed.next("arc"), Some(0));
        assert_eq!(ledger.confirmed.next("arc"), Some(1));
        // A shot the replay never showed (the prediction was withdrawn) is
        // drawn as it is confirmed.
        ledger.begin(1, AGENT);
        assert_eq!(ledger.confirm(&arc_event(30, 5), &applied, 0.4), None);
        assert_eq!(ledger.confirmed.ids(), [None]);
        // One that never confirms is given up once, and left drawn.
        let applied = [(40, command(3))];
        assert_eq!(ledger.predict(&[arc(42, 5)], &applied, 1.0).new.len(), 1);
        let given_up = ledger.expire(43 + TIMEOUT_TICKS);
        assert_eq!(given_up.iter().map(|o| (o.kind, o.id, o.confirmed.is_none(), o.cancel)).collect::<Vec<_>>(), [("arc", 2, true, false)]);
        assert!(ledger.expire(200).is_empty());
    }

    /// A bow shot along x, `x` units, fired 10 ticks after its press.
    fn bow_event(started: u64, x: i32) -> Event {
        Event::BowFired { agent: AGENT, from: FxVec2::ZERO, to: at(x), height: Fx::from_ratio(3, 5), flight: 19, started }
    }

    #[test]
    fn a_bow_shot_is_presented_once_and_its_arrow_handed_over_or_withdrawn() {
        let mut ledger = ledger();
        let applied = [(10, command(0)), (20, command(1)), (30, command(2)), (40, command(3))];
        let shot = |tick: u64, x: i32| PredictedEvent { tick, event: bow_event(tick - 10, x) };
        let first = ledger.predict(&[shot(20, 9)], &applied, 0.0);
        assert_eq!(first.new, [NewEvent { id: 0, kind: "bow", tick: 20, event: bow_event(10, 9) }]);
        assert_eq!(ledger.predict(&[shot(20, 9), shot(30, 8)], &applied, 0.1).new.len(), 1, "the first arrow is drawn once");
        // Confirmed as drawn: its projectile takes the arrow over, and its
        // sound isn't played again.
        ledger.begin(1, AGENT);
        assert!(ledger.confirm(&bow_event(10, 9), &applied, 0.2).unwrap().exact);
        // The second flies elsewhere (the avatar was elsewhere): still the
        // arrow drawn, so its projectile takes it over too (`Keep`).
        let elsewhere = ledger.confirm(&bow_event(20, 7), &applied, 0.3).unwrap();
        assert!(elsewhere.confirmed.is_some() && !elsewhere.exact && !elsewhere.cancel);
        assert_eq!(ledger.confirmed.next("bow"), Some(0));
        assert_eq!(ledger.confirmed.next("bow"), Some(1));
        // One the replay never showed is drawn, with its sound, as it is
        // confirmed.
        ledger.begin(1, AGENT);
        assert_eq!(ledger.confirm(&bow_event(30, 9), &applied, 0.4), None);
        assert_eq!(ledger.confirmed.ids(), [None]);
        // One that never confirms is given up once, unconfirmed: its arrow
        // is withdrawn (`lockstep::predict_events`).
        assert_eq!(ledger.predict(&[shot(50, 9)], &applied, 1.0).new.len(), 1);
        let given_up = ledger.expire(51 + TIMEOUT_TICKS);
        assert_eq!(given_up.iter().map(|o| (o.kind, o.id, o.confirmed.is_none())).collect::<Vec<_>>(), [("bow", 2, true)]);
        assert!(ledger.expire(200).is_empty());
    }

    #[test]
    fn a_late_input_moves_its_event_instead_of_presenting_another() {
        let mut ledger = ledger();
        // Asked for tick 28; it fires 2 ticks after it applies.
        assert_eq!(ledger.predict(&[laser(30, 5)], &[(28, command(7))], 0.0).new.len(), 1);
        // The confirmed world passed tick 28 without the input: the replay
        // applies it on each next tick, so the shot keeps moving.
        for tick in 29..60 {
            assert!(ledger.predict(&[laser(tick + 2, 5)], &[(tick, command(7))], 0.0).new.is_empty());
            assert!(ledger.expire(tick).is_empty(), "tick {tick}");
        }
        // The host applied it on tick 60: the drain record says so.
        let fired = ledger.confirm(&laser_event(60, 5), &[(60, command(7))], 1.0).unwrap();
        assert!(fired.exact);
    }

    #[test]
    fn an_event_that_never_confirms_is_given_up_once() {
        let mut ledger = ledger();
        let applied = [(28, command(0))];
        assert_eq!(ledger.predict(&[laser(30, 5)], &applied, 0.0).new.len(), 1);
        // The confirmed world never fires it; the replay stops showing it.
        for tick in 25..=30 + TIMEOUT_TICKS {
            assert!(ledger.predict(&[], &applied, 0.0).new.is_empty());
            assert!(ledger.expire(tick).is_empty(), "given up early, on tick {tick}");
        }
        let given_up = ledger.expire(31 + TIMEOUT_TICKS);
        assert_eq!(given_up.len(), 1);
        assert_eq!((given_up[0].predicted.clone(), given_up[0].confirmed.clone(), given_up[0].cancel), (laser_event(28, 5), None, false));
        assert!(ledger.expire(200).is_empty(), "logged once");
        // The next shot is presented and matched as usual.
        let applied = [(28, command(0)), (48, command(1))];
        assert_eq!(ledger.predict(&[laser(50, 7)], &applied, 2.0).new.len(), 1);
        assert!(ledger.confirm(&laser_event(48, 7), &applied, 2.2).unwrap().exact);
    }

    #[test]
    fn another_world_or_avatar_starts_over() {
        let mut ledger = ledger();
        let applied = [(28, command(0)), (38, command(1))];
        assert_eq!(ledger.predict(&[laser(30, 5)], &applied, 0.0).new.len(), 1);
        ledger.begin(2, AGENT);
        assert_eq!(ledger.confirm(&laser_event(28, 5), &applied, 1.0), None);
        assert_eq!(ledger.predict(&[laser(40, 5)], &applied, 0.0).new.len(), 1);
        ledger.begin(2, 1);
        assert!(ledger.expire(1000).is_empty());
        // Another avatar's events aren't this one's.
        assert!(ledger.predict(&[laser(40, 5)], &applied, 0.0).new.is_empty());
    }

    #[test]
    fn an_event_that_drops_out_of_the_replay_does_not_take_the_next_ones_place() {
        let mut ledger = ledger();
        let mut applied = vec![(28, command(0)), (33, command(1))];
        assert_eq!(ledger.predict(&[laser(30, 5)], &applied, 0.0).new.len(), 1);
        // The replay no longer fires it (its gating changed), and the next
        // press resolves before it would time out: that one is presented,
        // and the first given up.
        let predicted = ledger.predict(&[laser(35, 7)], &applied, 0.1);
        assert_eq!(predicted.new, [NewEvent { id: 1, kind: "laser", tick: 35, event: laser_event(33, 7) }]);
        assert_eq!(predicted.dropped.iter().map(|o| (o.id, o.confirmed.clone())).collect::<Vec<_>>(), [(0, None)]);
        assert_eq!(ledger.predict(&[laser(35, 7)], &applied, 0.2), Predicted::default());
        // Its confirmation is matched to it, not to the first.
        assert!(ledger.confirm(&laser_event(33, 7), &applied, 0.3).unwrap().exact);
        assert_eq!(ledger.confirmed.ids(), [Some(1)]);
        assert!(ledger.expire(100).is_empty(), "given up once");

        // A shot the host moved later keeps its command: still one shot.
        applied.extend([(48, command(2)), (58, command(3))]);
        assert_eq!(ledger.predict(&[laser(50, 5), laser(60, 5)], &applied, 1.0).new.len(), 2);
        let moved = [(50, command(2)), (60, command(3))];
        let applied: Vec<_> = applied[..2].iter().copied().chain(moved).collect();
        assert_eq!(ledger.predict(&[laser(52, 5), laser(62, 5)], &applied, 1.1), Predicted::default());
        // The first of two drops out: only it is given up.
        let predicted = ledger.predict(&[laser(62, 5)], &applied, 1.2);
        assert!(predicted.new.is_empty());
        assert_eq!(predicted.dropped.iter().map(|o| o.id).collect::<Vec<_>>(), [2]);
        assert!(ledger.expire(70).is_empty());
        ledger.begin(1, AGENT);
        assert!(ledger.confirm(&laser_event(60, 5), &applied, 1.3).unwrap().exact);
        assert_eq!(ledger.confirmed.ids(), [Some(3)]);
    }

    #[test]
    fn identical_presses_are_told_apart_by_their_command() {
        // Two presses with the same aim and muzzle, ending in the same spot,
        // which matching by aim and muzzle couldn't tell apart.
        let mut ledger = ledger();
        let applied = [(28, command(0)), (32, command(1))];
        assert_eq!(ledger.predict(&[laser(30, 5), laser(34, 5)], &applied, 0.0).new.len(), 2);
        // The first never fires in the confirmed world; the second does. It
        // is matched to its own command, and the first is given up.
        let fired = ledger.confirm(&laser_event(32, 5), &applied, 0.2).unwrap();
        assert_eq!((fired.id, fired.exact), (1, true));
        let given_up = ledger.expire(31 + TIMEOUT_TICKS);
        assert_eq!(given_up.iter().map(|o| (o.id, o.confirmed.is_none())).collect::<Vec<_>>(), [(0, true)]);

        // Two presses applied on the same tick: the second waits for the
        // first one's swing, and both are keyed by their press. They are
        // told apart in order, also once the first is confirmed.
        let applied = [(60, command(2)), (60, command(3))];
        let both = [laser(62, 5), PredictedEvent { tick: 70, event: laser_event(60, 5) }];
        assert_eq!(ledger.predict(&both, &applied, 1.0).new.iter().map(|e| e.id).collect::<Vec<_>>(), [2, 3]);
        assert!(ledger.predict(&both, &applied, 1.1).new.is_empty());
        assert_eq!(ledger.confirm(&laser_event(60, 5), &applied, 1.2).unwrap().id, 2);
        // The replay now starts after the first: the second is still the second.
        assert_eq!(ledger.predict(&both[1..], &applied, 1.3), Predicted::default());
        assert_eq!(ledger.confirm(&laser_event(60, 5), &applied, 1.4).unwrap().id, 3);
    }

    #[test]
    fn an_event_whose_command_has_no_record_is_keyed_by_its_start_tick() {
        // A swing loaded with the world: no command of this client's
        // applied on its start tick.
        let mut ledger = ledger();
        assert_eq!(ledger.predict(&[laser(30, 5)], &[], 0.0).new.len(), 1);
        assert!(ledger.confirm(&laser_event(28, 5), &[], 0.1).unwrap().exact);
    }

    /// The test policy: every predictable kind is presented early, and a
    /// laser shot starts something lasting.
    fn lasting_lasers(event: &Event, agent: u16) -> Option<Policy> {
        let Prediction::Predict { agent: owner, started, on_miss } = event.prediction() else { return None };
        let kind = kind_of(event)?;
        let on_miss = if kind == "laser" { OnMiss::Cancel } else { on_miss };
        (owner == agent).then_some(Policy { kind, started, on_miss })
    }

    #[test]
    fn a_kept_event_stays_and_a_cancelled_one_is_stopped_when_it_misses() {
        let mut ledger = EventPrediction { policy: lasting_lasers, ..EventPrediction::default() };
        ledger.begin(1, AGENT);
        let applied = [(10, command(0)), (11, command(1)), (30, command(2))];
        let new = ledger.predict(&[laser(13, 5), arrow(20), laser(32, 6)], &applied, 0.0).new;
        assert_eq!(new.iter().map(|e| (e.id, e.kind)).collect::<Vec<_>>(), [(0, "laser"), (1, "bow"), (2, "laser")]);

        // The first laser shot confirms ending elsewhere: it is stopped, and
        // the confirmed one presented instead of skipped.
        let elsewhere = ledger.confirm(&laser_event(11, 4), &applied, 0.1).unwrap();
        assert_eq!((elsewhere.id, elsewhere.exact, elsewhere.cancel), (0, false, true));
        assert_eq!(ledger.confirmed.ids(), [None]);
        // Neither of the others ever confirms: the arrow (`Keep`) is left,
        // the laser shot stopped.
        let given_up = ledger.expire(33 + TIMEOUT_TICKS);
        let outcomes: Vec<_> = given_up.iter().map(|o| (o.id, o.kind, o.cancel)).collect();
        assert_eq!(outcomes, [(1, "bow", false), (2, "laser", true)]);

        // One that drops out of the replay is stopped at once.
        let applied = [(50, command(3)), (60, command(4))];
        assert_eq!(ledger.predict(&[laser(52, 5)], &applied, 1.0).new.len(), 1);
        let dropped = ledger.predict(&[laser(62, 5)], &applied, 1.1).dropped;
        assert_eq!(dropped.iter().map(|o| (o.id, o.cancel)).collect::<Vec<_>>(), [(3, true)]);
        // One that confirms as presented stays, under either policy.
        let fired = ledger.confirm(&laser_event(60, 5), &applied, 1.2).unwrap();
        assert!(fired.exact && !fired.cancel);
        assert_eq!(ledger.confirmed.ids(), [None, Some(4)]);

        // The app's own policy keeps a laser shot that ended elsewhere.
        let mut kept = self::ledger();
        kept.predict(&[laser(13, 5)], &[(11, command(0))], 0.0);
        let elsewhere = kept.confirm(&laser_event(11, 4), &[(11, command(0))], 0.1).unwrap();
        assert!(!elsewhere.exact && !elsewhere.cancel);
        assert_eq!(kept.confirmed.ids(), [Some(0)]);
    }
}

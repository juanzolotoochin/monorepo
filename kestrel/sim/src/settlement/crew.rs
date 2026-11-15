//! A crawler's crew (TU.4 of the crawler turret, `kestrel/CRAWLER_TURRET.md`
//! T3, T4): sprites on the `SpriteJob::Crew` job climb to a crawler's gun,
//! elders boarding take it from them, and another job climbs them down.
//!
//! - **Climbing up:** the sprite's brain (`sprites`) walks it to the free
//!   spot beside the box nearest it (`crawler::board_spot`) while the
//!   crawler stands still (`crawler::walking`) and nobody mans the gun;
//!   beside the box (`interact::beside_gun`) stage 3 asks
//!   (`Messages::crew_gun`) and stage 5, after the elders' boarding, seats
//!   the first to ask (`crew_guns`, through `man_gun`). A walking crawler
//!   waits for nobody: a sprite on its way, or beside it, waits and plans
//!   again.
//! - **Bumped (T3):** an elder boarding takes the gun from a sprite there
//!   (`Settlement::board`, `bump`): the sprite moves to the free rider's
//!   place with the lowest index (`SpriteState::rider`), or climbs down by
//!   the gunner's climbing-down rules if both are taken. With neither, the
//!   gun isn't offered. A riding sprite does nothing, and nothing hits it.
//! - **Housekeeping (`crew_housekeeping`):** a riding sprite still on the
//!   crew job takes the gun back once it is free; a sprite aboard on any
//!   other job climbs down beside the box, as an elder does, and goes on
//!   with its job. With nowhere to climb down to it stays aboard and tries
//!   again next tick.

use fx::{Fx, FxVec2};

use super::crawler;
use super::sprites::{SpriteJob, SpriteState, Step};
use super::vehicle::VehicleIx;
use super::{AgentIx, AgentState, Role, Settlement};
use crate::messages::CrewGun;
use crate::terrain::Terrain;

/// Where a sprite bumped from a crawler's gun goes (T3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bump {
    /// To this free rider's place.
    Ride(u8),
    /// Down beside the box, at this spot and floor.
    Down(FxVec2, Fx),
}

impl Settlement {
    /// Who mans vehicle `v`'s gun: an elder gunner, or else a sprite.
    pub fn gun_crew(&self, v: VehicleIx) -> Option<AgentIx> {
        self.gunner_of(v).or_else(|| self.sprite_gunner_of(v)).map(|a| a as AgentIx)
    }

    /// The sprite riding vehicle `vehicle` in `place`, if one is.
    pub fn sprite_rider_of(&self, vehicle: VehicleIx, place: u8) -> Option<usize> {
        self.agents.iter().position(|a| a.sprite().is_some_and(|s| s.rider == Some((vehicle, place))))
    }

    /// Whether rider's place `place` of vehicle `vehicle` is free of elders
    /// and sprites.
    pub(super) fn place_free(&self, vehicle: VehicleIx, place: u8) -> bool {
        self.rider_of(vehicle, place).is_none() && self.sprite_rider_of(vehicle, place).is_none()
    }

    /// Where sprite `sprite`, at vehicle `ix`'s gun, goes when an elder
    /// takes it (T3): the free rider's place with the lowest index, else
    /// down by the gunner's climbing-down rules. `None` when neither is
    /// free: the elder doesn't get the gun.
    pub(super) fn bump(&self, ix: VehicleIx, sprite: usize, terrain: &Terrain) -> Option<Bump> {
        if let Some(place) = (0..crawler::RIDERS.len() as u8).find(|&p| self.place_free(ix, p)) {
            return Some(Bump::Ride(place));
        }
        let v = &self.vehicles[ix as usize];
        let radius = self.agents[sprite].radius();
        super::vehicle::exit_spot(v, &self.characters[v.character as usize], Some(crawler::GUNNER), radius, terrain).map(|(p, ground)| Bump::Down(p, ground))
    }

    /// Takes vehicle `ix`'s gun from the sprite there, if one is, for an
    /// elder (T3): it rides or climbs down (`bump`). Returns whether the
    /// gun is free now.
    pub(super) fn bump_sprite_gunner(&mut self, ix: VehicleIx, terrain: &Terrain) -> bool {
        let Some(sprite) = self.sprite_gunner_of(ix) else { return true };
        let Some(bump) = self.bump(ix, sprite, terrain) else { return false };
        let volume = terrain.volume.is_some();
        let AgentState { body, role: Role::Sprite(s), .. } = &mut self.agents[sprite] else { unreachable!("a sprite gunner") };
        s.gun = None;
        match bump {
            Bump::Ride(place) => s.rider = Some((ix, place)),
            Bump::Down(p, ground) => climb_down(body, s, p, ground, volume),
        }
        true
    }

    /// Whether sprite `agent` may climb to vehicle `vehicle`'s gun now: alive,
    /// on the crew job and its `Crew` step for it, not aboard, beside the
    /// still crawler's box (`interact::beside_gun`), and nobody at the gun.
    pub(super) fn crew_may_board(&self, agent: usize, vehicle: VehicleIx, terrain: &Terrain) -> bool {
        let Some(a) = self.agents.get(agent).filter(|a| a.body.alive()) else { return false };
        let Some(sprite) = a.sprite() else { return false };
        let on_step = sprite.job == SpriteJob::Crew && sprite.current() == Some(Step::Crew(vehicle)) && sprite.aboard().is_none();
        on_step
            && self.vehicles.get(vehicle as usize).is_some_and(|v| super::gun::gun_of(&self.characters[v.character as usize]).is_some())
            && self.gun_crew(vehicle).is_none()
            && super::interact::beside_gun(&self.vehicles, &self.characters, terrain, vehicle, a.body.position, sprite.height, a.body.base_radius)
    }

    /// Stage 5, last: crew sprites beside a still crawler climb to its gun,
    /// the first to ask in agent order, after the elders' boarding, so an
    /// elder boarding on the same tick takes it. The others find it taken
    /// and wait.
    pub(crate) fn crew_guns(&mut self, messages: Vec<CrewGun>, terrain: &Terrain) {
        for m in messages {
            if self.crew_may_board(m.agent as usize, m.vehicle, terrain) {
                let manned = self.man_gun(m.agent, m.vehicle);
                debug_assert!(manned, "checked");
            }
        }
    }

    /// Housekeeping (in `seat_housekeeping`, before sprites are put in
    /// their places): a sprite aboard a crawler on any job but `Crew`
    /// climbs down beside the box from its place, and plans for its job; a
    /// riding sprite still on the crew job takes the gun once it is free.
    pub(super) fn crew_housekeeping(&mut self, terrain: &Terrain) {
        let volume = terrain.volume.is_some();
        for i in 0..self.agents.len() {
            let Some(sprite) = self.agents[i].sprite() else { continue };
            let Some(ix) = sprite.aboard() else { continue };
            if sprite.job != SpriteJob::Crew {
                let place = match sprite.rider {
                    Some((_, p)) => crawler::RIDERS[p as usize],
                    None => crawler::GUNNER,
                };
                let v = &self.vehicles[ix as usize];
                let spot = super::vehicle::exit_spot(v, &self.characters[v.character as usize], Some(place), self.agents[i].radius(), terrain);
                let Some((p, ground)) = spot else { continue };
                let AgentState { body, role: Role::Sprite(s), .. } = &mut self.agents[i] else { unreachable!("a sprite") };
                climb_down(body, s, p, ground, volume);
            } else if sprite.rider.is_some() && self.gun_crew(ix).is_none() {
                let s = self.agents[i].sprite_mut().expect("a sprite");
                s.rider = None;
                s.gun = Some(ix);
            }
        }
    }
}

/// Puts a sprite down from a crawler at `p` on floor `ground` (its feet's
/// height in a volume scene, zero elsewhere), with no plan, so it plans for
/// its job on its next step.
fn climb_down(body: &mut super::Body, s: &mut SpriteState, p: FxVec2, ground: Fx, volume: bool) {
    s.gun = None;
    s.rider = None;
    s.plan.clear();
    s.step = 0;
    s.retry_at = 0;
    s.height = if volume { ground } else { Fx::ZERO };
    body.position = p;
    body.route.clear();
    body.velocity = FxVec2::ZERO;
    body.push = FxVec2::ZERO;
}

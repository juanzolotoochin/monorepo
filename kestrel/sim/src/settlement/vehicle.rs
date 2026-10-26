//! Deterministic vehicle occupancy and boarding. Controls continue to drive
//! the player's avatar; a vehicle changes its body, movement, and weapon.

use content::scenario::ticks_from_60hz;
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::{AgentIx, Settlement};
use crate::terrain::Terrain;

pub const MECH_RADIUS: Fx = Fx::from_ratio(4, 5);
pub const MECH_WALK: Fx = Fx::from_ratio(5, 2);
pub const MECH_RUN: Fx = Fx::from_int(4);
pub const BOARD_TICKS: u64 = ticks_from_60hz(54);
pub const EXIT_TICKS: u64 = ticks_from_60hz(48);
pub const BOARD_REACH: Fx = Fx::from_ratio(11, 5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VehicleKind { Mech }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VehiclePhase {
    Parked,
    Boarding { agent: AgentIx, started: u64, from: FxVec2 },
    Driving { agent: AgentIx },
    Exiting { agent: AgentIx, started: u64, to: FxVec2 },
}

impl VehiclePhase {
    pub fn agent(&self) -> Option<AgentIx> {
        match *self {
            Self::Parked => None,
            Self::Boarding { agent, .. } | Self::Driving { agent } | Self::Exiting { agent, .. } => Some(agent),
        }
    }

    pub fn driving(&self, agent: AgentIx) -> bool { matches!(self, Self::Driving { agent: driver } if *driver == agent) }
    pub fn transitioning(&self, agent: AgentIx) -> bool {
        matches!(self, Self::Boarding { agent: driver, .. } | Self::Exiting { agent: driver, .. } if *driver == agent)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleState {
    pub kind: VehicleKind,
    pub position: FxVec2,
    pub heading: Fx,
    pub phase: VehiclePhase,
}

impl Settlement {
    pub fn vehicle_of(&self, agent: AgentIx) -> Option<(usize, &VehicleState)> {
        self.vehicles.iter().enumerate().find(|(_, vehicle)| vehicle.phase.agent() == Some(agent))
    }

    /// Toggle the nearest parked mech, or leave the mech this avatar drives.
    pub fn toggle_vehicle(&mut self, tick: u64, agent: usize, vehicle: u16, terrain: &Terrain) {
        let Some(a) = self.agents.get(agent).filter(|a| a.controlled && (a.avatar.max_health == 0 || a.avatar.health > 0)) else { return };
        let Some(index) = self.vehicles.get(usize::from(vehicle)) else { return };
        match index.phase {
            VehiclePhase::Parked if self.vehicle_of(agent as AgentIx).is_none()
                && (a.position - index.position).length_squared() < BOARD_REACH * BOARD_REACH
                && terrain.clear(a.position, index.position, self.characters[a.character as usize].body.radius)
                && !a.avatar.flight.airborne && a.avatar.swing.is_none() => {
                let from = a.position;
                self.vehicles[vehicle as usize].phase = VehiclePhase::Boarding { agent: agent as AgentIx, started: tick, from };
                let a = &mut self.agents[agent];
                a.route.clear();
                a.avatar.intent = Some(super::avatar::Intent::Stop);
                a.avatar.velocity = FxVec2::ZERO;
                a.avatar.repair = None;
            }
            VehiclePhase::Driving { agent: driver } if driver as usize == agent => {
                let center = index.position;
                let radius = self.characters[a.character as usize].body.radius;
                let offsets = [FxVec2::new(Fx::from_ratio(8, 5), Fx::ZERO),
                    FxVec2::new(-Fx::from_ratio(8, 5), Fx::ZERO),
                    FxVec2::new(Fx::ZERO, Fx::from_ratio(8, 5)),
                    FxVec2::new(Fx::ZERO, -Fx::from_ratio(8, 5))];
                let Some(to) = offsets.into_iter().map(|offset| center + offset)
                    .find(|&to| terrain.walkable(center, to, radius) && terrain.clear(to, to, radius)
                        && self.vehicles.iter().enumerate().all(|(other, v)| other == vehicle as usize
                            || !matches!(v.phase, VehiclePhase::Parked)
                            || (to - v.position).length() > radius + MECH_RADIUS)) else { return };
                self.vehicles[vehicle as usize].phase = VehiclePhase::Exiting { agent: driver, started: tick, to };
                let a = &mut self.agents[agent];
                a.route.clear();
                a.avatar.intent = Some(super::avatar::Intent::Stop);
                a.avatar.velocity = FxVec2::ZERO;
                a.avatar.swing = None;
                a.avatar.queued = None;
                a.avatar.height = self.spawn_height + Fx::from_ratio(7, 5);
            }
            _ => {}
        }
    }

    pub(crate) fn step_vehicles(&mut self, tick: u64) {
        for vehicle in &mut self.vehicles {
            if let Some(agent) = vehicle.phase.agent() {
                if self.agents[agent as usize].avatar.max_health > 0 && self.agents[agent as usize].avatar.health == 0 {
                    let a = &mut self.agents[agent as usize];
                    a.base_radius = self.characters[a.character as usize].body.radius;
                    a.avatar.height = self.spawn_height;
                    vehicle.phase = VehiclePhase::Parked;
                    continue;
                }
            }
            match vehicle.phase {
                VehiclePhase::Parked => {}
                VehiclePhase::Boarding { agent, started, from } => {
                    let a = &mut self.agents[agent as usize];
                    let previous = a.position;
                    let elapsed = tick.saturating_sub(started).min(BOARD_TICKS);
                    let t = Fx::from_ratio(elapsed as i64, BOARD_TICKS as i64);
                    let amount = t * t * (Fx::from_int(3) - Fx::from_int(2) * t);
                    a.position = from + (vehicle.position - from) * amount;
                    a.avatar.velocity = (a.position - previous) * Fx::from_int(content::TICKS_PER_SECOND);
                    let facing = crate::nav::bearing(a.position, vehicle.position);
                    a.heading += crate::nav::angle_between(a.heading, facing) * Fx::from_ratio(1, 5);
                    a.avatar.height = self.spawn_height + Fx::from_ratio(7, 5) * amount;
                    if elapsed == BOARD_TICKS {
                        a.base_radius = MECH_RADIUS;
                        a.avatar.height = self.spawn_height;
                        a.avatar.velocity = FxVec2::ZERO;
                        vehicle.phase = VehiclePhase::Driving { agent };
                    }
                }
                VehiclePhase::Driving { agent } => {
                    let a = &self.agents[agent as usize];
                    vehicle.position = a.position;
                    vehicle.heading = a.heading;
                }
                VehiclePhase::Exiting { agent, started, to } => {
                    let a = &mut self.agents[agent as usize];
                    let previous = a.position;
                    let elapsed = tick.saturating_sub(started).min(EXIT_TICKS);
                    let t = Fx::from_ratio(elapsed as i64, EXIT_TICKS as i64);
                    let amount = t * t * (Fx::from_int(3) - Fx::from_int(2) * t);
                    a.position = vehicle.position + (to - vehicle.position) * amount;
                    a.avatar.velocity = (a.position - previous) * Fx::from_int(content::TICKS_PER_SECOND);
                    let facing = crate::nav::bearing(a.position, to);
                    a.heading += crate::nav::angle_between(a.heading, facing) * Fx::from_ratio(1, 5);
                    a.avatar.height = self.spawn_height + Fx::from_ratio(7, 5) * (Fx::ONE - amount);
                    if elapsed == EXIT_TICKS {
                        a.base_radius = self.characters[a.character as usize].body.radius;
                        a.avatar.height = self.spawn_height;
                        a.avatar.velocity = FxVec2::ZERO;
                        vehicle.phase = VehiclePhase::Parked;
                    }
                }
            }
        }
    }
}

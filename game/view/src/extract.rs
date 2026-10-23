//! The render extract: what the view needs from one tick, converted to f32.
//! Building it is the only place the view reads simulation state.

use sim::World;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoverExtract {
    /// The simulation ID's bits; stable for the mover's lifetime.
    pub id: u64,
    pub kind: u16,
    /// World position in tiles: x and y on the ground, z up.
    pub pos: [f32; 3],
    pub radius: f32,
    pub asleep: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extract {
    /// The tick this extract shows (the last tick that ran).
    pub tick: u64,
    /// Sorted by `id`.
    pub movers: Vec<MoverExtract>,
}

impl Extract {
    pub fn from_world(world: &World) -> Extract {
        let mut movers: Vec<MoverExtract> = world
            .chunks()
            .iter()
            .flat_map(|chunk| {
                let m = &chunk.movers;
                (0..m.len()).map(move |slot| MoverExtract {
                    id: m.id[slot].to_bits(),
                    kind: m.kind[slot],
                    pos: [m.pos[slot].x.to_f32(), m.pos[slot].y.to_f32(), m.pos[slot].z.to_f32()],
                    radius: m.radius[slot].to_f32(),
                    asleep: m.sleep_until[slot] != 0,
                })
            })
            .collect();
        movers.sort_unstable_by_key(|m| m.id);
        Extract { tick: world.tick().saturating_sub(1), movers }
    }

    pub fn mover(&self, id: u64) -> Option<&MoverExtract> {
        self.movers.binary_search_by_key(&id, |m| m.id).ok().map(|i| &self.movers[i])
    }
}

/// A mover as drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub id: u64,
    pub kind: u16,
    pub pos: [f32; 3],
    pub radius: f32,
    pub asleep: bool,
}

/// Blends the previous and current extracts. The view draws one tick behind
/// the simulation, so motion is smooth at any frame rate.
#[derive(Default)]
pub struct Interpolator {
    previous: Extract,
    current: Extract,
}

impl Interpolator {
    pub fn push(&mut self, extract: Extract) {
        self.previous = std::mem::replace(&mut self.current, extract);
    }

    pub fn current(&self) -> &Extract {
        &self.current
    }

    /// Every mover in the current extract at `alpha` (0 = previous tick,
    /// 1 = current tick). Movers that are new this tick appear at their
    /// current position instead of sliding in from nowhere.
    pub fn sample(&self, alpha: f32) -> impl Iterator<Item = Drawn> + '_ {
        let alpha = alpha.clamp(0.0, 1.0);
        self.current.movers.iter().map(move |now| {
            let pos = match self.previous.mover(now.id) {
                Some(before) => std::array::from_fn(|i| before.pos[i] + (now.pos[i] - before.pos[i]) * alpha),
                None => now.pos,
            };
            Drawn { id: now.id, kind: now.kind, pos, radius: now.radius, asleep: now.asleep }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mover(id: u64, x: f32) -> MoverExtract {
        MoverExtract { id, kind: 0, pos: [x, 0.0, 0.0], radius: 0.5, asleep: false }
    }

    #[test]
    fn interpolates_between_ticks_and_pops_in_new_movers() {
        let mut view = Interpolator::default();
        view.push(Extract { tick: 0, movers: vec![mover(1, 0.0)] });
        view.push(Extract { tick: 1, movers: vec![mover(1, 2.0), mover(2, 5.0)] });
        let drawn: Vec<Drawn> = view.sample(0.25).collect();
        assert_eq!(drawn[0].pos[0], 0.5);
        assert_eq!(drawn[1].pos[0], 5.0);
        assert_eq!(view.sample(7.0).next().unwrap().pos[0], 2.0);
    }

    #[test]
    fn extract_matches_world() {
        let scenario = sim::scenario::Scenario::small();
        let mut sim = scenario.start(2);
        scenario.run_from(&mut sim, 3);
        let extract = Extract::from_world(sim.world());
        assert_eq!(extract.tick, 2);
        assert_eq!(extract.movers.len(), sim.world().mover_count());
        assert!(extract.movers.windows(2).all(|w| w[0].id < w[1].id));
        let first = extract.movers[0];
        let row = sim.world().mover(sim::Id::from_bits(first.id)).unwrap();
        assert_eq!(first.pos[0], row.pos.x.to_f32());
    }
}

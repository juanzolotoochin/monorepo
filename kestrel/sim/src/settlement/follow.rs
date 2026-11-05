//! The test walker (V.7; V8 of the verticality design,
//! `kestrel/VERTICALITY.md`): a sprite whose job is `SpriteJob::Follow`. It
//! follows the player through the volume, the model's proof before sprites
//! and ghouls path through tunnels, and does nothing else.
//!
//! Every `REPLAN_TICKS` it plans a 3D path (`nav::path_at`) to the floor
//! the player stands on, and walks it at its character's walk: each step a
//! straight walk on foot (`Terrain::walk_at`), its feet on the floor it
//! reaches (`SpriteState::height`). Within `KEEP` of the player on the
//! same floor it stops. A step the ground refuses slides along the wall
//! (its x or y alone) if it can (V.8: a path's 0.1 m samples can skip a
//! corner a step clips, and it stood there replanning every tick); if not
//! (the player dug, or the path went stale) it drops the route and plans
//! again next tick. It steers round
//! no one: it keeps its distance from the player, and the Tunnel Lab has
//! no one else.

use fx::{Fx, FxVec2};

use super::haulers::Ctx;
use super::sprites::SpriteState;
use super::Body;
use crate::nav::{self, distance};

/// How often it plans again: half a second.
pub const REPLAN_TICKS: u64 = 30;
/// How far from the player it stops: 2 m.
pub const KEEP: Fx = Fx::from_bits(2 << 32);
/// How far apart in height the player and it may be and still be on one
/// floor: floors in a column are more than this apart.
const SAME_FLOOR: Fx = Fx::ONE;

/// Stage 2 for the walker: plan when due, then walk a step of `step`
/// meters along the route.
pub(super) fn self_update(body: &mut Body, sprite: &mut SpriteState, ctx: &Ctx, step: Fx) {
    let Some((leader, leader_height)) = ctx.leader else { return };
    let radius = body.base_radius;
    if distance(body.position, leader) <= KEEP && (leader_height - sprite.height).abs() <= SAME_FLOOR {
        body.route.clear();
        return;
    }
    if ctx.tick >= sprite.retry_at {
        sprite.retry_at = ctx.tick + REPLAN_TICKS;
        let path = nav::path_at(ctx.terrain, body.position, sprite.height, leader, Some(leader_height), radius, &[]);
        body.route = path.into_iter().map(|(p, _)| p).collect();
    }
    let mut left = step;
    while left > Fx::ZERO {
        let Some(&target) = body.route.first() else { return };
        let gap = distance(body.position, target);
        let next = if gap <= left { target } else { body.position + (target - body.position) * (left / gap) };
        let Some(feet) = ctx.terrain.walk_at(body.position, next, sprite.height, radius) else {
            // A path checks its segments every 0.1 m, so a step can still
            // clip a wall's corner the samples skipped: slide along it
            // (the step's x or y alone) and go on next tick.
            let d = next - body.position;
            let slid = [FxVec2::new(d.x, Fx::ZERO), FxVec2::new(Fx::ZERO, d.y)]
                .into_iter()
                .filter(|&s| s != FxVec2::ZERO)
                .find_map(|s| ctx.terrain.walk_at(body.position, body.position + s, sprite.height, radius).map(|feet| (body.position + s, feet)));
            if let Some((p, feet)) = slid {
                body.heading = nav::bearing(body.position, p);
                body.travelled += distance(body.position, p);
                body.position = p;
                sprite.height = feet;
                return;
            }
            // Stale (the player dug, or the ground changed): plan again
            // next tick.
            body.route.clear();
            sprite.retry_at = ctx.tick + 1;
            return;
        };
        if next != body.position {
            body.heading = nav::bearing(body.position, next);
        }
        body.travelled += distance(body.position, next);
        body.position = next;
        sprite.height = feet;
        if next == target {
            body.route.remove(0);
        }
        left -= gap.min(left);
        if distance(body.position, leader) <= KEEP && (leader_height - sprite.height).abs() <= SAME_FLOOR {
            body.route.clear();
            return;
        }
    }
}

/// Where a walker placed at `p` starts: on the floor nearest the ground
/// (`Terrain::footprint_near` at 0 m) in a volume scene; elsewhere at 0.
pub(super) fn start_height(terrain: &crate::terrain::Terrain, p: FxVec2, radius: Fx) -> Fx {
    if terrain.volume.is_none() {
        return Fx::ZERO;
    }
    terrain.footprint_near(p, Fx::ZERO, radius).unwrap_or_default()
}

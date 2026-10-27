//! Kestrel: pooled bursts (VFX spike step 4, see `UPSTREAM.md`).
//!
//! Upstream gives every emitter of every particle system its own particle
//! buffers, material and mesh entity, so each burst is drawn (and binned by
//! Bevy, in every view) on its own. Effects that fire many short bursts then
//! cost a draw per emitter per burst per view.
//!
//! Here, emitters that allow it share a pool per (asset, emitter): one
//! particle buffer split into slots of `particles_amount`, one material and
//! one mesh entity drawing every slot. A burst still has its emitter entity
//! and runtime (time steps, seed, trigger), simulates only its slot (the
//! `slot_offset` uniform) and frees the slot when it despawns. The freed
//! range is zeroed before the next draw. A full pool chains a bigger one.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::asset::{EmitterData, ParticlesAsset};
use crate::material::ParticleEmitterUniforms;
use crate::runtime::{ParticleBufferHandle, ParticleData};

/// Whether emitters that allow it are pooled (the default).
/// `KESTREL_SPRINKLES_NO_POOL=1` gives every burst its own buffers and draw,
/// as upstream does, for A/B measurements.
pub fn enabled() -> bool {
    static CHECKED: AtomicBool = AtomicBool::new(false);
    static ENABLED: AtomicBool = AtomicBool::new(true);
    if !CHECKED.swap(true, Ordering::Relaxed) {
        let off = std::env::var("KESTREL_SPRINKLES_NO_POOL").is_ok_and(|v| v == "1");
        ENABLED.store(!off, Ordering::Relaxed);
    }
    ENABLED.load(Ordering::Relaxed)
}

/// Slots in the first pool of an emitter; each further pool doubles, up to
/// [`MAX_CAPACITY`].
const FIRST_CAPACITY: u32 = 32;
const MAX_CAPACITY: u32 = 256;

/// Whether emitter `index` of `asset` can be pooled: world space, no trails,
/// no sub-emitters either way.
pub(crate) fn eligible(asset: &ParticlesAsset, index: usize, is_editor: bool) -> bool {
    let emitter = &asset.emitters[index];
    enabled()
        && !is_editor
        && emitter.emission.particles_amount > 0
        && emitter.trail_size() <= 1
        && !emitter.draw_pass.use_local_coords
        && emitter.sub_emitter.is_none()
        && !asset
            .emitters
            .iter()
            .any(|e| e.sub_emitter.as_ref().is_some_and(|s| s.target_emitter == index))
}

/// The pools one emitter's bursts can go into.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct PoolKey {
    pub(crate) asset: AssetId<ParticlesAsset>,
    pub(crate) emitter: usize,
}

pub(crate) struct Pool {
    /// The entity that draws every slot.
    pub(crate) entity: Entity,
    pub(crate) capacity: u32,
    /// Particles per slot (the emitter's `particles_amount`).
    pub(crate) amount: u32,
    pub(crate) draw_order: u32,
    pub(crate) buffers: ParticleBufferHandle,
    /// The material's uniforms, rewritten each frame with the frame's time step.
    pub(crate) uniforms: ParticleEmitterUniforms,
    free: Vec<u32>,
    pub(crate) used: u32,
    /// Whether a slot was freed this frame: the pool is still drawn this
    /// frame, so it must still be sorted (with the freed slots zeroed).
    pub(crate) freed_this_frame: bool,
}

impl Pool {
    pub(crate) fn new(
        entity: Entity,
        capacity: u32,
        emitter: &EmitterData,
        draw_order: u32,
        buffers: ParticleBufferHandle,
        uniforms: ParticleEmitterUniforms,
    ) -> Self {
        Pool {
            entity,
            capacity,
            amount: emitter.emission.particles_amount,
            draw_order,
            buffers,
            uniforms,
            // Lowest slots first.
            free: (0..capacity).rev().collect(),
            used: 0,
            freed_this_frame: false,
        }
    }
}

/// Every pool, and the slots freed this frame.
#[derive(Resource, Default)]
pub struct ParticlePools {
    pub(crate) pools: HashMap<PoolKey, Vec<Pool>>,
    released: Vec<PoolSlot>,
    /// Byte ranges of particle buffers to zero before this frame's
    /// simulation: the slots freed this frame.
    pub(crate) clears: Vec<(Handle<ShaderBuffer>, u64, u64)>,
}

impl ParticlePools {
    /// Claims a slot for a burst of `key`, creating a pool (with `create`,
    /// given the new pool's capacity) if every pool is full.
    pub(crate) fn claim(&mut self, key: PoolKey, create: impl FnOnce(u32) -> Pool) -> PoolSlot {
        let pools = self.pools.entry(key).or_default();
        let index = match pools.iter().position(|p| !p.free.is_empty()) {
            Some(index) => index,
            None => {
                let capacity = pools.last().map_or(FIRST_CAPACITY, |p| (p.capacity * 2).min(MAX_CAPACITY));
                pools.push(create(capacity));
                pools.len() - 1
            }
        };
        let pool = &mut pools[index];
        let slot = pool.free.pop().unwrap();
        pool.used += 1;
        PoolSlot { key, pool: pool.entity, slot, offset: slot * pool.amount }
    }

    /// How many pools there are, and how many slots are in use.
    pub fn counts(&self) -> (usize, u32) {
        let pools = self.pools.values().flatten();
        pools.fold((0, 0), |(n, used), p| (n + 1, used + p.used))
    }
}

/// On a pooled burst's emitter entity: its slot. Removing it (despawning
/// the burst) frees the slot.
#[derive(Component, Clone, Copy, Debug)]
#[component(on_remove = release_slot)]
pub struct PoolSlot {
    pub(crate) key: PoolKey,
    pub(crate) pool: Entity,
    pub(crate) slot: u32,
    /// The slot's first particle in the pool's buffer.
    pub(crate) offset: u32,
}

fn release_slot(mut world: DeferredWorld, context: HookContext) {
    let Some(&slot) = world.get::<PoolSlot>(context.entity) else { return };
    if let Some(mut pools) = world.get_resource_mut::<ParticlePools>() {
        pools.released.push(slot);
    }
}

/// Marks the entity that draws a pool.
#[derive(Component)]
pub struct ParticlePoolDraw;

/// In `Last`: frees the slots released this frame (their particles are
/// zeroed before this frame's simulation, see `compute.rs`), hides empty
/// pools and writes each pool's material uniforms.
pub(crate) fn maintain_pools(
    time: Res<Time>,
    mut pools: ResMut<ParticlePools>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut draws: Query<&mut Visibility, With<ParticlePoolDraw>>,
) {
    let pools = &mut *pools;
    pools.clears.clear();
    for pool in pools.pools.values_mut().flatten() {
        pool.freed_this_frame = false;
    }
    let stride = std::mem::size_of::<ParticleData>() as u64;
    for released in pools.released.drain(..) {
        let Some(pool) = pools.pools.get_mut(&released.key).and_then(|v| v.iter_mut().find(|p| p.entity == released.pool))
        else {
            continue;
        };
        pool.free.push(released.slot);
        pool.used -= 1;
        pool.freed_this_frame = true;
        let amount = u64::from(pool.amount);
        pools.clears.push((pool.buffers.particle_buffer.clone(), u64::from(released.slot) * amount * stride, amount * stride));
    }
    for pool in pools.pools.values_mut().flatten() {
        if let Ok(mut visibility) = draws.get_mut(pool.entity) {
            let wanted = if pool.used > 0 { Visibility::Inherited } else { Visibility::Hidden };
            visibility.set_if_neq(wanted);
        }
        if pool.used == 0 {
            continue;
        }
        pool.uniforms.delta_time = time.delta_secs();
        if let Some(mut buffer) = buffers.get_mut(&pool.buffers.emitter_uniforms_buffer)
            && let Some(slot) = buffer.cast_slice_mut::<ParticleEmitterUniforms>().and_then(|s| s.first_mut())
        {
            *slot = pool.uniforms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(world: &mut World, capacity: u32) -> Pool {
        let mut emitter = EmitterData::default();
        emitter.emission.particles_amount = 12;
        let buffers = ParticleBufferHandle {
            particle_buffer: Handle::default(),
            indices_buffer: Handle::default(),
            sorted_particles_buffer: Handle::default(),
            emitter_uniforms_buffer: Handle::default(),
            max_particles: 12 * capacity,
            amount: 12 * capacity,
            trail_size: 1,
            trail_history_buffer: None,
            trail_history_frames: 0,
        };
        Pool::new(world.spawn_empty().id(), capacity, &emitter, 0, buffers, ParticleEmitterUniforms::default())
    }

    #[test]
    fn bursts_fill_a_pool_then_chain_a_bigger_one() {
        let mut world = World::new();
        let mut pools = ParticlePools::default();
        let key = PoolKey { asset: AssetId::default(), emitter: 0 };
        let slots: Vec<PoolSlot> = (0..FIRST_CAPACITY + 1).map(|_| pools.claim(key, |c| pool(&mut world, c))).collect();
        // Lowest slots first, each `amount` particles further on.
        assert_eq!((slots[0].slot, slots[0].offset), (0, 0));
        assert_eq!((slots[1].slot, slots[1].offset), (1, 12));
        // The 33rd burst opens a second pool, twice as big.
        let pools_of_key = &pools.pools[&key];
        assert_eq!(pools_of_key.len(), 2);
        assert_eq!(pools_of_key[1].capacity, FIRST_CAPACITY * 2);
        assert_ne!(slots[FIRST_CAPACITY as usize].pool, slots[0].pool);
        assert_eq!(pools.counts(), (2, FIRST_CAPACITY + 1));
    }

    #[test]
    fn a_despawned_burst_frees_its_slot_and_zeroes_it() {
        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Assets<ShaderBuffer>>();
        let mut pools = ParticlePools::default();
        let key = PoolKey { asset: AssetId::default(), emitter: 0 };
        let first = pools.claim(key, |c| pool(&mut world, c));
        let second = pools.claim(key, |c| pool(&mut world, c));
        world.insert_resource(pools);
        let burst = world.spawn(second).id();
        world.spawn(first);
        world.despawn(burst);
        world.run_system_cached(maintain_pools).unwrap();
        let pools = world.resource::<ParticlePools>();
        assert_eq!(pools.counts(), (1, 1));
        let stride = std::mem::size_of::<ParticleData>() as u64;
        assert_eq!(pools.clears.iter().map(|c| (c.1, c.2)).collect::<Vec<_>>(), vec![(12 * stride, 12 * stride)]);
        // The freed slot is the next one claimed.
        let mut pools = world.remove_resource::<ParticlePools>().unwrap();
        assert_eq!(pools.claim(key, |_| unreachable!()).slot, second.slot);
    }
}

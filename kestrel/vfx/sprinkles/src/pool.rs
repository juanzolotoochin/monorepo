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
//!
//! Bursts take the lowest free slot, and a pool draws (and sorts) only up to
//! its highest used slot, rounded up to a power of two ([`drawn_slots`]),
//! with a mesh of that many slots: the mesh's particle indices read the
//! sorted buffer in order, so a smaller mesh draws the first slots, and
//! every live particle is in them.
//!
//! A pool empty for [`IDLE_SECONDS`] of game time is dropped, except the
//! first pool of each (asset, emitter): the chained, bigger pools hold most
//! of the memory, and keeping the first spares an effect fired every few
//! seconds a new pool (buffers, material, upload) per shot. When an asset
//! changes, its pools are dropped and its bursts restart on new ones
//! ([`rebuild_changed_pools`]).
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::asset::{EmitterData, ParticleMesh, ParticlesAsset};
use crate::material::ParticleEmitterUniforms;
use crate::mesh::ParticleMeshCache;
use crate::runtime::{ColliderEntity, EmitterEntity, ParticleBufferHandle, ParticleData, ParticleSystemRuntime};

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
/// The fewest slots a pool draws.
const MIN_DRAWN: u32 = 8;
/// Seconds of game time a pool (other than an emitter's first) stays empty
/// before it is dropped: long enough to ride out a lull in a fight, short
/// enough that a spike of load gives its memory back soon after.
pub const IDLE_SECONDS: f32 = 5.0;

/// How many slots a pool of `capacity` draws when its highest used slot is
/// `high - 1`: a power of two, so a pool needs at most a few meshes (about
/// twice its full mesh in all).
pub(crate) fn drawn_slots(high: u32, capacity: u32) -> u32 {
    high.next_power_of_two().max(MIN_DRAWN).min(capacity)
}

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
    /// The emitter's particle mesh, to build the pool's smaller meshes.
    mesh: ParticleMesh,
    /// Per slot, whether a burst holds it.
    occupied: Vec<bool>,
    pub(crate) used: u32,
    /// How many slots, from the first, are drawn and sorted; set each frame
    /// in [`maintain_pools`].
    pub(crate) drawn: u32,
    /// Whether a slot was freed this frame: the pool is still drawn this
    /// frame, so it must still be sorted (with the freed slots zeroed).
    pub(crate) freed_this_frame: bool,
    /// Seconds of game time the pool has been empty.
    idle: f32,
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
            mesh: emitter.draw_pass.mesh.clone(),
            occupied: vec![false; capacity as usize],
            used: 0,
            drawn: capacity,
            freed_this_frame: false,
            idle: 0.0,
        }
    }

    /// One past the highest used slot (0 if none is).
    fn high(&self) -> u32 {
        self.occupied.iter().rposition(|&o| o).map_or(0, |i| i as u32 + 1)
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
    /// Cached meshes dropped pools may have used, released in the next
    /// [`maintain_pools`] (once the pools' draws are despawned) if nothing
    /// else holds them.
    stale_meshes: Vec<(ParticleMesh, u32)>,
}

impl ParticlePools {
    /// Claims a slot for a burst of `key`, creating a pool (with `create`,
    /// given the new pool's capacity) if every pool is full.
    pub(crate) fn claim(&mut self, key: PoolKey, create: impl FnOnce(u32) -> Pool) -> PoolSlot {
        let pools = self.pools.entry(key).or_default();
        let index = match pools.iter().position(|p| p.used < p.capacity) {
            Some(index) => index,
            None => {
                let capacity = pools.last().map_or(FIRST_CAPACITY, |p| (p.capacity * 2).min(MAX_CAPACITY));
                pools.push(create(capacity));
                pools.len() - 1
            }
        };
        let pool = &mut pools[index];
        // The lowest free slot, so used slots stay at the front.
        let slot = pool.occupied.iter().position(|&o| !o).unwrap() as u32;
        pool.occupied[slot as usize] = true;
        pool.used += 1;
        PoolSlot { key, pool: pool.entity, slot, offset: slot * pool.amount }
    }

    /// Drops `pool`: despawns its draw (its material goes with it, its
    /// buffers with their last handles) and queues its meshes for release.
    fn drop_pool(&mut self, pool: Pool, commands: &mut Commands) {
        commands.entity(pool.entity).try_despawn();
        // Every size `drawn_slots` gives (powers of two), up to the full pool.
        let mut slots = MIN_DRAWN.min(pool.capacity);
        loop {
            self.stale_meshes.push((pool.mesh.clone(), slots * pool.amount));
            if slots >= pool.capacity {
                break;
            }
            slots = (slots * 2).min(pool.capacity);
        }
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

/// In `PostUpdate`, after the frame's bursts are set up (last in
/// `SprinklesLateSetup`): sizes each pool's draw to its used slots. The mesh
/// and the sort (`extract.rs`) both follow `drawn`. This runs before the
/// asset events flush and materials re-specialize, so a new mesh and the
/// changed `Mesh3d` are drawn this frame; from `Last` they would be a frame
/// late (the pool undrawn, or binned with the old mesh). Slots freed in
/// `Last` shrink the draw a frame later, which only draws zeroed slots.
pub(crate) fn size_pool_draws(
    mut pools: ResMut<ParticlePools>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mesh_cache: ResMut<ParticleMeshCache>,
    mut draws: Query<&mut Mesh3d, With<ParticlePoolDraw>>,
) {
    for pool in pools.pools.values_mut().flatten() {
        pool.drawn = drawn_slots(pool.high(), pool.capacity);
        if let Ok(mut mesh) = draws.get_mut(pool.entity) {
            let handle = mesh_cache.get_or_create(&pool.mesh, pool.drawn * pool.amount, &mut meshes);
            mesh.set_if_neq(Mesh3d(handle));
        }
    }
}

/// In `Last`: frees the slots released this frame (their particles are
/// zeroed before this frame's simulation, see `compute.rs`), drops pools
/// empty for [`IDLE_SECONDS`] (but an emitter's first), hides empty pools
/// and writes each pool's material uniforms.
pub(crate) fn maintain_pools(
    mut commands: Commands,
    time: Res<Time>,
    mut pools: ResMut<ParticlePools>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut mesh_cache: ResMut<ParticleMeshCache>,
    mut draws: Query<&mut Visibility, With<ParticlePoolDraw>>,
) {
    let pools = &mut *pools;
    for (mesh, count) in pools.stale_meshes.drain(..) {
        mesh_cache.release_unused(&mesh, count);
    }
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
        pool.occupied[released.slot as usize] = false;
        pool.used -= 1;
        pool.freed_this_frame = true;
        let amount = u64::from(pool.amount);
        pools.clears.push((pool.buffers.particle_buffer.clone(), u64::from(released.slot) * amount * stride, amount * stride));
    }
    let delta = time.delta_secs();
    let mut idle = Vec::new();
    for list in pools.pools.values_mut() {
        for pool in list.iter_mut() {
            pool.idle = if pool.used == 0 { pool.idle + delta } else { 0.0 };
        }
        // The first pool of an emitter stays.
        let mut index = 1;
        while index < list.len() {
            if list[index].idle > IDLE_SECONDS {
                idle.push(list.remove(index));
            } else {
                index += 1;
            }
        }
    }
    for pool in idle {
        pools.drop_pool(pool, &mut commands);
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

/// In `PostUpdate`, after the asset events flush: when a particle asset is
/// modified or removed, drops its pools and restarts every system with a
/// burst in them (its emitter and collider entities are despawned and its
/// runtime removed, so `setup_particle_systems` sets it up again next
/// frame, on new pools). A pool's slot size, mesh and material come from
/// the asset, and a burst left on an old pool would be simulated with the
/// new asset's particle count over the old slots. A restarted burst plays
/// again from its start, where it was triggered. Unpooled emitters keep
/// upstream's live `sync_*` updates and aren't restarted.
///
/// An asset changed after the events flush (later in `PostUpdate`, or in
/// `Last`) is seen a frame later, and its bursts run a frame on the old
/// pools: change assets in `Update`.
pub(crate) fn rebuild_changed_pools(
    mut commands: Commands,
    mut events: MessageReader<AssetEvent<ParticlesAsset>>,
    mut pools: ResMut<ParticlePools>,
    bursts: Query<(&EmitterEntity, &PoolSlot)>,
    emitters: Query<(Entity, &EmitterEntity)>,
    colliders: Query<(Entity, &ColliderEntity)>,
) {
    let changed: HashSet<AssetId<ParticlesAsset>> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => Some(*id),
            _ => None,
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    let keys: Vec<PoolKey> = pools.pools.keys().filter(|key| changed.contains(&key.asset)).copied().collect();
    for key in keys {
        for pool in pools.pools.remove(&key).unwrap_or_default() {
            pools.drop_pool(pool, &mut commands);
        }
    }
    let systems: HashSet<Entity> = bursts
        .iter()
        .filter(|(_, slot)| changed.contains(&slot.key.asset))
        .map(|(emitter, _)| emitter.parent_system)
        .collect();
    let children = emitters.iter().map(|(entity, e)| (entity, e.parent_system));
    let children = children.chain(colliders.iter().map(|(entity, c)| (entity, c.parent_system)));
    for (entity, _) in children.filter(|(_, system)| systems.contains(system)) {
        commands.entity(entity).despawn();
    }
    for system in systems {
        if let Ok(mut system) = commands.get_entity(system) {
            system.try_remove::<ParticleSystemRuntime>();
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
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<ParticleMeshCache>();
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

    #[test]
    fn a_pool_draws_up_to_its_highest_used_slot() {
        assert_eq!(drawn_slots(0, 32), MIN_DRAWN);
        assert_eq!(drawn_slots(9, 32), 16);
        assert_eq!(drawn_slots(17, 32), 32);
        assert_eq!(drawn_slots(200, 128), 128);

        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Assets<ShaderBuffer>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<ParticleMeshCache>();
        let mut pools = ParticlePools::default();
        let key = PoolKey { asset: AssetId::default(), emitter: 0 };
        let slots: Vec<PoolSlot> = (0..10).map(|_| pools.claim(key, |c| pool(&mut world, c))).collect();
        let draw = world.spawn((ParticlePoolDraw, Visibility::Inherited, Mesh3d::default())).id();
        pools.pools.get_mut(&key).unwrap()[0].entity = draw;
        world.insert_resource(pools);
        let bursts: Vec<Entity> = slots.iter().map(|s| world.spawn(PoolSlot { pool: draw, ..*s }).id()).collect();
        world.run_system_cached(size_pool_draws).unwrap();
        let drawn = |world: &World| world.resource::<ParticlePools>().pools[&key][0].drawn;
        assert_eq!(drawn(&world), 16);
        let first_mesh = world.get::<Mesh3d>(draw).unwrap().0.clone();
        // Freeing the two highest slots shrinks the draw, with a smaller mesh.
        world.despawn(bursts[9]);
        world.despawn(bursts[8]);
        world.run_system_cached(maintain_pools).unwrap();
        world.run_system_cached(size_pool_draws).unwrap();
        assert_eq!(drawn(&world), MIN_DRAWN);
        assert_ne!(world.get::<Mesh3d>(draw).unwrap().0, first_mesh);
        // A new burst takes the lowest free slot.
        let mut pools = world.remove_resource::<ParticlePools>().unwrap();
        assert_eq!(pools.claim(key, |_| unreachable!()).slot, 8);
    }

    #[test]
    fn an_empty_pool_is_dropped_after_a_while_except_an_emitters_first() {
        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Assets<ShaderBuffer>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<ParticleMeshCache>();
        let mut pools = ParticlePools::default();
        let key = PoolKey { asset: AssetId::default(), emitter: 0 };
        let slots: Vec<PoolSlot> = (0..FIRST_CAPACITY + 1).map(|_| pools.claim(key, |c| pool(&mut world, c))).collect();
        let (first, second) = (pools.pools[&key][0].entity, pools.pools[&key][1].entity);
        // The second pool's 8-slot mesh, cached and drawn by nothing else.
        let mesh = pools.pools[&key][1].mesh.clone();
        world.resource_scope(|world, mut cache: Mut<ParticleMeshCache>| {
            cache.get_or_create(&mesh, MIN_DRAWN * 12, &mut world.resource_mut::<Assets<Mesh>>());
        });
        world.insert_resource(pools);
        let bursts: Vec<Entity> = slots.iter().map(|s| world.spawn(*s).id()).collect();
        for burst in bursts {
            world.despawn(burst);
        }
        let wait = |world: &mut World, seconds: f32| {
            world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(seconds));
            world.run_system_cached(maintain_pools).unwrap();
        };
        wait(&mut world, 0.0);
        wait(&mut world, IDLE_SECONDS - 1.0);
        assert_eq!(world.resource::<ParticlePools>().counts(), (2, 0));
        wait(&mut world, 2.0);
        assert_eq!(world.resource::<ParticlePools>().counts(), (1, 0));
        assert!(world.get_entity(second).is_err());
        assert!(world.get_entity(first).is_ok());
        // Its unused meshes leave the cache a frame later.
        assert!(world.resource::<ParticleMeshCache>().contains(&mesh, MIN_DRAWN * 12));
        wait(&mut world, 0.0);
        assert!(!world.resource::<ParticleMeshCache>().contains(&mesh, MIN_DRAWN * 12));
    }

    #[test]
    fn a_changed_asset_drops_its_pools_and_restarts_its_bursts() {
        let mut world = World::new();
        world.init_resource::<bevy::ecs::message::Messages<AssetEvent<ParticlesAsset>>>();
        let mut pools = ParticlePools::default();
        let key = PoolKey { asset: AssetId::default(), emitter: 0 };
        let other = PoolKey { asset: AssetId::Uuid { uuid: bevy::asset::uuid::Uuid::from_u128(7) }, emitter: 0 };
        let slot = pools.claim(key, |c| pool(&mut world, c));
        let other_slot = pools.claim(other, |c| pool(&mut world, c));
        world.insert_resource(pools);
        let system = world.spawn(ParticleSystemRuntime::default()).id();
        let burst = world.spawn((EmitterEntity { parent_system: system }, slot)).id();
        let other_system = world.spawn(ParticleSystemRuntime::default()).id();
        let other_burst = world.spawn((EmitterEntity { parent_system: other_system }, other_slot)).id();
        world.write_message(AssetEvent::<ParticlesAsset>::Modified { id: key.asset });
        world.run_system_cached(rebuild_changed_pools).unwrap();
        let pools = world.resource::<ParticlePools>();
        assert!(!pools.pools.contains_key(&key));
        assert!(pools.pools.contains_key(&other));
        assert!(world.get_entity(slot.pool).is_err());
        // The system is set up again (no runtime), without its old burst.
        assert!(world.get_entity(burst).is_err());
        assert!(world.get::<ParticleSystemRuntime>(system).is_none());
        // Other assets' bursts carry on.
        assert!(world.get_entity(other_burst).is_ok());
        assert!(world.get::<ParticleSystemRuntime>(other_system).is_some());
    }
}

use bevy::{
    light::NotShadowCaster, pbr::ExtendedMaterial, prelude::*, render::storage::ShaderBuffer,
};

use crate::{
    asset::{DrawPassMaterial, EmitterData, EmitterTrail, ParticlesAsset},
    material::{ParticleEmitterUniforms, ParticleMaterialExtension, TRAIL_THICKNESS_CURVE_SAMPLES},
    mesh::ParticleMeshCache,
    runtime::{
        ColliderEntity, CurrentMaterialConfig, CurrentMeshConfig, EditorMode, EmitterEntity,
        EmitterRuntime, ParticleBufferHandle, ParticleData, ParticleMaterial,
        ParticleMaterialHandle, ParticleMeshHandle, ParticleSystemRuntime, Particles3d,
        ParticleTrigger, ParticlesCollider3D, SimulationStep, SubEmitterBufferHandle,
        TrailHistoryEntry,
    },
};

const MAX_FRAME_DELTA: f32 = 0.1;
const INACTIVE_GRACE_FACTOR: f32 = 1.2;
const MAX_TRAIL_HISTORY_FPS: f32 = 240.0;
fn create_trail_history_buffer(
    amount: u32,
    frames: u32,
    buffers: &mut Assets<ShaderBuffer>,
) -> Option<Handle<ShaderBuffer>> {
    if frames > 0 {
        let data = vec![TrailHistoryEntry::default(); (amount * frames) as usize];
        Some(buffers.add(ShaderBuffer::from(data)))
    } else {
        None
    }
}

fn compute_trail_history_frames(emitter: &EmitterData) -> u32 {
    let trail_size = emitter.trail_size();
    if trail_size <= 1 {
        return 0;
    }
    let effective_fps = if emitter.time.fixed_fps > 0 {
        emitter.time.fixed_fps as f32
    } else {
        MAX_TRAIL_HISTORY_FPS
    };
    let from_stretch = (emitter.trail.stretch_time * effective_fps).ceil() as u32;
    trail_size.max(from_stretch).max(2)
}

fn get_particle_asset<'a>(
    parent_system: Entity,
    particle_systems: &Query<&Particles3d>,
    assets: &'a Assets<ParticlesAsset>,
) -> Option<&'a ParticlesAsset> {
    let particle_system = particle_systems.get(parent_system).ok()?;
    assets.get(particle_system)
}

fn get_emitter_data<'a>(
    parent_system: Entity,
    emitter_index: usize,
    particle_systems: &Query<&Particles3d>,
    assets: &'a Assets<ParticlesAsset>,
) -> Option<&'a EmitterData> {
    get_particle_asset(parent_system, particle_systems, assets)
        .and_then(|asset| asset.emitters.get(emitter_index))
}

fn get_editor_assets_folders<'a>(
    parent_system: Entity,
    is_editor: bool,
    particle_systems: &Query<&Particles3d>,
    assets: &'a Assets<ParticlesAsset>,
) -> &'a [String] {
    if !is_editor {
        return &[];
    }
    get_particle_asset(parent_system, particle_systems, assets)
        .map(|a| a.sprinkles_editor.assets_folder.as_slice())
        .unwrap_or(&[])
}

/// Kestrel: marks emitters set up after transform propagation this frame, so
/// only they get the late time step (see `lib.rs`).
#[derive(Component)]
pub(crate) struct SetUpLate;

/// Kestrel: the late pass's time step, for emitters [`SetUpLate`] only.
pub(crate) fn update_late_particle_time(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<Assets<ParticlesAsset>>,
    system_query: Query<(&Particles3d, &ParticleSystemRuntime)>,
    mut emitter_query: Query<(Entity, &EmitterEntity, &mut EmitterRuntime), With<SetUpLate>>,
) {
    let _timer = crate::stats::UPDATE_NS.time();
    for (entity, ..) in &emitter_query {
        commands.entity(entity).remove::<SetUpLate>();
    }
    step_particle_time(&time, &assets, &system_query, emitter_query.iter_mut().map(|(_, e, r)| (e, r)));
}

pub fn update_particle_time(
    time: Res<Time>,
    assets: Res<Assets<ParticlesAsset>>,
    system_query: Query<(&Particles3d, &ParticleSystemRuntime)>,
    mut emitter_query: Query<(&EmitterEntity, &mut EmitterRuntime)>,
) {
    let _timer = crate::stats::UPDATE_NS.time();
    step_particle_time(&time, &assets, &system_query, emitter_query.iter_mut());
}

fn step_particle_time<'a>(
    time: &Time,
    assets: &Assets<ParticlesAsset>,
    system_query: &Query<(&Particles3d, &ParticleSystemRuntime)>,
    emitters: impl Iterator<Item = (&'a EmitterEntity, Mut<'a, EmitterRuntime>)>,
) {
    for (emitter, mut runtime) in emitters {
        let Ok((particle_system, system_runtime)) = system_query.get(emitter.parent_system) else {
            continue;
        };

        let Some(asset) = assets.get(particle_system) else {
            continue;
        };

        let Some(emitter_data) = asset.emitters.get(runtime.emitter_index) else {
            continue;
        };
        // Kestrel: the trigger may set the cycle length.
        let emitter_time = runtime.time(&emitter_data.time);

        runtime.simulation_steps.clear();

        let clear_requested = runtime.clear_requested;
        runtime.clear_requested = false;

        if runtime.inactive || system_runtime.paused {
            if clear_requested {
                let step = SimulationStep {
                    prev_system_time: runtime.system_time,
                    system_time: runtime.system_time,
                    cycle: runtime.cycle,
                    delta_time: 0.0,
                    clear_requested: true,
                    trail_history_write_index: runtime.trail_history_write_index,
                };
                runtime.simulation_steps.push(step);
            }
            continue;
        }

        let fixed_fps = emitter_time.fixed_fps;
        let total_duration = emitter_time.total_duration();

        if fixed_fps > 0 {
            let fixed_delta = 1.0 / fixed_fps as f32;
            let frame_delta = time.delta_secs().min(MAX_FRAME_DELTA);
            runtime.accumulated_delta += frame_delta;

            while runtime.accumulated_delta >= fixed_delta
                || (clear_requested && runtime.simulation_steps.is_empty())
            {
                runtime.accumulated_delta -= fixed_delta;

                let prev_time = runtime.system_time;
                runtime.system_time += fixed_delta;

                if runtime.system_time >= total_duration && total_duration > 0.0 {
                    runtime.system_time = runtime.system_time % total_duration;
                    runtime.cycle += 1;
                }

                let step = SimulationStep {
                    prev_system_time: prev_time,
                    system_time: runtime.system_time,
                    cycle: runtime.cycle,
                    delta_time: fixed_delta,
                    clear_requested: if runtime.simulation_steps.is_empty() {
                        clear_requested
                    } else {
                        false
                    },
                    trail_history_write_index: runtime.trail_history_write_index,
                };
                runtime.advance_trail_history();
                runtime.simulation_steps.push(step);
            }

            if !runtime.simulation_steps.is_empty() {
                runtime.prev_system_time = runtime.simulation_steps[0].prev_system_time;
            }
        } else {
            let delta = time.delta_secs();
            let prev_time = runtime.system_time;
            runtime.prev_system_time = runtime.system_time;
            runtime.system_time += delta;

            if runtime.system_time >= total_duration && total_duration > 0.0 {
                runtime.system_time = runtime.system_time % total_duration;
                runtime.cycle += 1;
            }

            let step = SimulationStep {
                prev_system_time: prev_time,
                system_time: runtime.system_time,
                cycle: runtime.cycle,
                delta_time: delta,
                clear_requested,
                trail_history_write_index: runtime.trail_history_write_index,
            };
            runtime.advance_trail_history();
            runtime.simulation_steps.push(step);
        }

        if emitter_time.one_shot && runtime.cycle > 0 && !runtime.one_shot_completed {
            // Kestrel: stop emitting when the cycle ends, but finish (and
            // despawn) a frame later. Particles spawn at age 0 at the end of
            // their first step, so they still have this frame to live;
            // upstream cut every one-shot burst's last frame.
            if runtime.emitting {
                runtime.set_emitting(false);
            } else {
                runtime.one_shot_completed = true;
            }
        }

        if !runtime.emitting {
            runtime.inactive_time += time.delta_secs();
            let grace = emitter_time.lifetime * INACTIVE_GRACE_FACTOR;
            if runtime.inactive_time > grace {
                runtime.inactive = true;
            }
        } else {
            runtime.inactive_time = 0.0;
        }
    }
}

fn transform_align_to_u32(align: Option<crate::asset::TransformAlign>) -> u32 {
    use crate::asset::TransformAlign;
    match align {
        None => 0,
        Some(TransformAlign::Billboard) => 1,
        Some(TransformAlign::YToVelocity) => 2,
        Some(TransformAlign::BillboardYToVelocity) => 3,
        Some(TransformAlign::BillboardFixedY) => 4,
    }
}

fn create_particle_material_from_config(
    config: &DrawPassMaterial,
    sorted_particles_buffer: Handle<ShaderBuffer>,
    emitter_uniforms_buffer: Handle<ShaderBuffer>,
    asset_server: &AssetServer,
    assets_folders: &[String],
) -> ParticleMaterial {
    let base = match config {
        DrawPassMaterial::Standard(mat) => mat.to_standard_material(asset_server, assets_folders),
        DrawPassMaterial::CustomShader { .. } => {
            todo!("custom shader support not yet implemented")
        }
    };

    ExtendedMaterial {
        base,
        extension: ParticleMaterialExtension {
            sorted_particles: sorted_particles_buffer,
            emitter_uniforms: emitter_uniforms_buffer,
        },
    }
}

fn bake_thickness_curve(trail: &EmitterTrail) -> [f32; TRAIL_THICKNESS_CURVE_SAMPLES] {
    let mut samples = [1.0f32; TRAIL_THICKNESS_CURVE_SAMPLES];
    if let Some(ref curve) = trail.thickness_curve {
        for (i, sample) in samples.iter_mut().enumerate() {
            let t = i as f32 / (TRAIL_THICKNESS_CURVE_SAMPLES - 1) as f32;
            *sample = curve.sample(t);
        }
    }
    samples
}

/// Kestrel: what a trigger sets for one emitter at setup.
struct TriggeredEmitter {
    seed: Option<u32>,
    spawn_count: Option<u32>,
    lifetime: Option<f32>,
}

fn triggered_emitter(
    emitter: &EmitterData,
    index: usize,
    trigger: Option<&ParticleTrigger>,
    world: &GlobalTransform,
) -> TriggeredEmitter {
    let seed = trigger.and_then(|t| t.emitter_seed(index)).or(emitter.time.fixed_seed);
    let distance = trigger.and_then(|t| t.target).map(|target| target.distance(world.translation()));
    let spawn_count = emitter.emission.particles_per_meter.zip(distance).map(|(rate, distance)| {
        ((distance * rate).ceil() as u32).clamp(1, emitter.emission.particles_amount.max(1))
    });
    // Speeds scale with the system, as in `build_base_uniforms`.
    let (scale, _, _) = world.to_scale_rotation_translation();
    let slowest = emitter.velocities.initial_velocity.min * scale.element_product().abs().cbrt();
    let lifetime = distance
        .filter(|_| emitter.time.lifetime_to_target && slowest > 0.0)
        .map(|distance| (distance / slowest).clamp(1e-3, emitter.time.lifetime));
    TriggeredEmitter { seed, spawn_count, lifetime }
}

/// Kestrel: a pool of `capacity` bursts of `emitter` (`pool.rs`): buffers for
/// every slot, one material and one entity that draws them all.
#[allow(clippy::too_many_arguments)]
fn create_pool(
    commands: &mut Commands,
    emitter: &EmitterData,
    capacity: u32,
    meshes: &mut Assets<Mesh>,
    mesh_cache: &mut ParticleMeshCache,
    buffers: &mut Assets<ShaderBuffer>,
    materials: &mut Assets<ParticleMaterial>,
    asset_server: &AssetServer,
) -> crate::pool::Pool {
    let total = emitter.emission.particles_amount * capacity;
    let particles = vec![ParticleData::default(); total as usize];
    let mut particle_buffer = ShaderBuffer::from(particles.clone());
    particle_buffer.buffer_usage |= bevy::render::render_resource::BufferUsages::COPY_SRC;
    let particle_buffer = buffers.add(particle_buffer);
    let indices_buffer = buffers.add(ShaderBuffer::from((0..total).collect::<Vec<u32>>()));
    let sorted_particles_buffer = buffers.add(ShaderBuffer::from(particles));
    let uniforms = ParticleEmitterUniforms {
        emitter_transform: Mat4::IDENTITY,
        max_particles: total,
        particle_flags: emitter.particle_flags.bits(),
        trail_size: 1,
        transform_align: transform_align_to_u32(emitter.draw_pass.transform_align),
        pooled: 1,
        ..default()
    };
    let emitter_uniforms_buffer = buffers.add(ShaderBuffer::from(vec![uniforms]));
    let mesh = mesh_cache.get_or_create(&emitter.draw_pass.mesh, total, meshes);
    let material = materials.add(create_particle_material_from_config(
        &emitter.draw_pass.material,
        sorted_particles_buffer.clone(),
        emitter_uniforms_buffer.clone(),
        asset_server,
        &[],
    ));
    let mut entity = commands.spawn((
        Name::new("particle pool"),
        crate::pool::ParticlePoolDraw,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        GlobalTransform::IDENTITY,
        Visibility::Inherited,
        // Bursts anywhere in the world share this draw.
        bevy::camera::visibility::NoFrustumCulling,
    ));
    if !emitter.draw_pass.shadow_caster {
        entity.insert(NotShadowCaster);
    }
    let draw_order = match emitter.draw_pass.draw_order {
        crate::asset::DrawOrder::Index => 0,
        crate::asset::DrawOrder::Lifetime => 1,
        crate::asset::DrawOrder::ReverseLifetime => 2,
        crate::asset::DrawOrder::ViewDepth => 3,
    };
    let handle = ParticleBufferHandle {
        particle_buffer,
        indices_buffer,
        sorted_particles_buffer,
        emitter_uniforms_buffer,
        max_particles: total,
        amount: total,
        trail_size: 1,
        trail_history_buffer: None,
        trail_history_frames: 0,
    };
    crate::pool::Pool::new(entity.id(), capacity, emitter, draw_order, handle, uniforms)
}

/// Sets up newly spawned systems. Kestrel: runs twice, before transform
/// propagation and again after it (`LATE`) for systems spawned in between,
/// such as at a posed weapon's muzzle; see `lib.rs`.
pub fn setup_particle_systems<const LATE: bool>(
    mut commands: Commands,
    query: Query<
        (Entity, &Particles3d, Has<EditorMode>, Option<&Transform>, Option<&ParticleTrigger>),
        Without<ParticleSystemRuntime>,
    >,
    assets: Res<Assets<ParticlesAsset>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mesh_cache: ResMut<ParticleMeshCache>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
    mut pools: ResMut<crate::pool::ParticlePools>,
) {
    let _timer = crate::stats::SETUP_NS.time();
    for (system_entity, particle_system, is_editor, transform, trigger) in query.iter() {
        let Some(asset) = assets.get(particle_system) else {
            continue;
        };

        if asset.emitters.is_empty() {
            continue;
        }

        let assets_folders = if is_editor {
            asset.sprinkles_editor.assets_folder.as_slice()
        } else {
            &[]
        };

        let initial_transform = asset.initial_transform.to_transform();
        // Kestrel: where the system is before transforms propagate. Assumes a
        // root entity (no parent), as Kestrel spawns them.
        let system_world = GlobalTransform::from(transform.copied().unwrap_or(initial_transform));

        commands
            .entity(system_entity)
            .queue_silenced(move |mut entity: EntityWorldMut| {
                entity
                    .insert(ParticleSystemRuntime::default())
                    .insert_if_new((initial_transform, Visibility::default()));
            });

        let mut emitter_entities: Vec<Entity> = Vec::new();

        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            // Kestrel: a burst of a poolable emitter takes a slot in its pool
            // (`pool.rs`) instead of buffers, a material and a draw of its own.
            if crate::pool::eligible(asset, emitter_index, is_editor) {
                let key = crate::pool::PoolKey { asset: particle_system.id(), emitter: emitter_index };
                let slot = pools.claim(key, |capacity| {
                    create_pool(&mut commands, emitter, capacity, &mut meshes, &mut mesh_cache, &mut buffers, &mut materials, &asset_server)
                });
                let pool = pools.pools[&key].iter().find(|p| p.entity == slot.pool).unwrap();
                let mut handle = pool.buffers.clone();
                handle.max_particles = pool.amount;
                handle.amount = pool.amount;
                // An empty pool is hidden; show it before visibility is computed.
                commands.entity(slot.pool).insert(Visibility::Inherited);
                let local = emitter.initial_transform.to_transform();
                let emitter_world = system_world.mul_transform(local);
                let triggered = triggered_emitter(emitter, emitter_index, trigger, &emitter_world);
                let mut runtime = EmitterRuntime::new(emitter_index, triggered.seed);
                runtime.spawn_count = triggered.spawn_count;
                runtime.lifetime = triggered.lifetime;
                let mut emitter_cmds = commands.spawn((
                    EmitterEntity { parent_system: system_entity },
                    runtime,
                    handle,
                    slot,
                    local,
                    emitter_world,
                ));
                if LATE {
                    emitter_cmds.insert(SetUpLate);
                }
                let emitter_entity = emitter_cmds.id();
                emitter_entities.push(emitter_entity);
                commands
                    .entity(system_entity)
                    .queue_silenced(move |mut entity: EntityWorldMut| {
                        entity.add_child(emitter_entity);
                    });
                continue;
            }

            let amount = emitter.emission.particles_amount;
            let trail_size = emitter.trail_size();
            let total_slots = amount * trail_size;

            let particles: Vec<ParticleData> =
                (0..total_slots).map(|_| ParticleData::default()).collect();

            let mut particle_buffer = ShaderBuffer::from(particles.clone());
            particle_buffer.buffer_usage |= bevy::render::render_resource::BufferUsages::COPY_SRC;
            let particle_buffer_handle = buffers.add(particle_buffer);

            let indices: Vec<u32> = (0..total_slots).collect();
            let indices_buffer_handle = buffers.add(ShaderBuffer::from(indices));

            let sorted_particles_buffer_handle = buffers.add(ShaderBuffer::from(particles));

            let trail_history_frames = compute_trail_history_frames(emitter);
            let trail_history_buffer =
                create_trail_history_buffer(amount, trail_history_frames, &mut buffers);

            let emitter_uniforms = ParticleEmitterUniforms {
                emitter_transform: Mat4::IDENTITY,
                max_particles: total_slots,
                particle_flags: emitter.particle_flags.bits(),
                trail_size,
                transform_align: transform_align_to_u32(emitter.draw_pass.transform_align),
                ..default()
            };
            let emitter_uniforms_buffer_handle =
                buffers.add(ShaderBuffer::from(vec![emitter_uniforms]));

            let current_mesh = emitter.draw_pass.mesh.clone();
            let current_material = emitter.draw_pass.material.clone();
            let shadow_caster = emitter.draw_pass.shadow_caster;

            let particle_mesh_handle = mesh_cache.get_or_create(&current_mesh, amount, &mut meshes);

            let material_handle = materials.add(create_particle_material_from_config(
                &current_material,
                sorted_particles_buffer_handle.clone(),
                emitter_uniforms_buffer_handle.clone(),
                &asset_server,
                assets_folders,
            ));

            let local = emitter.initial_transform.to_transform();
            let emitter_world = system_world.mul_transform(local);
            let triggered = triggered_emitter(emitter, emitter_index, trigger, &emitter_world);
            let mut runtime = EmitterRuntime::new(emitter_index, triggered.seed);
            runtime.trail_history_frames = trail_history_frames;
            runtime.spawn_count = triggered.spawn_count;
            runtime.lifetime = triggered.lifetime;

            let mut emitter_cmds = commands.spawn((
                EmitterEntity {
                    parent_system: system_entity,
                },
                runtime,
                ParticleBufferHandle {
                    particle_buffer: particle_buffer_handle.clone(),
                    indices_buffer: indices_buffer_handle.clone(),
                    sorted_particles_buffer: sorted_particles_buffer_handle.clone(),
                    emitter_uniforms_buffer: emitter_uniforms_buffer_handle,
                    max_particles: total_slots,
                    amount,
                    trail_size,
                    trail_history_buffer,
                    trail_history_frames,
                },
                Mesh3d(particle_mesh_handle.clone()),
                MeshMaterial3d(material_handle.clone()),
                CurrentMeshConfig(current_mesh),
                CurrentMaterialConfig(current_material),
                ParticleMeshHandle(particle_mesh_handle),
                ParticleMaterialHandle(material_handle),
                local,
                // Kestrel: already right for a late setup, which draws before
                // the next propagation.
                emitter_world,
                Visibility::default(),
            ));

            if !shadow_caster {
                emitter_cmds.insert(NotShadowCaster);
            }
            if LATE {
                emitter_cmds.insert(SetUpLate);
            }
            // Kestrel: cull by the authored box, around the emitter. Upstream
            // never applied it, so Bevy culled by one particle mesh at the
            // emitter and a far-flying particle vanished with its emitter offscreen.
            let aabb = emitter.draw_pass.visibility_aabb;
            emitter_cmds.insert((
                bevy::camera::primitives::Aabb { center: aabb.center, half_extents: aabb.half_extents },
                bevy::camera::visibility::NoAutoAabb,
            ));

            let emitter_entity = emitter_cmds.id();

            emitter_entities.push(emitter_entity);
            commands
                .entity(system_entity)
                .queue_silenced(move |mut entity: EntityWorldMut| {
                    entity.add_child(emitter_entity);
                });
        }

        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            if let Some(ref sub_config) = emitter.sub_emitter {
                let target_index = sub_config.target_emitter;
                if target_index == emitter_index || target_index >= asset.emitters.len() {
                    continue;
                }

                let target_amount = asset.emitters[target_index].emission.particles_amount;
                let buffer_len = 4 + 12 * target_amount as usize;
                let mut initial_data = vec![0u32; buffer_len];
                initial_data[1] = target_amount;
                let mut buffer = ShaderBuffer::from(initial_data);
                buffer.buffer_usage |= bevy::render::render_resource::BufferUsages::COPY_DST;

                let buffer_handle = buffers.add(buffer);
                let target_entity = emitter_entities[target_index];
                let parent_entity = emitter_entities[emitter_index];

                commands
                    .entity(parent_entity)
                    .insert(SubEmitterBufferHandle {
                        buffer: buffer_handle,
                        target_emitter: target_entity,
                        max_particles: target_amount,
                    });
            }
        }

        for (collider_index, collider_data) in asset.colliders.iter().enumerate() {
            let collider_entity = commands
                .spawn((
                    ColliderEntity {
                        parent_system: system_entity,
                        collider_index,
                    },
                    ParticlesCollider3D {
                        enabled: collider_data.enabled,
                        shape: collider_data.shape.clone(),
                    },
                    collider_data.initial_transform.to_transform(),
                    Name::new(collider_data.name.clone()),
                ))
                .id();

            commands
                .entity(system_entity)
                .queue_silenced(move |mut entity: EntityWorldMut| {
                    entity.add_child(collider_entity);
                });
        }
    }
}

pub fn cleanup_particle_entities(
    mut commands: Commands,
    mut removed_systems: RemovedComponents<Particles3d>,
    emitter_entities: Query<Entity, With<EmitterEntity>>,
    emitter_parent_query: Query<&EmitterEntity>,
    collider_entities: Query<(Entity, &ColliderEntity)>,
) {
    for removed_system in removed_systems.read() {
        for emitter_entity in emitter_entities.iter() {
            if let Ok(emitter) = emitter_parent_query.get(emitter_entity) {
                if emitter.parent_system == removed_system {
                    commands.entity(emitter_entity).despawn();
                }
            }
        }

        for (entity, collider) in collider_entities.iter() {
            if collider.parent_system == removed_system {
                commands.entity(entity).despawn();
            }
        }
    }
}

pub fn sync_collider_data(
    particle_systems: Query<&Particles3d>,
    assets: Res<Assets<ParticlesAsset>>,
    mut collider_query: Query<(&ColliderEntity, &mut ParticlesCollider3D, &mut Transform)>,
) {
    if !assets.is_changed() {
        return;
    }

    for (collider, mut collider3d, mut transform) in collider_query.iter_mut() {
        let Some(collider_data) =
            get_particle_asset(collider.parent_system, &particle_systems, &assets)
                .and_then(|asset| asset.colliders.get(collider.collider_index))
        else {
            continue;
        };

        collider3d.enabled = collider_data.enabled;
        collider3d.shape = collider_data.shape.clone();
        *transform = collider_data.initial_transform.to_transform();
    }
}

pub fn sync_particle_mesh(
    particle_systems: Query<&Particles3d>,
    mut emitter_query: Query<(
        &EmitterEntity,
        &EmitterRuntime,
        &ParticleBufferHandle,
        &mut CurrentMeshConfig,
        &mut ParticleMeshHandle,
        &mut Mesh3d,
    )>,
    assets: Res<Assets<ParticlesAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mesh_cache: ResMut<ParticleMeshCache>,
) {
    for (emitter, runtime, buffer_handle, mut current_config, mut mesh_handle, mut mesh3d) in
        emitter_query.iter_mut()
    {
        let Some(emitter_data) = get_emitter_data(
            emitter.parent_system,
            runtime.emitter_index,
            &particle_systems,
            &assets,
        ) else {
            continue;
        };

        let new_mesh = emitter_data.draw_pass.mesh.clone();

        if current_config.0 != new_mesh {
            let new_mesh_handle =
                mesh_cache.get_or_create(&new_mesh, buffer_handle.amount, &mut meshes);
            mesh3d.0 = new_mesh_handle.clone();
            current_config.0 = new_mesh;
            mesh_handle.0 = new_mesh_handle;
        }
    }
}

pub(crate) fn sync_particle_buffers(
    particle_systems: Query<&Particles3d>,
    editor_modes: Query<Has<EditorMode>>,
    mut emitter_query: Query<(
        &EmitterEntity,
        &mut EmitterRuntime,
        &mut ParticleBufferHandle,
        &mut ParticleMeshHandle,
        &mut Mesh3d,
        &mut CurrentMeshConfig,
        &mut ParticleMaterialHandle,
        &mut MeshMaterial3d<ParticleMaterial>,
        &mut CurrentMaterialConfig,
    )>,
    assets: Res<Assets<ParticlesAsset>>,
    asset_server: Res<AssetServer>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mesh_cache: ResMut<ParticleMeshCache>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
) {
    for (
        emitter,
        mut runtime,
        mut buffer_handle,
        mut mesh_handle,
        mut mesh3d,
        mut current_config,
        mut material_handle,
        mut material3d,
        mut current_material_config,
    ) in emitter_query.iter_mut()
    {
        let Some(emitter_data) = get_emitter_data(
            emitter.parent_system,
            runtime.emitter_index,
            &particle_systems,
            &assets,
        ) else {
            continue;
        };

        let new_amount = emitter_data.emission.particles_amount;
        let new_trail_size = emitter_data.trail_size();
        let new_trail_history_frames = compute_trail_history_frames(emitter_data);

        if buffer_handle.amount == new_amount
            && buffer_handle.trail_size == new_trail_size
            && buffer_handle.trail_history_frames == new_trail_history_frames
        {
            continue;
        }

        let new_total = new_amount * new_trail_size;
        let particles: Vec<ParticleData> =
            (0..new_total).map(|_| ParticleData::default()).collect();

        let mut new_particle_buffer = ShaderBuffer::from(particles.clone());
        new_particle_buffer.buffer_usage |= bevy::render::render_resource::BufferUsages::COPY_SRC;
        let new_particle_buf = buffers.add(new_particle_buffer);
        let new_indices_buf = buffers.add(ShaderBuffer::from((0..new_total).collect::<Vec<u32>>()));
        let new_sorted_buf = buffers.add(ShaderBuffer::from(particles));

        let emitter_uniforms = ParticleEmitterUniforms {
            max_particles: new_total,
            particle_flags: emitter_data.particle_flags.bits(),
            trail_size: new_trail_size,
            transform_align: transform_align_to_u32(emitter_data.draw_pass.transform_align),
            trail_thickness_curve: bake_thickness_curve(&emitter_data.trail),
            ..default()
        };
        let new_uniforms_buf = buffers.add(ShaderBuffer::from(vec![emitter_uniforms]));

        buffer_handle.particle_buffer = new_particle_buf;
        buffer_handle.indices_buffer = new_indices_buf;
        buffer_handle.sorted_particles_buffer = new_sorted_buf.clone();
        buffer_handle.emitter_uniforms_buffer = new_uniforms_buf.clone();
        buffer_handle.max_particles = new_total;
        buffer_handle.amount = new_amount;
        buffer_handle.trail_size = new_trail_size;

        buffer_handle.trail_history_buffer =
            create_trail_history_buffer(new_amount, new_trail_history_frames, &mut buffers);
        buffer_handle.trail_history_frames = new_trail_history_frames;
        runtime.trail_history_write_index = 0;
        runtime.trail_history_frames = new_trail_history_frames;

        let is_editor = editor_modes.get(emitter.parent_system).unwrap_or(false);
        let assets_folders =
            get_editor_assets_folders(emitter.parent_system, is_editor, &particle_systems, &assets);

        let new_material = materials.add(create_particle_material_from_config(
            &emitter_data.draw_pass.material,
            new_sorted_buf,
            new_uniforms_buf,
            &asset_server,
            assets_folders,
        ));
        material3d.0 = new_material.clone();
        material_handle.0 = new_material;
        current_material_config.0 = emitter_data.draw_pass.material.clone();

        let new_mesh_handle =
            mesh_cache.get_or_create(&emitter_data.draw_pass.mesh, new_amount, &mut meshes);
        mesh3d.0 = new_mesh_handle.clone();
        mesh_handle.0 = new_mesh_handle.clone();
        current_config.0 = emitter_data.draw_pass.mesh.clone();
    }
}

pub fn write_emitter_uniforms(
    particle_systems: Query<&Particles3d>,
    // Kestrel: pooled bursts share their pool's uniforms (`pool.rs`).
    emitter_query: Query<
        (&EmitterEntity, &EmitterRuntime, &ParticleBufferHandle, &GlobalTransform),
        Without<crate::pool::PoolSlot>,
    >,
    assets: Res<Assets<ParticlesAsset>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let _timer = crate::stats::UPDATE_NS.time();
    for (emitter, runtime, buffer_handle, global_transform) in emitter_query.iter() {
        let Some(emitter_data) = get_emitter_data(
            emitter.parent_system,
            runtime.emitter_index,
            &particle_systems,
            &assets,
        ) else {
            continue;
        };

        let trail_size = emitter_data.trail_size();
        let trail_thickness_curve = bake_thickness_curve(&emitter_data.trail);

        let uniforms = ParticleEmitterUniforms {
            emitter_transform: global_transform.to_matrix(),
            max_particles: buffer_handle.max_particles,
            particle_flags: emitter_data.particle_flags.bits(),
            use_local_coords: emitter_data.draw_pass.use_local_coords as u32,
            trail_size,
            transform_align: transform_align_to_u32(emitter_data.draw_pass.transform_align),
            trail_thickness_curve,
            // How far the simulation moved this frame: zero when paused.
            delta_time: runtime.simulation_steps.iter().map(|step| step.delta_time).sum(),
            ..default()
        };

        if let Some(mut buffer) = buffers.get_mut(&buffer_handle.emitter_uniforms_buffer)
            && let Some(slot) = buffer
                .cast_slice_mut::<ParticleEmitterUniforms>()
                .and_then(|slice| slice.first_mut())
        {
            *slot = uniforms;
        }
    }
}

pub fn sync_particle_material(
    particle_systems: Query<&Particles3d>,
    editor_modes: Query<Has<EditorMode>>,
    mut emitter_query: Query<(
        &EmitterEntity,
        &EmitterRuntime,
        &mut CurrentMaterialConfig,
        &mut ParticleMaterialHandle,
        &mut MeshMaterial3d<ParticleMaterial>,
    )>,
    assets: Res<Assets<ParticlesAsset>>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
) {
    for (emitter, runtime, mut current_config, mut material_handle, mut material3d) in
        emitter_query.iter_mut()
    {
        let Some(emitter_data) = get_emitter_data(
            emitter.parent_system,
            runtime.emitter_index,
            &particle_systems,
            &assets,
        ) else {
            continue;
        };

        let new_material = emitter_data.draw_pass.material.clone();

        if current_config.0.cache_key() != new_material.cache_key() {
            let (sorted_particles_handle, emitter_uniforms_handle) = {
                let Some(existing_material) = materials.get(&material_handle.0) else {
                    continue;
                };
                (
                    existing_material.extension.sorted_particles.clone(),
                    existing_material.extension.emitter_uniforms.clone(),
                )
            };

            let is_editor = editor_modes.get(emitter.parent_system).unwrap_or(false);
            let assets_folders = get_editor_assets_folders(
                emitter.parent_system,
                is_editor,
                &particle_systems,
                &assets,
            );

            let new_material_handle = materials.add(create_particle_material_from_config(
                &new_material,
                sorted_particles_handle,
                emitter_uniforms_handle,
                &asset_server,
                assets_folders,
            ));

            material3d.0 = new_material_handle.clone();
            current_config.0 = new_material;
            material_handle.0 = new_material_handle;
        }
    }
}

#[cfg(test)]
mod trigger_tests {
    use super::*;

    fn emitter() -> EmitterData {
        let mut emitter = EmitterData::default();
        emitter.emission.particles_amount = 100;
        emitter.velocities.initial_velocity = crate::asset::Range::new(80.0, 80.0);
        emitter.time.lifetime = 1.0;
        emitter
    }

    #[test]
    fn a_target_sets_the_count_and_the_trip_time() {
        let mut emitter = emitter();
        emitter.emission.particles_per_meter = Some(9.0);
        emitter.time.lifetime_to_target = true;
        let trigger = ParticleTrigger { target: Some(Vec3::new(4.0, 0.0, 0.0)), seed: Some(7) };
        let at = GlobalTransform::IDENTITY;
        let triggered = triggered_emitter(&emitter, 0, Some(&trigger), &at);
        assert_eq!(triggered.spawn_count, Some(36));
        assert!((triggered.lifetime.unwrap() - 4.0 / 80.0).abs() < 1e-6);
        // Far away: capped by the amount and the lifetime.
        let far = ParticleTrigger { target: Some(Vec3::new(400.0, 0.0, 0.0)), ..trigger };
        let triggered = triggered_emitter(&emitter, 0, Some(&far), &at);
        assert_eq!((triggered.spawn_count, triggered.lifetime), (Some(100), Some(1.0)));
        // No target: the asset's values.
        let triggered = triggered_emitter(&emitter, 0, None, &at);
        assert_eq!((triggered.spawn_count, triggered.lifetime), (None, None));
    }

    #[test]
    fn a_trigger_seed_is_stable_and_differs_per_emitter() {
        let mut emitter = emitter();
        emitter.time.fixed_seed = Some(3);
        let trigger = ParticleTrigger { target: None, seed: Some(42) };
        let at = GlobalTransform::IDENTITY;
        let seed = |index| triggered_emitter(&emitter, index, Some(&trigger), &at).seed;
        assert_eq!(seed(0), seed(0));
        assert_ne!(seed(0), seed(1));
        assert_ne!(seed(0), Some(3));
        // Without a trigger seed, the asset's fixed seed.
        assert_eq!(triggered_emitter(&emitter, 0, None, &at).seed, Some(3));
    }
}

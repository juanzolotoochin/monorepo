use bevy::{
    core_pipeline::schedule::camera_driver,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
            BindingResource, BufferBinding,
            BufferUsages, CachedComputePipelineId, CachedPipelineState, ComputePassDescriptor,
            ComputePipelineDescriptor, PipelineCache, SamplerBindingType, SamplerDescriptor,
            ShaderStages, TextureSampleType,
            binding_types::{
                sampler, storage_buffer, storage_buffer_read_only, storage_buffer_sized,
                texture_2d, uniform_buffer_sized,
            },
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
        storage::GpuShaderBuffer,
        texture::GpuImage,
    },
};
use std::borrow::Cow;

use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderCacheError;

use crate::extract::{
    ColliderUniform, EmitterUniforms, ExtractedColliders, ExtractedEmitterData,
    ExtractedParticleSystem, MAX_COLLIDERS,
};
use crate::runtime::ParticleData;
use crate::textures::{FallbackCurveTexture, FallbackGradientTexture};

#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable, ShaderType)]
#[repr(C)]
pub struct ColliderArray {
    pub colliders: [ColliderUniform; MAX_COLLIDERS],
}

const SHADER_ASSET_PATH: &str = "embedded://bevy_sprinkles/shaders/particle_simulate.wesl";
const WORKGROUP_SIZE: u32 = 64;
/// Kestrel: the most emitters one dispatch simulates (`MAX_BATCH` in
/// `particle_simulate.wesl`): its uniforms are an array of this many
/// `EmitterUniforms`, which must fit the 64 KiB uniform binding limit.
const MAX_BATCH: u32 = 64;

/// Kestrel: bytes between two emitters' uniforms in a batch (the WGSL array
/// stride, a multiple of 16).
fn batch_stride() -> u64 {
    (std::mem::size_of::<EmitterUniforms>() as u64).next_multiple_of(16)
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub struct ParticleComputeLabel;

#[derive(Resource)]
pub struct ParticleComputePipeline {
    pub bind_group_layout: BindGroupLayoutDescriptor,
    pub simulate_pipeline: CachedComputePipelineId,
}

pub fn init_particle_compute_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
) {
    let bind_group_layout = BindGroupLayoutDescriptor::new(
        "ParticleComputeBindGroup",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // Kestrel: one buffer holds every emitter's uniforms. Each
                // dispatch binds a window of `MAX_BATCH` of them, selected
                // with a dynamic offset.
                uniform_buffer_sized(true, std::num::NonZeroU64::new(batch_stride() * MAX_BATCH as u64)),
                storage_buffer::<ParticleData>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                storage_buffer_read_only::<ColliderArray>(false),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
            ),
        ),
    );

    let shader = asset_server.load(SHADER_ASSET_PATH);
    let simulate_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("particle_simulate_pipeline".into()),
        layout: vec![bind_group_layout.clone()],
        shader,
        entry_point: Some(Cow::from("main")),
        ..default()
    });

    let linear_clamp_sampler = SamplerDescriptor {
        address_mode_u: bevy::render::render_resource::AddressMode::ClampToEdge,
        address_mode_v: bevy::render::render_resource::AddressMode::ClampToEdge,
        mag_filter: bevy::render::render_resource::FilterMode::Linear,
        min_filter: bevy::render::render_resource::FilterMode::Linear,
        ..default()
    };

    let gradient_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("gradient_sampler"),
        ..linear_clamp_sampler.clone()
    });

    let curve_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("curve_sampler"),
        ..linear_clamp_sampler
    });

    // dst and src need distinct buffers even when unused: WebGPU rejects two
    // writable storage bindings aliasing the same buffer range, even if neither
    // is read or written by the shader for this dispatch.
    let fallback_emission_dst_buffer = render_device.create_buffer_with_data(
        &bevy::render::render_resource::BufferInitDescriptor {
            label: Some("fallback_emission_dst_buffer"),
            contents: &[0u8; 64],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        },
    );

    let fallback_emission_src_buffer = render_device.create_buffer_with_data(
        &bevy::render::render_resource::BufferInitDescriptor {
            label: Some("fallback_emission_src_buffer"),
            contents: &[0u8; 64],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        },
    );

    let fallback_trail_history_buffer = render_device.create_buffer_with_data(
        &bevy::render::render_resource::BufferInitDescriptor {
            label: Some("fallback_trail_history_buffer"),
            contents: &[0u8; 64],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        },
    );

    commands.insert_resource(ParticleComputePipeline {
        bind_group_layout,
        simulate_pipeline,
    });
    commands.insert_resource(GradientSampler(gradient_sampler));
    commands.insert_resource(CurveSampler(curve_sampler));
    commands.insert_resource(FallbackEmissionBuffers {
        dst: fallback_emission_dst_buffer,
        src: fallback_emission_src_buffer,
    });
    commands.insert_resource(FallbackTrailHistoryBuffer(fallback_trail_history_buffer));
}

#[derive(Resource)]
pub struct GradientSampler(pub bevy::render::render_resource::Sampler);

#[derive(Resource)]
pub struct CurveSampler(pub bevy::render::render_resource::Sampler);

#[derive(Resource)]
pub struct FallbackEmissionBuffers {
    pub dst: Buffer,
    pub src: Buffer,
}

#[derive(Resource)]
pub(crate) struct FallbackTrailHistoryBuffer(pub(crate) Buffer);

#[derive(Resource, Default)]
pub struct EmissionBufferClearList {
    pub buffers: Vec<Buffer>,
    /// Kestrel: particle buffer byte ranges (freed pool slots) to zero before
    /// this frame's simulation.
    pub ranges: Vec<(Buffer, u64, u64)>,
}

/// Kestrel: one compute dispatch, simulating `count` emitters that share a
/// bind group (the bursts of one pool, or a single emitter).
pub struct ParticleDispatch {
    pub bind_group: BindGroup,
    /// Dynamic offset of the batch's first emitter uniforms.
    pub offset: u32,
    pub count: u32,
    /// Threads per emitter.
    pub threads: u32,
    pub is_sub_emitter_target: bool,
}

/// Per simulation step this frame, its dispatches in order (Kestrel;
/// upstream had a bind group and a dispatch per emitter per step).
#[derive(Resource, Default)]
pub struct ParticleComputeBindGroups {
    pub steps: Vec<Vec<ParticleDispatch>>,
}

/// Kestrel: the compute pass's GPU objects, kept from frame to frame.
/// Upstream created, every frame, the colliders buffer and a uniform buffer
/// and bind group per emitter per step. Now every emitter's uniforms go into
/// one shared buffer with a single write per frame (it only grows), and bind
/// groups are kept per set of bound resources, so the bursts of one pool
/// share one, rebuilt only when something it binds changes.
#[derive(Resource, Default)]
pub struct ParticleComputeCache {
    colliders: Option<Buffer>,
    uniforms: Option<Buffer>,
    scratch: Vec<u8>,
    /// By a hash of the ids of everything a bind group binds.
    bind_groups: std::collections::HashMap<u64, BindGroup>,
}

/// Kestrel: emitters that can share a dispatch in one step: same bind group,
/// same thread count, same pass.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BatchKey {
    bind_group: u64,
    threads: u32,
    is_sub_emitter_target: bool,
}

pub fn prepare_particle_compute_bind_groups(
    mut commands: Commands,
    pipeline: Res<ParticleComputePipeline>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut cache: ResMut<ParticleComputeCache>,
    extracted_systems: Res<ExtractedParticleSystem>,
    extracted_colliders: Option<Res<ExtractedColliders>>,
    gpu_storage_buffers: Res<RenderAssets<GpuShaderBuffer>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    fallback_gradient_texture: Option<Res<FallbackGradientTexture>>,
    fallback_curve_texture: Option<Res<FallbackCurveTexture>>,
    fallback_emission_buffers: Res<FallbackEmissionBuffers>,
    fallback_trail_history_buffer: Res<FallbackTrailHistoryBuffer>,
    gradient_sampler: Res<GradientSampler>,
    curve_sampler: Res<CurveSampler>,
) {
    let _timer = crate::stats::PREPARE_COMPUTE_NS.time();
    let reuse = crate::stats::reuse_gpu_resources();
    let cache = &mut *cache;

    let fallback_gradient_gpu_image = fallback_gradient_texture
        .as_ref()
        .and_then(|ft| gpu_images.get(&ft.handle));

    let fallback_curve_gpu_image = fallback_curve_texture
        .as_ref()
        .and_then(|ft| gpu_images.get(&ft.handle));

    let mut collider_array = ColliderArray::default();
    let collider_count = if let Some(ref colliders) = extracted_colliders {
        for (i, collider) in colliders.colliders.iter().enumerate() {
            if i >= MAX_COLLIDERS {
                break;
            }
            collider_array.colliders[i] = *collider;
        }
        colliders.colliders.len().min(MAX_COLLIDERS) as u32
    } else {
        0
    };

    // Kestrel: one colliders buffer (it has a fixed size), rewritten each frame.
    let colliders_buffer = match cache.colliders.as_ref().filter(|_| reuse) {
        Some(buffer) => {
            render_queue.write_buffer(buffer, 0, bytemuck::bytes_of(&collider_array));
            buffer.clone()
        }
        None => {
            crate::stats::BUFFERS_CREATED.add(1);
            let buffer = render_device.create_buffer_with_data(
                &bevy::render::render_resource::BufferInitDescriptor {
                    label: Some("colliders_buffer"),
                    contents: bytemuck::bytes_of(&collider_array),
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                },
            );
            cache.colliders = Some(buffer.clone());
            buffer
        }
    };

    // Kestrel: room for every step's uniforms, packed in batches that each
    // start at an aligned offset, plus the window the last batch binds.
    let stride = batch_stride();
    let window = stride * u64::from(MAX_BATCH);
    let alignment = u64::from(render_device.limits().min_uniform_buffer_offset_alignment);
    let total_steps: usize = extracted_systems.emitters.iter().map(|(_, e)| e.uniform_steps.len()).sum();
    let needed = (stride + alignment) * total_steps as u64 + window;
    if !reuse || cache.uniforms.as_ref().is_none_or(|b| b.size() < needed) {
        crate::stats::BUFFERS_CREATED.add(1);
        cache.uniforms = Some(render_device.create_buffer(&bevy::render::render_resource::BufferDescriptor {
            label: Some("emitter_uniform_buffer"),
            size: if reuse { needed.next_power_of_two() } else { needed },
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    let uniforms_buffer = cache.uniforms.clone().unwrap();
    let mut scratch = std::mem::take(&mut cache.scratch);
    scratch.clear();
    if !reuse {
        cache.bind_groups.clear();
    }

    let mut emission_clear_list = Vec::new();
    let mut live = std::collections::HashSet::new();
    // Per emitter that simulates this frame: its bind group's key and the
    // bind group.
    let mut simulated: Vec<(&ExtractedEmitterData, u64, BindGroup)> = Vec::new();

    for (_, emitter_data) in &extracted_systems.emitters {
        // Kestrel: a pool's entry only sorts (`pool.rs`).
        if emitter_data.uniform_steps.is_empty() {
            continue;
        }
        let Some(gpu_buffer) = gpu_storage_buffers.get(&emitter_data.particle_buffer_handle) else {
            continue;
        };

        fn resolve_texture<'a>(
            handle: &Option<Handle<Image>>,
            gpu_images: &'a RenderAssets<GpuImage>,
            fallback: Option<&'a GpuImage>,
        ) -> Option<&'a GpuImage> {
            handle.as_ref().and_then(|h| gpu_images.get(h)).or(fallback)
        }

        let Some(gradient_image) = resolve_texture(
            &emitter_data.gradient_texture_handle,
            &gpu_images,
            fallback_gradient_gpu_image,
        ) else {
            continue;
        };
        let Some(color_over_lifetime_image) = resolve_texture(
            &emitter_data.color_over_lifetime_texture_handle,
            &gpu_images,
            fallback_gradient_gpu_image,
        ) else {
            continue;
        };
        let Some(scale_over_lifetime_image) = resolve_texture(
            &emitter_data.scale_over_lifetime_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(alpha_over_lifetime_image) = resolve_texture(
            &emitter_data.alpha_over_lifetime_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(emission_over_lifetime_image) = resolve_texture(
            &emitter_data.emission_over_lifetime_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(turbulence_influence_over_lifetime_image) = resolve_texture(
            &emitter_data.turbulence_influence_over_lifetime_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(radial_velocity_curve_image) = resolve_texture(
            &emitter_data.radial_velocity_curve_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(angle_over_lifetime_image) = resolve_texture(
            &emitter_data.angle_over_lifetime_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(angular_velocity_curve_image) = resolve_texture(
            &emitter_data.angular_velocity_curve_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(orbit_velocity_curve_image) = resolve_texture(
            &emitter_data.orbit_velocity_curve_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };
        let Some(directional_velocity_curve_image) = resolve_texture(
            &emitter_data.directional_velocity_curve_texture_handle,
            &gpu_images,
            fallback_curve_gpu_image,
        ) else {
            continue;
        };

        let bind_group_layout = pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout);

        let dst_buffer = emitter_data
            .emission_buffer_handle
            .as_ref()
            .and_then(|h| gpu_storage_buffers.get(h))
            .map(|b| &b.buffer);

        let src_buffer = emitter_data
            .source_buffer_handle
            .as_ref()
            .and_then(|h| gpu_storage_buffers.get(h))
            .map(|b| &b.buffer);

        let dst_binding = dst_buffer.unwrap_or(&fallback_emission_buffers.dst);
        let src_binding = src_buffer.unwrap_or(&fallback_emission_buffers.src);

        let trail_history_buffer = emitter_data
            .trail_history_buffer_handle
            .as_ref()
            .and_then(|h| gpu_storage_buffers.get(h))
            .map(|b| &b.buffer);
        let trail_history_binding =
            trail_history_buffer.unwrap_or(&fallback_trail_history_buffer.0);

        if let Some(buf) = dst_buffer {
            emission_clear_list.push(buf.clone());
        }

        // Kestrel: reuse the bind group of these exact resources (including
        // the shared uniform buffer, which can grow), whichever emitter
        // created it.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            gpu_buffer.buffer.id().hash(&mut hasher);
            for image in [
                gradient_image,
                scale_over_lifetime_image,
                alpha_over_lifetime_image,
                emission_over_lifetime_image,
                turbulence_influence_over_lifetime_image,
                radial_velocity_curve_image,
                angle_over_lifetime_image,
                angular_velocity_curve_image,
                color_over_lifetime_image,
                orbit_velocity_curve_image,
                directional_velocity_curve_image,
            ] {
                image.texture_view.id().hash(&mut hasher);
            }
            for buffer in [&uniforms_buffer, &colliders_buffer, dst_binding, src_binding, trail_history_binding] {
                buffer.id().hash(&mut hasher);
            }
            hasher.finish()
        };
        live.insert(key);
        let bind_group = match cache.bind_groups.get(&key) {
            Some(group) => group.clone(),
            None => {
                crate::stats::BIND_GROUPS_CREATED.add(1);
                let group = render_device.create_bind_group(
                    Some("particle_compute_bind_group"),
                    &bind_group_layout,
                    &BindGroupEntries::sequential((
                        BindingResource::Buffer(BufferBinding {
                            buffer: &uniforms_buffer,
                            offset: 0,
                            size: std::num::NonZeroU64::new(window),
                        }),
                        gpu_buffer.buffer.as_entire_binding(),
                        &gradient_image.texture_view,
                        &gradient_sampler.0,
                        &scale_over_lifetime_image.texture_view,
                        &curve_sampler.0,
                        &alpha_over_lifetime_image.texture_view,
                        &curve_sampler.0,
                        &emission_over_lifetime_image.texture_view,
                        &curve_sampler.0,
                        &turbulence_influence_over_lifetime_image.texture_view,
                        &curve_sampler.0,
                        &radial_velocity_curve_image.texture_view,
                        &curve_sampler.0,
                        &angle_over_lifetime_image.texture_view,
                        &curve_sampler.0,
                        &angular_velocity_curve_image.texture_view,
                        &curve_sampler.0,
                        &color_over_lifetime_image.texture_view,
                        &gradient_sampler.0,
                        &orbit_velocity_curve_image.texture_view,
                        &curve_sampler.0,
                        &directional_velocity_curve_image.texture_view,
                        &curve_sampler.0,
                        colliders_buffer.as_entire_binding(),
                        dst_binding.as_entire_binding(),
                        src_binding.as_entire_binding(),
                        trail_history_binding.as_entire_binding(),
                    )),
                );
                cache.bind_groups.insert(key, group.clone());
                group
            }
        };
        simulated.push((emitter_data, key, bind_group));
    }
    cache.bind_groups.retain(|key, _| live.contains(key));

    // Kestrel: per step, emitters with the same bind group, thread count and
    // pass are simulated by one dispatch (up to `MAX_BATCH` at a time), in
    // the order their first emitter appears. Their uniforms are consecutive
    // in the shared buffer, from an aligned offset.
    let max_steps = simulated.iter().map(|(e, ..)| e.uniform_steps.len()).max().unwrap_or(0);
    let mut steps = Vec::with_capacity(max_steps);
    let mut batches: Vec<(BatchKey, BindGroup, Vec<EmitterUniforms>)> = Vec::new();
    let mut batch_index = std::collections::HashMap::new();
    for step_index in 0..max_steps {
        batches.clear();
        batch_index.clear();
        for (emitter_data, bind_group_key, bind_group) in &simulated {
            let Some(step_uniforms) = emitter_data.uniform_steps.get(step_index) else {
                continue;
            };
            // for trail passes, the uniform_steps alternate head/trail
            // dispatch count: head pass = amount threads, trail pass = amount * (trail_size - 1) threads
            let trail_size = emitter_data.trail_size;
            let is_trail_pass = trail_size > 1 && step_index % 2 == 1;
            let threads = if is_trail_pass { emitter_data.amount * (trail_size - 1) } else { emitter_data.amount };
            let key = BatchKey {
                bind_group: *bind_group_key,
                threads,
                is_sub_emitter_target: emitter_data.is_sub_emitter_target,
            };
            let index = *batch_index.entry(key).or_insert_with(|| {
                batches.push((key, bind_group.clone(), Vec::new()));
                batches.len() - 1
            });
            let mut uniforms = *step_uniforms;
            uniforms.collider_count = collider_count;
            batches[index].2.push(uniforms);
        }
        let mut dispatches = Vec::new();
        for (key, bind_group, uniforms) in &batches {
            for chunk in uniforms.chunks(MAX_BATCH as usize) {
                scratch.resize(scratch.len().next_multiple_of(alignment as usize), 0);
                dispatches.push(ParticleDispatch {
                    bind_group: bind_group.clone(),
                    offset: scratch.len() as u32,
                    count: chunk.len() as u32,
                    threads: key.threads,
                    is_sub_emitter_target: key.is_sub_emitter_target,
                });
                for uniforms in chunk {
                    scratch.extend_from_slice(bytemuck::bytes_of(uniforms));
                    scratch.resize(scratch.len().next_multiple_of(stride as usize), 0);
                }
            }
        }
        steps.push(dispatches);
    }
    if !scratch.is_empty() {
        render_queue.write_buffer(&uniforms_buffer, 0, &scratch);
    }
    cache.scratch = scratch;

    let mut unique_buffers: Vec<Buffer> = Vec::new();
    for buf in emission_clear_list {
        if !unique_buffers.iter().any(|b| b.id() == buf.id()) {
            unique_buffers.push(buf);
        }
    }

    commands.insert_resource(ParticleComputeBindGroups { steps });
    let ranges = extracted_systems
        .pool_clears
        .iter()
        .filter_map(|(handle, offset, size)| Some((gpu_storage_buffers.get(handle)?.buffer.clone(), *offset, *size)))
        .collect();
    commands.insert_resource(EmissionBufferClearList {
        buffers: unique_buffers,
        ranges,
    });
}

pub fn run_particle_compute_node(
    pipeline: Res<ParticleComputePipeline>,
    pipeline_cache: Res<PipelineCache>,
    bind_groups: Res<ParticleComputeBindGroups>,
    extracted: Res<ExtractedParticleSystem>,
    emission_clear_list: Res<EmissionBufferClearList>,
    mut ctx: RenderContext,
) {
    match pipeline_cache.get_compute_pipeline_state(pipeline.simulate_pipeline) {
        CachedPipelineState::Ok(_) => {}
        CachedPipelineState::Queued
        | CachedPipelineState::Creating(_)
        | CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_))
        | CachedPipelineState::Err(ShaderCacheError::ShaderImportNotYetAvailable) => return,
        CachedPipelineState::Err(err) => {
            panic!("Failed to compile particle compute shader: {err}")
        }
    }

    let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.simulate_pipeline)
    else {
        return;
    };

    // Kestrel: GPU and CPU time for every simulation dispatch this frame, as
    // `render/particle_compute/*` (needs `RenderDiagnosticsPlugin`).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "particle_compute");

    let has_sub_emitter_targets = extracted.emitters.iter().any(|(_, data)| data.is_sub_emitter_target);
    let pass_labels: &[&str] = if has_sub_emitter_targets {
        &["particle_compute_pass", "particle_sub_emitter_pass"]
    } else {
        &["particle_compute_pass"]
    };

    // Kestrel: zero the slots pools freed this frame, so nothing a finished
    // burst left behind is drawn.
    for (buffer, offset, size) in &emission_clear_list.ranges {
        ctx.command_encoder().clear_buffer(buffer, *offset, Some(*size));
    }

    for dispatches in &bind_groups.steps {
        for buf in &emission_clear_list.buffers {
            ctx.command_encoder().clear_buffer(buf, 0, Some(4));
        }

        for (pass_index, label) in pass_labels.iter().enumerate() {
            let is_target_pass = pass_index == 1;

            let mut pass = ctx
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some(label),
                    ..default()
                });

            pass.set_pipeline(compute_pipeline);

            for dispatch in dispatches.iter().filter(|d| d.is_sub_emitter_target == is_target_pass) {
                let workgroups = dispatch.threads.div_ceil(WORKGROUP_SIZE);
                pass.set_bind_group(0, &dispatch.bind_group, &[dispatch.offset]);
                pass.dispatch_workgroups(workgroups, dispatch.count, 1);
            }
        }
    }
    time_span.end(ctx.command_encoder());
}

pub struct ParticleComputePlugin;

impl Plugin for ParticleComputePlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<ParticleComputeBindGroups>()
            .init_resource::<ParticleComputeCache>()
            .init_resource::<EmissionBufferClearList>()
            .add_systems(RenderStartup, init_particle_compute_pipeline)
            .add_systems(
                Render,
                prepare_particle_compute_bind_groups.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                RenderGraph,
                run_particle_compute_node
                    .in_set(ParticleComputeLabel)
                    .in_set(RenderGraphSystems::Render)
                    .before(camera_driver),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_of_uniforms_fits_one_uniform_binding() {
        // WebGPU's default `max_uniform_buffer_binding_size`.
        assert!(batch_stride() * u64::from(MAX_BATCH) <= 65536, "stride {}", batch_stride());
        let shader = include_str!("shaders/particle_simulate.wesl");
        assert!(shader.contains(&format!("const MAX_BATCH: u32 = {MAX_BATCH}u;")));
    }
}

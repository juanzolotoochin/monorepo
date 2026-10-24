import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
};

struct GrassExtension {
    // x: seconds, yz: player position on the ground, w: grass half extent.
    wind: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> material: GrassExtension;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var bend_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var bend_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> bend_settings: vec4<f32>;

@vertex
fn vertex(input: Vertex) -> VertexOutput {
    let blade = decompress_vertex(input, input.instance_index);
    let root = blade.position.xz;
    let time = material.wind.x;
    let gust = sin(time * 0.53 + root.x * 0.15 - root.y * 0.21);
    let ripple = sin(time * 1.65 + blade.uv.y + root.x * 0.24 + root.y * 0.17);
    let away = root - material.wind.yz;
    let distance = length(away);
    let push = pow(max(1.0 - distance / bend_settings.x, 0.0), 2.0) * 0.30;
    let direction = away / max(distance, 0.001);
    let uv = (root + vec2<f32>(material.wind.w)) / (2.0 * material.wind.w);
    let trail_sample = textureSampleLevel(bend_map, bend_sampler, uv, 0.0);
    let trail = (trail_sample.rg * 2.0 - vec2<f32>(1.0)) * trail_sample.b;
    var position = blade.position;
    position.x += blade.uv.x * ((ripple * 0.13 + gust * 0.11) * position.y + direction.x * push + trail.x * 0.40);
    position.z += blade.uv.x * (gust * 0.08 * position.y + direction.y * push + trail.y * 0.40);
    position.y -= blade.uv.x * (push * 0.32 + trail_sample.b * 0.18);

    let world_from_local = mesh_functions::get_world_from_local(input.instance_index);
    var out: VertexOutput;
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    @if(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(blade.normal, input.instance_index);
    @if(VERTEX_UVS_A)
    out.uv = blade.uv;
    @if(VERTEX_COLORS)
    out.color = blade.color;
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = input.instance_index;
    @if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(input.instance_index, world_from_local[3]);
    return out;
}

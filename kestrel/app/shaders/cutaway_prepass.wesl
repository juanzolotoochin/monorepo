// The cutaway's prepass (see cutaway.wesl): the camera's depth, normal and
// motion prepass cuts the same hole, so the main pass sees through it.
// Shadow passes draw the uncut ground: they need no fragment shader for
// opaque ground (no targets), and a directional light's emulated unclipped
// depth (UNCLIPPED_DEPTH_ORTHO_EMULATION, shadows only) skips the cut.
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    render::pbr_prepass_functions,
};

struct Cutaway {
    // xyz: the middle of the player's body; w: the radius.
    center: vec4<f32>,
    // xyz: unit vector from the player toward the camera; w: the soft edge.
    toward: vec4<f32>,
    // x: the low plane's height (camera side), y: the head plane's (behind),
    // z: the radius under the head plane.
    planes: vec4<f32>,
    // rgb: the section colour (linear); w: 1 when the cut is on.
    section: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cutaway: Cutaway;

// How much of the ground at `p` is cut: 0 drawn, 1 gone, between in the edge.
fn cut(p: vec3<f32>) -> f32 {
    if cutaway.section.w < 0.5 {
        return 0.0;
    }
    let v = p - cutaway.center.xyz;
    let along = dot(v, cutaway.toward.xyz);
    // On the camera's side of the body the low plane, behind it the head's.
    let plane = select(cutaway.planes.y, cutaway.planes.x, along > 0.0);
    if p.y <= plane {
        return 0.0;
    }
    // Under the head only a narrower hole, enough to show the body.
    let radius = select(cutaway.planes.z, cutaway.center.w, p.y > cutaway.planes.y);
    let off = length(v - cutaway.toward.xyz * max(along, 0.0));
    return clamp((radius - off) / max(cutaway.toward.w, 0.001), 0.0, 1.0);
}

fn threshold(pixel: vec2<f32>) -> f32 {
    let x = u32(pixel.x) % 4u;
    let y = u32(pixel.y) % 4u;
    let bayer = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    return (f32(bayer[y * 4u + x]) + 0.5) / 16.0;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
@if(!UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    if cut(in.world_position.xyz) > threshold(in.position.xy) {
        discard;
    }
}
    var out: FragmentOutput;
@if(NORMAL_PREPASS) {
    let normal = normalize(select(-in.world_normal, in.world_normal, is_front));
    out.normal = vec4<f32>(normal * 0.5 + vec3<f32>(0.5), 1.0);
}
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(in.world_position, in.previous_world_position);
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
}

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
    // Line of sight. x: the inside factor; y: the height ground is hidden
    // over; z: 1 in the line of sight mode; w: how far out of sight greys.
    sight: vec4<f32>,
    // xy: the visibility texture's lowest corner (world x, z); z: its side;
    // w: how many tiles toward the camera walls are cut low.
    window: vec4<f32>,
    // xyz: the player's feet; w: the brightness kept out of sight.
    eye: vec4<f32>,
    // x: full light within this distance, y: down to z by this one; w: the
    // height over the feet walls on the camera's side are cut to.
    light: vec4<f32>,
    // x: the inside light on ground in sight (of its albedo); y: the
    // brightness the section keeps out of sight; z: the peek factor (a
    // notch over a shallow roof).
    inside: vec4<f32>,
    // xyz: the elder's lamp; w: its glow, 0..1.
    lamp: vec4<f32>,
    // rgb: the lamp's colour.
    lamp_color: vec4<f32>,
    // Roofed ground's light (of its albedo): x the ambient, y the lamp, z
    // the lanterns' channel; w the lamp's range.
    under: vec4<f32>,
    // rgb: the void beyond the rim round what is in sight; w: the fog's
    // night floor.
    void_color: vec4<f32>,
    // The fog of war. xy: its texture's lowest corner (world x, z); z: its
    // side; w: 1 in the fog mode.
    fog: vec4<f32>,
    // x: the brightness seen ground keeps; y: how far toward grey; z: the
    // ease from the last texture (b, a) to this one (r, g); w: the soft
    // edge's reach.
    fog_look: vec4<f32>,
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

// Line of sight: the height ground is hidden over at `xz` while inside:
// `sight.y`, the ceiling's or `cutaway::INSIDE_WALL` over the feet,
// whichever is lower; at most `light.w` over the feet in the tiles from the
// player's toward the camera (`window.w` tiles along x and z on the
// camera's side), so a near wall doesn't hide the body. The box's sides
// lie just inside the player tile's back edges (walls behind stay) and just
// outside the far tiles' edges, so walls are cut whole (blocky, as the
// voxels are) and a wall face on an edge is decided one way.
fn in_box(xz: vec2<f32>) -> bool {
    let rel = (xz - round(cutaway.eye.xz)) * sign(cutaway.toward.xz);
    let n = cutaway.window.w;
    return all(rel > vec2<f32>(-0.49)) && all(rel < vec2<f32>(n + 0.51));
}

fn plane_at(xz: vec2<f32>) -> f32 {
    let low = min(cutaway.eye.y + cutaway.light.w, cutaway.sight.y);
    return select(cutaway.sight.y, low, in_box(xz));
}

// Line of sight: how much of the ground at `p` is hidden (dithered) over
// `plane_at`: the inside factor; in the near-wall box also the peek
// factor, so a shallow roof (a stair just dug down) gets a notch over the
// body and nothing else is cut.
fn hidden(p: vec3<f32>) -> f32 {
    if cutaway.sight.z < 0.5 || p.y <= plane_at(p.xz) {
        return 0.0;
    }
    return select(cutaway.sight.x, max(cutaway.sight.x, cutaway.inside.z), in_box(p.xz));
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
    if max(cut(in.world_position.xyz), hidden(in.world_position.xyz)) > threshold(in.position.xy) {
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

// The cutaway (VERTICALITY.md V7): a volume scene's ground over the
// player's head is not drawn round the line from the head toward the
// camera, with a dithered edge, and back faces (the inside of solid ground
// seen through the hole) are drawn flat in the section colour. The cut
// test is `cutaway::cut` in app/src/cutaway.rs; keep the two in step (and
// cutaway_prepass.wesl, which cuts the depth prepass the same way).
// In the line of sight mode (`cutaway::ViewMode::Sight`) the cylinder is
// off; inside, ground over the ceiling's height is hidden (`hidden`) and
// the rest shaded by the visibility texture (`shade`).
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
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
    // brightness the section keeps out of sight.
    inside: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cutaway: Cutaway;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sight_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sight_sampler: sampler;

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
// the ceiling's, but `light.w` over the feet in the tiles from the
// player's toward the camera (`window.w` tiles along x and z on the
// camera's side), so a near wall doesn't hide the body. The box's sides
// lie just inside the player tile's back edges (walls behind stay) and just
// outside the far tiles' edges, so walls are cut whole (blocky, as the
// voxels are) and a wall face on an edge is decided one way.
fn plane_at(xz: vec2<f32>) -> f32 {
    let rel = (xz - round(cutaway.eye.xz)) * sign(cutaway.toward.xz);
    let n = cutaway.window.w;
    let near = all(rel > vec2<f32>(-0.49)) && all(rel < vec2<f32>(n + 0.51));
    let low = min(cutaway.eye.y + cutaway.light.w, cutaway.sight.y);
    return select(cutaway.sight.y, low, near);
}

// Line of sight: how much of the ground at `p` is hidden (the inside
// factor, dithered) over `plane_at`.
fn hidden(p: vec3<f32>) -> f32 {
    if cutaway.sight.z < 0.5 || p.y <= plane_at(p.xz) {
        return 0.0;
    }
    return cutaway.sight.x;
}

// Line of sight: how visible the ground at `p` is from the player's head
// (`sight.rs`), 0 outside the window.
fn visibility(p: vec3<f32>) -> f32 {
    let uv = (p.xz - cutaway.window.xy) / cutaway.window.z;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    return textureSampleLevel(sight_map, sight_sampler, uv, 0.0).r;
}

// Where the ray from `p` toward the camera crosses the hidden height (two
// steps, as the height varies).
fn cap(p: vec3<f32>) -> vec3<f32> {
    let toward = cutaway.toward.xyz;
    let rise = max(toward.y, 0.05);
    var q = p + toward * max(plane_at(p.xz) - p.y, 0.0) / rise;
    q = p + toward * max(plane_at(q.xz) - p.y, 0.0) / rise;
    return q;
}

// Line of sight: inside, ground in sight is lit falling off with distance
// from the player, out of sight dimmed and greyed; blended by the inside
// factor, so outside nothing changes.
fn shade(color: vec3<f32>, albedo: vec3<f32>, p: vec3<f32>, is_front: bool) -> vec3<f32> {
    let f = cutaway.sight.x * cutaway.sight.z;
    if f <= 0.0 {
        return color;
    }
    // A back face is the section seen through the cut: shade it where the
    // view ray crossed the cut (its cap), not deep down where it lies.
    let q = select(cap(p), p, is_front);
    let seen = visibility(q);
    let d = distance(q.xz, cutaway.eye.xz);
    let fall = mix(1.0, cutaway.light.z, smoothstep(cutaway.light.x, cutaway.light.y, d));
    let grey = vec3<f32>(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)));
    // The section out of sight goes darker than lit ground does, so the
    // floors in sight read against the cut tops round them.
    let keep = select(cutaway.inside.y, cutaway.eye.w, is_front);
    let dim = mix(color, grey, cutaway.sight.w) * keep;
    // In sight, the mountain's shadow still lies on the floors: an inside
    // light (a share of the albedo) lifts them, falling off as the light.
    let lit = select(color, (color + albedo * cutaway.inside.x) * fall, is_front);
    return mix(color, mix(dim, lit, seen), f);
}

// A 4 x 4 ordered dither threshold in (0, 1) for a pixel.
fn threshold(pixel: vec2<f32>) -> f32 {
    let x = u32(pixel.x) % 4u;
    let y = u32(pixel.y) % 4u;
    let bayer = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    return (f32(bayer[y * 4u + x]) + 0.5) / 16.0;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if max(cut(in.world_position.xyz), hidden(in.world_position.xyz)) > threshold(in.position.xy) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    if is_front {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = vec4<f32>(cutaway.section.rgb, 1.0);
    }
    out.color = vec4<f32>(shade(out.color.rgb, pbr_input.material.base_color.rgb, in.world_position.xyz, is_front), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

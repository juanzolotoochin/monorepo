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
    // rgb: the void beyond the rim round what is in sight.
    void_color: vec4<f32>,
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

// Line of sight: the grid at `p` (`sight.rs`, filtered): r how visible
// from the player's head, b the lanterns' light, a open sky. Outside the
// window: unseen, unlit, open.
fn grid(p: vec3<f32>) -> vec4<f32> {
    let uv = (p.xz - cutaway.window.xy) / cutaway.window.z;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return textureSampleLevel(sight_map, sight_sampler, uv, 0.0);
}

// Line of sight: how much of the ground at `p` is void (`sight::void`):
// its tile's near value, unfiltered, so the rim round floor in sight is
// whole tiles. Outside the window: void.
fn void_at(p: vec3<f32>) -> f32 {
    let t = floor(p.xz - cutaway.window.xy);
    let size = cutaway.window.z;
    if any(t < vec2<f32>(0.0)) || any(t >= vec2<f32>(size)) {
        return 1.0;
    }
    return 1.0 - textureLoad(sight_map, vec2<i32>(t), 0).g;
}

// Roofed ground's light (of its albedo, `cutaway::underground`): the
// ambient, the elder's lamp where it is in sight (`seen`), and the
// lanterns' channel.
fn underground(p: vec3<f32>, n: vec3<f32>, seen: f32, light: f32) -> vec3<f32> {
    let to = cutaway.lamp.xyz - p;
    let d = max(length(to), 0.001);
    let reach = clamp(1.0 - (d / cutaway.under.w) * (d / cutaway.under.w), 0.0, 1.0);
    let facing = 0.35 + 0.65 * max(dot(n, to / d), 0.0);
    let lamp = clamp(reach * reach * facing * cutaway.lamp.w * seen, 0.0, 1.0);
    return vec3<f32>(cutaway.under.x) + cutaway.lamp_color.rgb * (cutaway.under.y * lamp) + vec3<f32>(cutaway.under.z * clamp(light, 0.0, 1.0));
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

// Line of sight: inside, roofed ground is lit by the lamp (where in
// sight), the ambient and the lanterns (`underground`), ground under open
// sky by the sun as before (in sight falling off with distance, out of
// sight dimmed and greyed), the section by its visibility; blended by the
// inside factor (in the notch also the peek factor), so outside nothing
// changes. The void is mixed in after fog (`void_amount`).
fn shade(color: vec3<f32>, albedo: vec3<f32>, p: vec3<f32>, n: vec3<f32>, is_front: bool) -> vec3<f32> {
    // Inside everywhere; peeking (a shallow roof) only in the notch, so the
    // stair under it is lit by the lamp, not left in the roof's shadow.
    let peek = select(0.0, cutaway.inside.z, in_box(p.xz));
    let f = max(cutaway.sight.x, peek) * cutaway.sight.z;
    if f <= 0.0 {
        return color;
    }
    // A back face is the section seen through the cut: shade it where the
    // view ray crossed the cut (its cap), not deep down where it lies.
    let q = select(cap(p), p, is_front);
    let g = grid(q);
    let seen = g.r;
    var c: vec3<f32>;
    if is_front {
        // Under open sky (out of a mouth) the sun as before: in sight
        // falling off with distance, out of sight dimmed and greyed.
        let d = distance(q.xz, cutaway.eye.xz);
        let fall = mix(1.0, cutaway.light.z, smoothstep(cutaway.light.x, cutaway.light.y, d));
        let grey = vec3<f32>(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)));
        let dim = mix(color, grey, cutaway.sight.w) * cutaway.eye.w;
        let sunlit = mix(dim, color * fall, seen);
        // Under a roof no sun: the lamp, the ambient and the lanterns.
        let roofed = albedo * underground(p, n, seen, g.b);
        c = mix(roofed, sunlit, g.a);
    } else {
        // The section: the rim round floor in sight keeps its colour.
        c = color * mix(cutaway.inside.y, 1.0, seen);
    }
    // The notch's own cut faces keep the section colour.
    return mix(color, c, select(cutaway.sight.x * cutaway.sight.z, f, is_front));
}

// Line of sight: how much of the ground at `p` is void while inside (after
// fog, so it is near black): beyond the rim round floor in sight. A wall
// face goes by the tile it faces, a back face by its cap.
fn void_amount(p: vec3<f32>, n: vec3<f32>, is_front: bool) -> f32 {
    let f = cutaway.sight.x * cutaway.sight.z;
    if f <= 0.0 {
        return 0.0;
    }
    return f * void_at(select(cap(p), p + n * 0.05, is_front));
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
    out.color = vec4<f32>(shade(out.color.rgb, pbr_input.material.base_color.rgb, in.world_position.xyz, normalize(in.world_normal), is_front), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    let gone = void_amount(in.world_position.xyz, normalize(in.world_normal), is_front);
    out.color = vec4<f32>(mix(out.color.rgb, cutaway.void_color.rgb, gone), out.color.a);
    return out;
}

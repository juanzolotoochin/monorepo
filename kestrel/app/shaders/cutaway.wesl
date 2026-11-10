// The cutaway (VERTICALITY.md V7): a volume scene's ground over the
// player's head is not drawn round the line from the head toward the
// camera, with a dithered edge, and back faces (the inside of solid ground
// seen through the hole) are drawn flat in the section colour. The cut
// test is `cutaway::cut` in app/src/cutaway.rs; keep the two in step (and
// cutaway_prepass.wesl, which cuts the depth prepass the same way).
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

// A 4 x 4 ordered dither threshold in (0, 1) for a pixel.
fn threshold(pixel: vec2<f32>) -> f32 {
    let x = u32(pixel.x) % 4u;
    let y = u32(pixel.y) % 4u;
    let bayer = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    return (f32(bayer[y * 4u + x]) + 0.5) / 16.0;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if cut(in.world_position.xyz) > threshold(in.position.xy) {
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
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

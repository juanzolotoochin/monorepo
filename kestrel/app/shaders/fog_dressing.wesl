// The fog of war on the town's dressing (M9.3 of the Sacred Tree fog;
// `fog_dressing.rs`): the pines, the tree's canopy, lamp posts and the rest
// of the baked environment, drawn as the standard material and then as the
// team knows their ground: in sight as they are, seen before dimmed and
// greyed (with the night floor), never seen the void. The surface's fog
// only: inside, `cutaway::hide_outdoors` hides the dressing. The fog's
// look is the ground's (`cutaway.wesl`'s `fogged`); keep the two in step.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
};

struct FogDressing {
    // xy: the fog texture's lowest corner (world x, z); z: its side; w: 1
    // in the fog mode.
    fog: vec4<f32>,
    // x: the brightness seen ground keeps; y: how far toward grey; z: the
    // ease from the last texture (b) to this one (r); w: the soft edge's
    // reach.
    fog_look: vec4<f32>,
    // rgb: never seen; w: the share of its albedo seen ground keeps at
    // least.
    void_color: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> dressing: FogDressing;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var fog_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var fog_sampler: sampler;

// The surface's fog at `xz`: 0 never seen, 0.5 seen, 1 in sight, eased
// from the last texture. Outside the town's square: never seen.
fn fog_tap(xz: vec2<f32>) -> f32 {
    let uv = (xz - dressing.fog.xy) / dressing.fog.z;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    let s = textureSampleLevel(fog_map, fog_sampler, uv, 0.0);
    return mix(s.b, s.r, dressing.fog_look.z);
}

// ...softened as the ground's.
fn fog_state(xz: vec2<f32>) -> f32 {
    let r = dressing.fog_look.w;
    let sum = fog_tap(xz) * 2.0 + fog_tap(xz + vec2<f32>(r, 0.0)) + fog_tap(xz - vec2<f32>(r, 0.0)) + fog_tap(xz + vec2<f32>(0.0, r)) + fog_tap(xz - vec2<f32>(0.0, r));
    return sum / 6.0;
}

fn fogged(color: vec3<f32>, albedo: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    let state = fog_state(p.xz);
    let seen = smoothstep(0.05, 0.45, state);
    let now = smoothstep(0.55, 0.95, state);
    let lum = vec3<f32>(0.2126, 0.7152, 0.0722);
    let grey = vec3<f32>(dot(color, lum));
    let dim = max(mix(color, grey, dressing.fog_look.y) * dressing.fog_look.x, vec3<f32>(dot(albedo, lum) * dressing.void_color.w));
    return mix(dressing.void_color.rgb, mix(dim, color, now), seen);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    if dressing.fog.w > 0.5 {
        out.color = vec4<f32>(fogged(out.color.rgb, pbr_input.material.base_color.rgb, in.world_position.xyz), out.color.a);
    }
    return out;
}

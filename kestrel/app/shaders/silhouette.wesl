// The elder's silhouette through rock (K9 of the climbing design,
// app/src/climb_view.rs): the elder's meshes drawn again after the opaque
// pass with the depth test reversed, so only where something is in front
// of them, and here only where that is more than `settings.x` meters
// nearer the camera than the body (the depth prepass), so the elder's own
// limbs in front of its chest don't show it. A flat colour, blended.
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::view_transformations::{depth_ndc_to_view_z, position_world_to_view};
import bevy_pbr::prepass::utils::prepass_depth;

struct Silhouette {
    // The colour, linear, with its alpha.
    color: vec4<f32>,
    // x: how much nearer the camera what hides the body must be, meters.
    settings: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> silhouette: Silhouette;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    @if(DEPTH_PREPASS) {
        let here = position_world_to_view(in.world_position.xyz).z;
        let front = depth_ndc_to_view_z(prepass_depth(in.position, 0u));
        // View z grows toward the camera.
        if front - here < silhouette.settings.x {
            discard;
        }
    }
    return silhouette.color;
}

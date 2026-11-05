//! The test walker's body (V.7; V8 of the verticality design,
//! `kestrel/VERTICALITY.md`). The walker is a sprite (`SpriteJob::Follow`)
//! a volume scene adds (`walkers:` in the scene file) without a scenario,
//! so the settlement view, which draws a scenario's sprites by their skin,
//! has none for it. This draws each as a small blocky body, between the
//! last two ticks, its feet on the floor the simulation says it stands on
//! (`SpriteState::height`), which may be under another (a tunnel, the cave
//! under the bridge). It does not animate: it proves the paths, not the art.
//! In the line of sight view (`cutaway::SightView`), inside, a walker out
//! of the player's sight fades out (each body has its own materials).

use bevy::prelude::*;
use service::WorldScoped;
use sim::settlement::{AgentState, SpriteJob};

use crate::lockstep::{SessionService, WorldMirror};

/// A walker's body, by its agent index, with its materials (suit, visor,
/// dark) and the alpha they were last drawn with.
#[derive(Component)]
pub struct WalkerBody(usize, [Handle<StandardMaterial>; 3], f32);

/// Its meshes, made once.
#[derive(Clone)]
pub struct WalkerArt {
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    leg: Handle<Mesh>,
}

const SUIT: Color = Color::srgb(0.93, 0.63, 0.2);
const VISOR: Color = Color::srgb(0.1, 0.78, 1.0);
const VISOR_GLOW: LinearRgba = LinearRgba::rgb(0.4, 3.0, 4.0);
const DARK: Color = Color::srgb(0.065, 0.095, 0.14);

/// Under this alpha a fading body is not drawn at all.
const HIDDEN: f32 = 0.03;

/// Sets a body's materials to `alpha` (opaque at 1).
fn fade(materials: &mut Assets<StandardMaterial>, handles: &[Handle<StandardMaterial>; 3], alpha: f32) {
    let mode = if alpha >= 0.999 { AlphaMode::Opaque } else { AlphaMode::Blend };
    for (handle, color) in handles.iter().zip([SUIT, VISOR, DARK]) {
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color = color.with_alpha(alpha);
            material.alpha_mode = mode;
            if color == VISOR {
                material.emissive = VISOR_GLOW * alpha;
            }
        }
    }
}

fn walking(a: &AgentState) -> bool {
    a.sprite().is_some_and(|s| s.job == SpriteJob::Follow)
}

/// Places (and makes, and removes) the walkers' bodies.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    mirror: Res<WorldMirror>,
    service: Res<SessionService>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bodies: Query<(Entity, &mut WalkerBody, &mut Transform, &mut Visibility)>,
    mut art: Local<Option<WalkerArt>>,
    sight: Option<Res<crate::cutaway::SightView>>,
) {
    let volume = service.world().is_some_and(|w| w.terrain().volume.is_some());
    let walkers: Vec<(usize, Vec3)> = match (&mirror.current, volume) {
        (Some(current), true) => current
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| walking(a))
            .map(|(i, a)| {
                let before = mirror.previous.as_ref().and_then(|p| p.agents.get(i)).filter(|b| b.id == a.id && walking(b)).unwrap_or(a);
                let at = mirror.to_view(before.body.position).lerp(mirror.to_view(a.body.position), mirror.alpha);
                let (h0, h1) = (before.sprite().unwrap().height.to_f32(), a.sprite().unwrap().height.to_f32());
                let feet = h0 + (h1 - h0) * mirror.alpha;
                (i, at.with_y(feet))
            })
            .collect(),
        _ => Vec::new(),
    };
    for (entity, mut body, mut transform, mut visibility) in &mut bodies {
        match walkers.iter().find(|(i, ..)| *i == body.0) {
            Some(&(_, at)) => {
                transform.translation = at;
                let heading = a_heading(&mirror, body.0);
                transform.rotation = Quat::from_rotation_y(heading);
                let alpha = sight.as_ref().map_or(1.0, |s| s.body_alpha(at));
                // Quantized, so a steady body doesn't touch its materials.
                let alpha = (alpha * 32.0).round() / 32.0;
                if alpha != body.2 {
                    fade(&mut materials, &body.1, alpha);
                    body.2 = alpha;
                }
                let shown = if alpha < HIDDEN { Visibility::Hidden } else { Visibility::Inherited };
                visibility.set_if_neq(shown);
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for &(i, at) in &walkers {
        if bodies.iter().any(|(_, b, ..)| b.0 == i) {
            continue;
        }
        let art = art.get_or_insert_with(|| WalkerArt {
            torso: meshes.add(Cuboid::new(0.34, 0.42, 0.22)),
            head: meshes.add(Cuboid::new(0.26, 0.24, 0.24)),
            leg: meshes.add(Cuboid::new(0.1, 0.36, 0.12)),
        });
        let suit = materials.add(StandardMaterial { base_color: SUIT, perceptual_roughness: 0.6, ..default() });
        let visor = materials.add(StandardMaterial { base_color: VISOR, emissive: VISOR_GLOW, ..default() });
        let dark = materials.add(StandardMaterial { base_color: DARK, perceptual_roughness: 0.8, ..default() });
        commands
            .spawn((WalkerBody(i, [suit.clone(), visor.clone(), dark.clone()], 1.0), WorldScoped::default(), Transform::from_translation(at), Visibility::default()))
            .with_children(|body| {
                body.spawn((Mesh3d(art.leg.clone()), MeshMaterial3d(dark.clone()), Transform::from_xyz(-0.08, 0.18, 0.0)));
                body.spawn((Mesh3d(art.leg.clone()), MeshMaterial3d(dark.clone()), Transform::from_xyz(0.08, 0.18, 0.0)));
                body.spawn((Mesh3d(art.torso.clone()), MeshMaterial3d(suit.clone()), Transform::from_xyz(0.0, 0.57, 0.0)));
                body.spawn((Mesh3d(art.head.clone()), MeshMaterial3d(suit.clone()), Transform::from_xyz(0.0, 0.92, 0.0)));
                body.spawn((Mesh3d(art.head.clone()), MeshMaterial3d(visor), Transform::from_xyz(0.0, 0.94, 0.1).with_scale(Vec3::new(0.8, 0.25, 0.2))));
            });
    }
}

/// The heading agent `i` faces, as the settlement view turns bodies: its
/// front (+z) along the simulation's heading (0 along +y).
fn a_heading(mirror: &WorldMirror, i: usize) -> f32 {
    mirror.current.as_ref().and_then(|c| c.agents.get(i)).map_or(0.0, |a| a.body.heading.to_f32())
}

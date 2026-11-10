//! The test walker's body (V.7; V8 of the verticality design,
//! `kestrel/VERTICALITY.md`). The walker is a sprite (`SpriteJob::Follow`)
//! a volume scene adds (`walkers:` in the scene file) without a scenario,
//! so the settlement view, which draws a scenario's sprites by their skin,
//! has none for it. This draws each as a small blocky body, between the
//! last two ticks, its feet on the floor the simulation says it stands on
//! (`SpriteState::height`), which may be under another (a tunnel, the cave
//! under the bridge). It doesn't animate: it proves the paths, not the art.

use bevy::prelude::*;
use service::WorldScoped;
use sim::settlement::{AgentState, SpriteJob};

use crate::lockstep::{SessionService, WorldMirror};

/// A walker's body, by its agent index.
#[derive(Component)]
pub struct WalkerBody(usize);

/// Its meshes and materials, made once.
#[derive(Clone)]
pub struct WalkerArt {
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    leg: Handle<Mesh>,
    suit: Handle<StandardMaterial>,
    visor: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
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
    mut bodies: Query<(Entity, &WalkerBody, &mut Transform)>,
    mut art: Local<Option<WalkerArt>>,
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
    for (entity, body, mut transform) in &mut bodies {
        match walkers.iter().find(|(i, ..)| *i == body.0) {
            Some(&(_, at)) => {
                transform.translation = at;
                let heading = a_heading(&mirror, body.0);
                transform.rotation = Quat::from_rotation_y(heading);
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for &(i, at) in &walkers {
        if bodies.iter().any(|(_, b, _)| b.0 == i) {
            continue;
        }
        let art = art.get_or_insert_with(|| WalkerArt {
            torso: meshes.add(Cuboid::new(0.34, 0.42, 0.22)),
            head: meshes.add(Cuboid::new(0.26, 0.24, 0.24)),
            leg: meshes.add(Cuboid::new(0.1, 0.36, 0.12)),
            suit: materials.add(StandardMaterial { base_color: Color::srgb(0.93, 0.63, 0.2), perceptual_roughness: 0.6, ..default() }),
            visor: materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.78, 1.0), emissive: LinearRgba::rgb(0.4, 3.0, 4.0), ..default() }),
            dark: materials.add(StandardMaterial { base_color: Color::srgb(0.065, 0.095, 0.14), perceptual_roughness: 0.8, ..default() }),
        });
        commands
            .spawn((WalkerBody(i), WorldScoped::default(), Transform::from_translation(at), Visibility::default()))
            .with_children(|body| {
                body.spawn((Mesh3d(art.leg.clone()), MeshMaterial3d(art.dark.clone()), Transform::from_xyz(-0.08, 0.18, 0.0)));
                body.spawn((Mesh3d(art.leg.clone()), MeshMaterial3d(art.dark.clone()), Transform::from_xyz(0.08, 0.18, 0.0)));
                body.spawn((Mesh3d(art.torso.clone()), MeshMaterial3d(art.suit.clone()), Transform::from_xyz(0.0, 0.57, 0.0)));
                body.spawn((Mesh3d(art.head.clone()), MeshMaterial3d(art.suit.clone()), Transform::from_xyz(0.0, 0.92, 0.0)));
                body.spawn((Mesh3d(art.head.clone()), MeshMaterial3d(art.visor.clone()), Transform::from_xyz(0.0, 0.94, 0.1).with_scale(Vec3::new(0.8, 0.25, 0.2))));
            });
    }
}

/// The heading agent `i` faces, as the settlement view turns bodies: its
/// front (+z) along the simulation's heading (0 along +y).
fn a_heading(mirror: &WorldMirror, i: usize) -> f32 {
    mirror.current.as_ref().and_then(|c| c.agents.get(i)).map_or(0.0, |a| a.body.heading.to_f32())
}

//! Terrain-aware hover flight. The same controller runs for keyboard and scripted demos.
use crate::lockstep::WorldMirror;
use crate::scene_view::SceneView;
use bevy::prelude::*;
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};

/// The local player's hover flight, as the views see it (World layer).
#[derive(Resource, Default)]
pub struct Flight {
    pub requested: bool,
    pub airborne: bool,
    pub jump: sim::settlement::avatar::JumpPhase,
    pub support: f32,
    sound_timer: f32,
}
#[derive(Component)]
pub struct Deck;
#[derive(Component)]
pub struct TerrainVisual;
#[derive(Component)]
pub struct Exhaust;

/// The ground's height under `p` in a scene: walkable surfaces, or for a
/// scene with terrain, its platforms.
pub fn height(pack: &voxel_assets::ScenePack, p: Vec3) -> Option<f32> {
    let scene = &pack.scene;
    let surface = pack
        .environment
        .iter()
        .flat_map(|e| &e.walkable)
        .filter(|b| p.x >= b.min[0] && p.x <= b.max[0] && p.z >= b.min[1] && p.z <= b.max[1])
        .map(|b| b.height)
        .max_by(f32::total_cmp);
    match &scene.terrain {
        None => Some(surface.unwrap_or(0.0)),
        Some(t) => t
            .platforms
            .iter()
            .filter(|b| p.x >= b.min[0] && p.x <= b.max[0] && p.z >= b.min[1] && p.z <= b.max[1])
            .map(|b| b.height)
            .max_by(f32::total_cmp),
    }
}
impl Service for Flight {
    type Layer = WorldLayer;
    type Deps = (WorldMirror, SceneView, crate::character::Avatar);

    /// Raises the scene's platforms (`SceneView` hides the deck under them).
    fn init(world: &mut World) -> Self {
        let scene = world.resource::<SceneView>();
        let terrain = scene.scene().terrain.clone();
        let authored = scene.pack.environment.as_ref().map_or_else(Vec::new, |e| e.walkable.clone());
        if let Some(terrain) = terrain {
            world.run_system_cached_with(raise_platforms, (terrain, authored)).expect("raising the scene's platforms");
        }
        Flight::default()
    }

    fn systems(app: &mut App) {
        // `follow_avatar` (the Avatar service, which ticks first) writes it.
        app.init_resource::<crate::prediction::DrawnAvatar>()
            .add_service_systems::<Self, _>(Phase::Mirror, update.run_if(in_state(WorldState::Active)));
    }
}

/// The platforms' mesh and materials, uploaded once.
#[derive(Resource)]
struct PlatformArt(Handle<Mesh>, [Handle<StandardMaterial>; 3]);

fn raise_platforms(
    In((terrain, authored)): In<(voxel_assets::Terrain, Vec<voxel_assets::WalkableSurface>)>,
    mut commands: Commands,
    art: Option<Res<PlatformArt>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let PlatformArt(cube, [side, top, glow]) = match art {
        Some(art) => PlatformArt(art.0.clone(), art.1.clone()),
        None => {
            let art = PlatformArt(
                meshes.add(Cuboid::default()),
                [
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.09, 0.14, 0.21),
                        metallic: 0.6,
                        ..default()
                    }),
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.24, 0.32, 0.37),
                        metallic: 0.35,
                        ..default()
                    }),
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.05, 0.8, 1.0),
                        emissive: LinearRgba::rgb(0.1, 3.0, 5.0),
                        ..default()
                    }),
                ],
            );
            commands.insert_resource(PlatformArt(art.0.clone(), art.1.clone()));
            art
        }
    };
    let mut block = |pos: Vec3, size: Vec3, mat: &Handle<StandardMaterial>| {
        commands.spawn((
            TerrainVisual,
            WorldScoped::default(),
            Mesh3d(cube.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(pos).with_scale(size),
        ));
    };
    for p in &terrain.platforms {
        let x = (p.min[0] + p.max[0]) * 0.5;
        let z = (p.min[1] + p.max[1]) * 0.5;
        let w = p.max[0] - p.min[0];
        let d = p.max[1] - p.min[1];
        block(
            Vec3::new(x, (p.height - 3.08) * 0.5, z),
            Vec3::new(w, p.height + 2.92, d),
            &side,
        );
        for ix in 0..w.ceil() as u32 {
            for iz in 0..d.ceil() as u32 {
                let tw = (w - ix as f32).min(1.0);
                let td = (d - iz as f32).min(1.0);
                let min = [p.min[0] + ix as f32, p.min[1] + iz as f32];
                let max = [min[0] + tw, min[1] + td];
                // Terrain supplies collision; an authored walkable surface
                // at the same height supplies the visible top. Drawing both
                // faces there causes depth fighting across the whole tile.
                if authored.iter().any(|surface|
                    (surface.height - p.height).abs() < 0.0001
                        && (0..2).all(|axis| surface.min[axis] <= min[axis] + 0.0001
                            && surface.max[axis] >= max[axis] - 0.0001)) {
                    continue;
                }
                block(
                    Vec3::new(
                        p.min[0] + ix as f32 + tw * 0.5,
                        p.height - 0.04,
                        p.min[1] + iz as f32 + td * 0.5,
                    ),
                    Vec3::new(tw * 0.96, 0.08, td * 0.96),
                    &top,
                );
            }
        }
        for edge in [p.min[1] + 0.07, p.max[1] - 0.07] {
            block(
                Vec3::new(x, p.height + 0.01, edge),
                Vec3::new((w - 0.14).max(0.01), 0.025, 0.035),
                &glow,
            );
        }
        for edge in [p.min[0] + 0.07, p.max[0] - 0.07] {
            block(
                Vec3::new(edge, p.height + 0.01, z),
                Vec3::new(0.035, 0.025, (d - 0.14).max(0.01)),
                &glow,
            );
        }
    }
}

/// Mirrors the avatar's flight for the HUD, sound, exhaust and jump pose.
/// The flight itself is simulated (`kestrel/sim::settlement::avatar`); this
/// is the drawn avatar's (`prediction::DrawnAvatar`), so on a joined client
/// the flame and thrust start and stop with the predicted avatar.
pub fn update(
    time: Res<Time>,
    mirror: Res<crate::lockstep::WorldMirror>,
    drawn: Res<crate::prediction::DrawnAvatar>,
    mut flight: ResMut<Flight>,
    sound: Res<crate::sound::Sound>,
    mut exhaust: Query<(Entity, &mut Transform, &mut Visibility), With<Exhaust>>,
    player: Option<Single<Entity, With<crate::character::Player>>>,
    parents: Query<&ChildOf>,
) {
    let Some(drawn) = drawn.0.as_ref().filter(|drawn| Some(drawn.agent) == mirror.player()) else { return };
    let avatar = drawn.avatar();
    let climbing = avatar.height > drawn.from.avatar.height;
    flight.requested = avatar.flight.requested;
    let was_airborne = flight.airborne;
    flight.airborne = avatar.flight.airborne;
    flight.jump = avatar.jump;
    flight.support = avatar.flight.support.to_f32();
    let dt = time.delta_secs().min(0.1);
    flight.sound_timer -= dt;
    let jumping = !matches!(flight.jump, sim::settlement::avatar::JumpPhase::Grounded);
    if was_airborne
        && matches!(
            flight.jump,
            sim::settlement::avatar::JumpPhase::Landing { .. }
        )
    {
        sound.play(crate::sound::Cue::Land);
    }
    if flight.airborne && !jumping && flight.sound_timer <= 0.0 {
        sound.play(crate::sound::Cue::Jetpack);
        flight.sound_timer = 0.16;
    }
    if !flight.airborne {
        flight.sound_timer = 0.0;
    }
    // Remote players' exhausts follow their own flight (`peers`).
    let Some(player) = player.map(|player| *player) else { return };
    for (entity, mut transform, mut visibility) in &mut exhaust {
        if !parents.iter_ancestors(entity).any(|ancestor| ancestor == player) {
            continue;
        }
        *visibility = if flight.airborne && !jumping {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.scale.y =
            0.4 + 0.1 * (time.elapsed_secs() * 35.0).sin() + if climbing { 0.25 } else { 0.0 };
    }
}

/// Pick the closest visible platform top (or the dug ground in a digging
/// scene), rather than projecting elevated clicks onto y=0.
pub fn cursor_surface(
    camera: &Camera,
    transform: &GlobalTransform,
    window: &Window,
    scene: &SceneView,
    service: &crate::lockstep::SessionService,
) -> Option<Vec3> {
    let ray = camera
        .viewport_to_world(transform, window.cursor_position()?)
        .ok()?;
    if scene.has_feature(voxel_assets::SceneFeature::Digging) {
        return crate::digging::ground_hit(ray, service);
    }
    let Some(terrain) = &scene.scene().terrain else {
        return crate::gameplay::cursor_ground(camera, transform, window);
    };
    terrain
        .platforms
        .iter()
        .filter_map(|platform| {
            let distance =
                ray.intersect_plane(Vec3::Y * platform.height, InfinitePlane3d::new(Vec3::Y))?;
            let p = ray.get_point(distance);
            (p.x >= platform.min[0]
                && p.x <= platform.max[0]
                && p.z >= platform.min[1]
                && p.z <= platform.max[1])
                .then_some((distance, p))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
        .or_else(|| crate::gameplay::cursor_ground(camera, transform, window))
}

use super::*;

fn art(world: &mut World) {
    if world.contains_resource::<Art>() {
        return;
    }
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    let mut mat = |rgb: [f32; 3], glow: f32, alpha: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgba(rgb[0], rgb[1], rgb[2], alpha),
            emissive: LinearRgba::rgb(rgb[0] * glow, rgb[1] * glow, rgb[2] * glow),
            alpha_mode: if alpha < 1.0 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            metallic: 0.45,
            ..default()
        })
    };
    let a = Art {
        cube,
        metal: mat([0.25, 0.34, 0.42], 0.0, 1.0),
        dark: mat([0.04, 0.07, 0.12], 0.0, 1.0),
        blue: mat([0.05, 0.65, 1.0], 3.0, 1.0),
        green: mat([0.2, 1.0, 0.45], 3.0, 1.0),
        gold: mat([1.0, 0.48, 0.08], 3.0, 1.0),
        ghost: mat([0.1, 0.75, 1.0], 1.7, 0.48),
    };
    world.insert_resource(a);
}
fn block(world: &mut World, pos: Vec3, size: Vec3, material: Handle<StandardMaterial>) -> Entity {
    let cube = world.resource::<Art>().cube.clone();
    world
        .spawn((
            Trial,
            Mesh3d(cube),
            MeshMaterial3d(material),
            Transform::from_translation(pos).with_scale(size),
        ))
        .id()
}
pub fn setup(world: &mut World) {
    let index = world.resource::<SceneLibrary>().current;
    let reset = world
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::KeyR);
    if world.resource::<Memory>().current == Some(index) && !reset {
        return;
    }
    let old: Vec<_> = world
        .query_filtered::<Entity, With<Trial>>()
        .iter(world)
        .collect();
    for e in old {
        world.despawn(e);
    }
    *world.resource_mut::<Memory>() = Memory {
        current: Some(index),
        ..default()
    };
    art(world);
    let a = world.resource::<Art>();
    let (cube, metal, dark, blue, green, gold, ghost) = (
        a.cube.clone(),
        a.metal.clone(),
        a.dark.clone(),
        a.blue.clone(),
        a.green.clone(),
        a.gold.clone(),
        a.ghost.clone(),
    );
    world
        .spawn((Trial, Ghost, Transform::default(), Visibility::Hidden))
        .with_children(|p| {
            for (pos, size) in [
                (Vec3::new(0.0, 1.03, 0.0), Vec3::new(0.66, 0.66, 0.4)),
                (Vec3::new(0.0, 1.62, 0.0), Vec3::splat(0.42)),
                (Vec3::new(-0.19, 0.38, 0.0), Vec3::new(0.23, 0.65, 0.25)),
                (Vec3::new(0.19, 0.38, 0.0), Vec3::new(0.23, 0.65, 0.25)),
                (Vec3::new(-0.43, 0.95, 0.0), Vec3::new(0.16, 0.5, 0.2)),
                (Vec3::new(0.43, 0.95, 0.0), Vec3::new(0.16, 0.5, 0.2)),
            ] {
                p.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(ghost.clone()),
                    Transform::from_translation(pos).with_scale(size),
                ));
            }
            p.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(blue.clone()),
                Transform::from_xyz(0.0, 1.66, 0.225).with_scale(Vec3::new(0.34, 0.08, 0.02)),
            ));
        });
    if !world.resource::<SceneLibrary>().has_feature(voxel_assets::SceneFeature::MemoryTrial) {
        return;
    }
    for (i, p) in PADS.iter().enumerate() {
        block(
            world,
            *p + Vec3::Y * 0.025,
            Vec3::new(1.6, 0.05, 1.6),
            metal.clone(),
        );
        for (offset, size) in [
            (Vec3::new(0.0, 0.065, -0.72), Vec3::new(1.4, 0.04, 0.06)),
            (Vec3::new(0.0, 0.065, 0.72), Vec3::new(1.4, 0.04, 0.06)),
            (Vec3::new(-0.72, 0.065, 0.0), Vec3::new(0.06, 0.04, 1.4)),
            (Vec3::new(0.72, 0.065, 0.0), Vec3::new(0.06, 0.04, 1.4)),
        ] {
            let e = block(world, *p + offset, size, blue.clone());
            world.entity_mut(e).insert(Pad(i));
        }
        // L-shaped conductors reveal the relationship between the plates and the lock.
        for (start, end) in [
            (*p, Vec3::new(1.1, 0.0, p.z)),
            (Vec3::new(1.1, 0.0, p.z), Vec3::new(1.1, 0.0, 0.0)),
        ] {
            let size = Vec3::new(
                (end.x - start.x).abs().max(0.045),
                0.025,
                (end.z - start.z).abs().max(0.045),
            );
            let e = block(
                world,
                (start + end) * 0.5 + Vec3::Y * 0.012,
                size,
                dark.clone(),
            );
            world.entity_mut(e).insert(Cable(i));
        }
    }
    for n in -18..=18 {
        let z = n as f32 * 0.45;
        let door = z.abs() < 1.15;
        let e = block(
            world,
            Vec3::new(2.0, if door { 1.1 } else { 0.45 }, z),
            Vec3::new(0.25, if door { 2.2 } else { 0.9 }, 0.44),
            if door { blue.clone() } else { metal.clone() },
        );
        world.entity_mut(e).insert(Obstacle(Disc {
            center: Vec3::new(2.0, 0.0, z),
            radius: 0.24,
        }));
        if door {
            world.entity_mut(e).insert(Door);
        }
    }
    for z in [-1.4, 1.4] {
        block(
            world,
            Vec3::new(2.0, 1.35, z),
            Vec3::new(0.6, 2.7, 0.5),
            dark.clone(),
        );
    }
    block(
        world,
        Vec3::new(2.0, 2.8, 0.0),
        Vec3::new(0.6, 0.22, 3.3),
        metal.clone(),
    );
    for i in 0..8 {
        let e = block(
            world,
            Vec3::new(2.32, 0.3 + i as f32 * 0.29, 1.4),
            Vec3::new(0.04, 0.17, 0.3),
            dark.clone(),
        );
        world.entity_mut(e).insert(Meter(i));
    }
    block(
        world,
        Vec3::new(CORE.x, 0.14, CORE.z),
        Vec3::new(1.6, 0.28, 1.6),
        dark.clone(),
    );
    for i in 0..4 {
        let angle = i as f32 * std::f32::consts::FRAC_PI_2;
        block(
            world,
            Vec3::new(
                CORE.x + angle.cos() * 0.65,
                0.31,
                CORE.z + angle.sin() * 0.65,
            ),
            Vec3::new(0.16, 0.07, 0.16),
            green.clone(),
        );
    }
    let e = block(world, CORE, Vec3::splat(0.5), gold);
    world.entity_mut(e).insert(Core);
    world.spawn((
        Trial,
        Instructions,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.4, 0.9, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            left: px(36),
            top: px(135),
            ..default()
        },
    ));
}

pub(super) fn update(world: &mut World, m: &Memory, elapsed: f32, dt: f32) {
    for (mut transform, mut visibility) in world
        .query_filtered::<(&mut Transform, &mut Visibility), With<Ghost>>()
        .iter_mut(world)
    {
        if let Some(g) = m.ghost {
            *visibility = Visibility::Inherited;
            transform.translation = g.position;
            transform.rotation = g.rotation;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    if world.resource::<SceneLibrary>().has_feature(voxel_assets::SceneFeature::MemoryTrial) {
        let a = world.resource::<Art>();
        let (blue, green, dark) = (a.blue.clone(), a.green.clone(), a.dark.clone());
        for (pad, mut mat) in world
            .query::<(&Pad, &mut MeshMaterial3d<StandardMaterial>)>()
            .iter_mut(world)
        {
            mat.0 = if m.pads[pad.0] {
                green.clone()
            } else {
                blue.clone()
            };
        }
        for (cable, mut mat) in world
            .query::<(&Cable, &mut MeshMaterial3d<StandardMaterial>)>()
            .iter_mut(world)
        {
            mat.0 = if m.pads[cable.0] {
                green.clone()
            } else {
                dark.clone()
            };
        }
        for (meter, mut mat) in world
            .query::<(&Meter, &mut MeshMaterial3d<StandardMaterial>)>()
            .iter_mut(world)
        {
            mat.0 = if (meter.0 as f32) < m.charge * 8.0 {
                green.clone()
            } else {
                dark.clone()
            };
        }
        let doors: Vec<_> = world
            .query_filtered::<Entity, With<Door>>()
            .iter(world)
            .collect();
        for e in doors {
            let mut transform = world.get_mut::<Transform>(e).unwrap();
            transform.scale.y =
                (transform.scale.y - dt * if m.unlocked { 3.0 } else { 0.0 }).max(0.02);
            transform.translation.y = transform.scale.y * 0.5;
        }
        for (mut transform, mut visibility) in world
            .query_filtered::<(&mut Transform, &mut Visibility), With<Core>>()
            .iter_mut(world)
        {
            *visibility = if m.collected {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            transform.translation.y = CORE.y + (elapsed * 2.5).sin() * 0.12;
            transform.rotation = Quat::from_rotation_y(elapsed) * Quat::from_rotation_z(0.6);
        }
        let text = if m.collected {
            "FIELD TRIAL COMPLETE  /  TWO PLACES. ONE YOU.\nE  Record another echo    Q  Dismiss"
                .to_string()
        } else if m.unlocked {
            "RESONANCE LOCK RELEASED\nRecover the suspended core beyond the gate.".to_string()
        } else if m.recording {
            format!("RECORDING {:.1} / 8.0s\nWalk onto the far plate. E to project and return. Q to cancel.",m.elapsed)
        } else if m.ghost.is_some() {
            format!("ECHO ACTIVE  /  LOCK {:03}%\nYour echo holds its last pose. Stand on the other plate.",(m.charge*100.0)as u32)
        } else {
            "ECHO ANCHOR  /  E to record    Q to dismiss\nRecord a walk onto one plate, then press E again.\nYour past self repeats the walk while you move freely.".to_string()
        };
        for mut label in world
            .query_filtered::<&mut Text, With<Instructions>>()
            .iter_mut(world)
        {
            label.0 = text.clone();
        }
    }
}

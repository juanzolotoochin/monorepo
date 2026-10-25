//! Character Studio controls and layout. Playback stays in the parent module.
use super::*;
use crate::menu::LaunchMenu;
use bevy::ui::FocusPolicy;

fn button(parent: &mut ChildSpawnerCommands, action: Action, title: &str) {
    parent
        .spawn((
            action,
            Interaction::None,
            FocusPolicy::Block,
            Node {
                width: percent(100.0),
                min_height: px(30.0),
                padding: UiRect::axes(px(10.0), px(5.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.16, 0.20, 0.92)),
            BorderColor::all(Color::srgb(0.18, 0.37, 0.42)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.95, 0.94)),
            ));
        });
}
fn label(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.49, 0.82, 0.85)),
    ));
}

pub fn setup(
    mut commands: Commands,
    viewer: Res<Viewer>,
    library: Res<SceneLibrary>,
    roots: Query<Entity, With<Root>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !viewer.active || !roots.is_empty() {
        return;
    }
    commands.spawn((
        Stage,
        Mesh3d(meshes.add(Cuboid::new(4.0, STUDIO_FLOOR_THICKNESS, 4.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.09, 0.17, 0.21),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, STUDIO_FLOOR_Y - STUDIO_FLOOR_THICKNESS * 0.5, 0.0),
    ));
    commands.spawn((
        Stage,
        StepPlatform,
        Mesh3d(meshes.add(Cuboid::new(1.5, STEP_HEIGHT, STEP_END - STEP_START))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.23, 0.37, 0.43),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, STUDIO_FLOOR_Y + STEP_HEIGHT * 0.5,
            (STEP_START + STEP_END) * 0.5),
        Visibility::Hidden,
    ));
    for index in -4..4 {
        let base = index as f32 * GRID_SPACING;
        let major = index % 2 == 0;
        let material = materials.add(StandardMaterial {
            base_color: if major { Color::srgb(0.32, 0.53, 0.57) } else { Color::srgb(0.19, 0.34, 0.38) },
            perceptual_roughness: 0.92,
            ..default()
        });
        let width = if major { 0.018 } else { 0.008 };
        for axis in [MarkAxis::X, MarkAxis::Z] {
            let size = match axis {
                MarkAxis::X => Vec3::new(width, 0.004, GRID_PERIOD),
                MarkAxis::Z => Vec3::new(GRID_PERIOD, 0.004, width),
            };
            commands.spawn((
                Stage,
                GroundMark { axis, base },
                Mesh3d(meshes.add(Cuboid::from_size(size))),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(
                    if matches!(axis, MarkAxis::X) { base } else { 0.0 },
                    STUDIO_FLOOR_Y + 0.003,
                    if matches!(axis, MarkAxis::Z) { base } else { 0.0 },
                ),
            ));
        }
    }
    for (position, intensity) in [
        (Vec3::new(2.5, STUDIO_FLOOR_Y + 3.5, 3.5), 1900.0),
        (Vec3::new(-3.0, STUDIO_FLOOR_Y + 2.6, -1.0), 1100.0),
    ] {
        commands.spawn((
            Stage,
            PointLight {
                intensity,
                range: 9.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(position),
        ));
    }
    commands
        .spawn((
            Root,
            GlobalZIndex(1001),
            Node {
                position_type: PositionType::Absolute,
                left: px(20.0),
                top: px(18.0),
                width: px(245.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                padding: UiRect::all(px(15.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.05, 0.94)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("CHARACTER STUDIO"),
                TextFont {
                    font_size: FontSize::Px(19.0),
                    ..default()
                },
                TextColor(Color::srgb(0.42, 0.92, 0.95)),
            ));
            label(panel, "LOOPING ANIMATIONS");
            label(panel, "MOVING FLOOR / 0.5 M GRID");
            panel.spawn((CharacterName, Text::new(""), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.49, 0.82, 0.85))));
            for clip in Clip::ALL {
                button(panel, Action::Clip(clip), clip.title());
            }
            label(panel, "CAMERA");
            button(
                panel,
                Action::Angle(ViewAngle::ThreeQuarter),
                "THREE QUARTER",
            );
            button(panel, Action::Angle(ViewAngle::Front), "FRONT");
            button(panel, Action::Angle(ViewAngle::Side), "SIDE");
            button(panel, Action::Speed, "CYCLE SPEED");
            button(panel, Action::Pause, "PAUSE / PLAY");
            button(panel, Action::Step, "STEP +1 [RIGHT]");
            button(panel, Action::StepBack, "STEP -1 [LEFT]");
            button(panel, Action::Reset, "RESET LOOP");
            button(panel, Action::Back, "BACK TO MENU");
            panel.spawn((
                Status,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.66, 0.87, 0.88)),
            ));
        });
    commands
        .spawn((
            Root,
            SwordMetrics,
            GlobalZIndex(1002),
            Node {
                display: Display::Flex,
                position_type: PositionType::Absolute,
                right: px(20.0),
                top: px(18.0),
                width: px(315.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                padding: UiRect::all(px(14.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.05, 0.94)),
        ))
        .with_children(|panel| {
            panel.spawn((
                SwordMetricsTitle,
                Text::new(""),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                TextColor(Color::srgb(0.42, 0.92, 0.95)),
            ));
            panel.spawn((
                SwordAngleText,
                Text::new(""),
                TextFont { font_size: FontSize::Px(19.0), ..default() },
                TextColor(Color::srgb(0.85, 0.95, 0.94)),
            ));
            panel.spawn((
                SwordPhaseText,
                Text::new(""),
                TextFont { font_size: FontSize::Px(13.0), ..default() },
                TextColor(Color::srgb(0.66, 0.87, 0.88)),
            ));
        });
    commands.spawn((
        Root,
        CharacterPicker,
        GlobalZIndex(1001),
        Node {
            position_type: PositionType::Absolute,
            right: px(20.0),
            top: px(145.0),
            width: px(245.0),
            max_height: percent(70.0),
            overflow: Overflow::scroll_y(),
            flex_direction: FlexDirection::Column,
            row_gap: px(5.0),
            padding: UiRect::all(px(12.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.015, 0.035, 0.05, 0.94)),
    )).with_children(|panel| {
        label(panel, "CHARACTERS / SELECT DIRECTLY");
        for (index, character) in library.characters.iter().enumerate() {
            button(panel, Action::Character(index), &character.name);
        }
    });
}

pub fn input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut viewer: ResMut<Viewer>,
    mut library: ResMut<SceneLibrary>,
    mut menu: ResMut<LaunchMenu>,
    roots: Query<Entity, With<Root>>,
    stages: Query<Entity, With<Stage>>,
    mut actions: Query<(&Action, Ref<Interaction>, &mut BackgroundColor, &mut Node)>,
    mut character_names: Query<&mut Text, With<CharacterName>>,
) {
    if !viewer.active {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        viewer.paused = true;
        viewer.step_frame();
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        viewer.paused = true;
        viewer.step_back_frame();
    }
    for (action, interaction, _, _) in &mut actions {
        if !interaction.is_changed() || *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            Action::Character(index) => {
                if let Some(character) = library.characters.get(index) {
                    if !viewer.clip.supports(character) {
                        viewer.clip = if Clip::Run.supports(character) { Clip::Run } else { Clip::Idle };
                    }
                    viewer.character = index;
                    viewer.reset_clock();
                }
            }
            Action::Clip(clip) => {
                if library.characters.get(viewer.character).is_some_and(|character| clip.supports(character)) {
                    viewer.clip = clip;
                    viewer.reset_clock();
                }
            }
            Action::Angle(angle) => viewer.angle = angle,
            Action::Speed => {
                viewer.speed = if viewer.speed < 0.75 {
                    1.0
                } else if viewer.speed < 1.25 {
                    1.5
                } else {
                    0.5
                }
            }
            Action::Pause => viewer.paused = !viewer.paused,
            Action::Step => {
                viewer.paused = true;
                viewer.step_frame();
            }
            Action::StepBack => {
                viewer.paused = true;
                viewer.step_back_frame();
            }
            Action::Reset => {
                viewer.paused = true;
                viewer.reset_clock();
            }
            Action::Back => {
                viewer.active = false;
                menu.open = true;
                // The preview only selected a character asset pack. Force the
                // next play session to build its actual scene, even when the
                // menu selects the same pack that the preview used.
                library.current = usize::MAX;
                library.requested = None;
                for root in &roots {
                    commands.entity(root).despawn();
                }
                for stage in &stages {
                    commands.entity(stage).despawn();
                }
                return;
            }
        }
    }
    let character = library.characters.get(viewer.character);
    for mut name in &mut character_names {
        name.0 = character.map_or_else(|| "NO CHARACTER".to_string(), |entry| format!("CHARACTER / {}", entry.name));
    }
    for (action, interaction, mut background, mut node) in &mut actions {
        if let Action::Clip(clip) = action {
            node.display = if character.is_some_and(|entry| clip.supports(entry)) { Display::Flex } else { Display::None };
        }
        let selected = match *action {
            Action::Character(index) => viewer.character == index,
            Action::Clip(clip) => viewer.clip == clip,
            Action::Angle(angle) => viewer.angle == angle,
            _ => false,
        };
        background.0 = if selected {
            Color::srgba(0.09, 0.38, 0.43, 0.98)
        } else if *interaction == Interaction::Hovered {
            Color::srgba(0.12, 0.25, 0.29, 0.98)
        } else {
            Color::srgba(0.07, 0.16, 0.20, 0.92)
        };
    }
}

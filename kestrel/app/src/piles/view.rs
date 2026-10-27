//! Shared geometry and one mineral chunk per logical unit.
use super::*;

fn chunk(
    parent: &mut ChildSpawnerCommands,
    assets: &LabAssets,
    material: &Handle<StandardMaterial>,
    position: Vec3,
    scale: Vec3,
) {
    parent.spawn((
        Mesh3d(assets.cube.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(position).with_scale(scale),
    ));
}
fn unit(
    parent: &mut ChildSpawnerCommands,
    assets: &LabAssets,
    material: &Handle<StandardMaterial>,
    position: Vec3,
    scale: Vec3,
) {
    parent.spawn((
        MineralUnit,
        Mesh3d(assets.cube.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(position).with_scale(scale),
    ));
}

fn label(commands: &mut Commands, kind: CaptionKind, anchor: Vec3, text: &str) -> Entity {
    commands
        .spawn((
            LabEntity,
            Caption { kind, anchor },
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgb(0.7, 0.92, 1.0)),
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                width: px(220.0),
                ..default()
            },
        ))
        .id()
}

/// Builds a world's lab view from its scene's pile layout, if it has one:
/// dispensers, the collector, the cargo display and the panel.
pub(super) fn build(
    In(mut lab): In<PileLab>,
    mut commands: Commands,
    scene: Res<crate::scene_view::SceneView>,
    shared: Option<Res<LabAssets>>,
    player: Single<Entity, With<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) -> PileLab {
    lab.layout = scene.scene().piles.clone();
    lab.stock = lab.layout.as_ref().map(Stock::new);
    let Some(layout) = lab.layout.clone() else {
        return lab;
    };
    if let Some(shared) = shared {
        lab.assets = Some(shared.clone());
    } else {
        let mineral = MINERALS.map(|m| {
            let c = mineral_color(m);
            materials.add(StandardMaterial {
                base_color: Color::srgb(c[0], c[1], c[2]),
                emissive: LinearRgba::rgb(c[0] * 0.4, c[1] * 0.4, c[2] * 0.4),
                metallic: 0.3,
                perceptual_roughness: 0.5,
                ..default()
            })
        });
        lab.assets = Some(LabAssets {
            cube: meshes.add(Cuboid::default()),
            mineral,
            dark: materials.add(StandardMaterial {
                base_color: Color::srgb(0.07, 0.12, 0.18),
                metallic: 0.7,
                ..default()
            }),
            trim: materials.add(StandardMaterial {
                base_color: Color::srgb(0.3, 0.5, 0.56),
                metallic: 0.6,
                ..default()
            }),
        });
        commands.insert_resource(lab.assets.clone().unwrap());
    }
    let assets = lab.assets.as_ref().unwrap().clone();
    for (index, dispenser) in layout.dispensers.iter().enumerate() {
        let position = Vec3::from_array(dispenser.position);
        let ore = &assets.mineral[mineral_index(dispenser.material)];
        commands
            .spawn((
                LabEntity,
                Transform::from_translation(position),
                Visibility::default(),
                Obstacle(Disc {
                    center: position,
                    radius: crate::obstacles::DISPENSER_RADIUS,
                }),
            ))
            .with_children(|p| {
                chunk(
                    p,
                    &assets,
                    &assets.dark,
                    Vec3::new(0.0, 0.65, 0.0),
                    Vec3::new(1.1, 1.3, 1.1),
                );
                chunk(
                    p,
                    &assets,
                    &assets.trim,
                    Vec3::new(0.0, 0.12, 0.0),
                    Vec3::new(1.5, 0.24, 1.5),
                );
                chunk(
                    p,
                    &assets,
                    ore,
                    Vec3::new(0.0, 1.5, 0.0),
                    Vec3::new(0.7, 0.55, 0.7),
                );
                chunk(
                    p,
                    &assets,
                    ore,
                    Vec3::new(0.0, 0.95, 0.58),
                    Vec3::new(0.7, 0.25, 0.1),
                );
            });
        let caption = label(
            &mut commands,
            CaptionKind::Dispenser(index),
            position + Vec3::Y * 3.3,
            "",
        );
        commands.entity(caption).insert((
            // Bevy 0.20 made the old `Button` marker private; spell out
            // what it required so `Interaction` keeps updating as before.
            Interaction::None,
            bevy::ui::FocusPolicy::Block,
            SourceButton(index),
            BackgroundColor(Color::srgba(0.02, 0.06, 0.10, 0.94)),
            BorderColor::all(Color::srgb(0.15, 0.55, 0.65)),
            Node {
                position_type: PositionType::Absolute,
                width: px(220.0),
                padding: UiRect::all(px(8.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
        ));
    }
    if let Some(at) = layout.collector {
        spawn_collector(&mut commands, &assets, Vec3::from_array(at));
    }
    let mine = layout.dispensers.is_empty();
    // An empty destination bay suggests a place to start; mineral can be dropped elsewhere too.
    if !mine {
        commands
            .spawn((
                LabEntity,
                Transform::from_xyz(4.0, 0.025, 4.5),
                Visibility::default(),
            ))
            .with_children(|p| {
                for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    chunk(
                        p,
                        &assets,
                        &assets.trim,
                        Vec3::new(x, 0.0, z),
                        Vec3::new(0.35, 0.04, 0.35),
                    );
                }
            });
    }
    commands.spawn((
        LabEntity,
        CarryVisual {
            agent: None,
            units: u32::MAX,
            minerals: Vec::new(),
            hidden: 0,
        },
        Transform::default(),
        Visibility::default(),
        ChildOf(*player),
    ));
    label(&mut commands, CaptionKind::Cargo, Vec3::ZERO, "");
    // The panel covers the captions of whatever lies behind it.
    commands.spawn((LabEntity,LabPanel,ZIndex(1),Interaction::None,Node { position_type: PositionType::Absolute, left: px(36.0), top: px(126.0), width: px(350.0), padding: UiRect::all(px(16.0)), flex_direction: FlexDirection::Column, row_gap: px(10.0), ..default() },BackgroundColor(Color::srgba(0.015,0.035,0.055,0.93))))
        .with_children(|p| {
            p.spawn((Text::new("MINERAL LOGISTICS"),TextFont { font_size: FontSize::Px(18.0), ..default() },TextColor(Color::srgb(0.25,0.9,1.0))));
            p.spawn((LabStatus,Text::new(""),TextFont { font_size: FontSize::Px(13.0), ..default() },TextColor(Color::srgb(0.7,0.84,0.9))));
            p.spawn(Node { column_gap: px(4.0), ..default() }).with_children(|row| {
                for index in 0..layout.capacity { row.spawn((CapacityTick(index),Node { width: px(21.0), height: px(12.0), ..default() },BackgroundColor(Color::srgb(0.1,0.18,0.22)))); }
            });
            let help = if mine {
                "F: dig the tile underfoot (2 s)\nOre flies out onto a pile beside the pit\nLMB pile: walk over + collect\nCarrying? LMB collector: deliver\nSPACE: fly out of a pit\nWASD: move / cancel hauling\nSHIFT: run\nOne chunk = one unit"
            } else {
                "LMB dispenser: add 5 units\nLMB pile: walk over + collect\nCtrl+LMB pile: collect / top up\nCarrying? LMB ground / matching pile:\nwalk over + deposit\nWASD: move / cancel hauling\nSHIFT: run\nOne chunk = one unit"
            };
            p.spawn((Text::new(help),TextFont { font_size: FontSize::Px(12.0), ..default() },TextColor(Color::srgb(0.45,0.65,0.72))));
        });
    lab.notice = if mine {
        "Dig for ore (F), then carry it to the collector."
    } else {
        "Click a dispenser to add mineral, or collect an existing pile."
    }
    .into();
    lab
}

pub fn render(
    mut commands: Commands,
    lab: Res<PileLab>,
    mut piles: Query<(Entity, &mut PileVisual)>,
    mut carriers: Query<(Entity, &mut CarryVisual)>,
    captions: Query<(Entity, &Caption)>,
    airborne: Query<&transfer::Transit>,
    peers: Query<(Entity, &crate::peers::Peer)>,
) {
    let (Some(stock), Some(assets)) = (&lab.stock, &lab.assets) else {
        return;
    };
    let mut hidden_piles: std::collections::BTreeMap<PileId, std::collections::BTreeSet<u32>> =
        std::collections::BTreeMap::new();
    // A backpack's slots shift as minerals mix, so a slot is hidden only
    // while it holds the flying unit's mineral: once that unit landed.
    let mut hidden_cargo: std::collections::BTreeMap<u16, Vec<(u32, Mineral)>> =
        std::collections::BTreeMap::new();
    for transit in &airborne {
        match transit.destination {
            transfer::Destination::Cargo(agent, index) => {
                hidden_cargo.entry(agent).or_default().push((index, transit.material));
            }
            transfer::Destination::Pile(id, index) => {
                hidden_piles.entry(id).or_default().insert(index);
            }
            transfer::Destination::Collector(_) => {}
        }
    }
    let empty = std::collections::BTreeSet::new();
    let mut existing = std::collections::BTreeSet::new();
    for (entity, mut visual) in &mut piles {
        let Some(pile) = stock.piles.get(&visual.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        existing.insert(visual.id);
        let hidden = hidden_piles.get(&visual.id).unwrap_or(&empty);
        if visual.units == pile.units && visual.hidden == hidden.len() as u32 {
            continue;
        }
        visual.units = pile.units;
        visual.hidden = hidden.len() as u32;
        commands.entity(entity).despawn_children();
        commands.entity(entity).insert(Transform::from_translation(place(pile)));
        if pile.units > 0 {
            commands.entity(entity).insert(Obstacle(Disc {
                center: point(pile.position),
                radius: pile_radius(pile.units),
            }));
        } else {
            commands.entity(entity).remove::<Obstacle>();
        }
        commands
            .entity(entity)
            .with_children(|p| spawn_units(p, assets, pile.material, pile.units, hidden));
    }
    for (&id, pile) in &stock.piles {
        if existing.contains(&id) {
            continue;
        }
        let hidden = hidden_piles.get(&id).unwrap_or(&empty);
        let mut root = commands.spawn((
            LabEntity,
            PileVisual {
                id,
                units: pile.units,
                hidden: hidden.len() as u32,
            },
            Transform::from_translation(place(pile)),
            Visibility::default(),
        ));
        if pile.units > 0 {
            root.insert(Obstacle(Disc {
                center: point(pile.position),
                radius: pile_radius(pile.units),
            }));
        }
        root.with_children(|p| spawn_units(p, assets, pile.material, pile.units, hidden));
        label(
            &mut commands,
            CaptionKind::Pile(id),
            place(pile),
            "",
        );
    }
    for (entity, caption) in &captions {
        if let CaptionKind::Pile(id) = caption.kind {
            if !stock.piles.contains_key(&id) {
                commands.entity(entity).despawn();
            }
        }
    }
    // Other players carry backpacks too, on their own bodies.
    let carried: Vec<Option<u16>> = carriers.iter().map(|(_, v)| v.agent).collect();
    for (entity, peer) in &peers {
        if !carried.contains(&Some(peer.agent())) {
            commands.spawn((
                LabEntity,
                CarryVisual { agent: Some(peer.agent()), units: u32::MAX, minerals: Vec::new(), hidden: 0 },
                Transform::default(),
                Visibility::default(),
                ChildOf(entity),
            ));
        }
    }
    let nothing = crate::pile_model::Cargo::default();
    let no_slots = Vec::new();
    for (entity, mut visual) in &mut carriers {
        let cargo = match visual.agent {
            None => &stock.cargo,
            Some(agent) => lab.others.get(&agent).unwrap_or(&nothing),
        };
        let hidden = visual.agent.or(lab.me).and_then(|a| hidden_cargo.get(&a)).unwrap_or(&no_slots);
        if visual.units == cargo.units && visual.minerals == cargo.minerals && visual.hidden == hidden.len() as u32 {
            continue;
        }
        visual.units = cargo.units;
        visual.hidden = hidden.len() as u32;
        visual.minerals = cargo.minerals.clone();
        commands.entity(entity).despawn_children();
        commands.entity(entity).with_children(|p| {
            chunk(
                p,
                assets,
                &assets.dark,
                Vec3::new(0.0, 0.95, -0.28),
                Vec3::new(0.78, 0.65, 0.12),
            );
            for i in 0..cargo.units {
                let Some(material) = cargo.material_at(i) else { continue };
                if hidden.contains(&(i, material)) {
                    continue;
                }
                let position = transfer::backpack_slot(i);
                unit(
                    p,
                    assets,
                    &assets.mineral[mineral_index(material)],
                    position,
                    Vec3::new(0.23, 0.21, 0.23),
                );
            }
        });
    }
}
/// The collector: a hopper on legs over a squat bin, with a lamp for each
/// mineral it takes.
fn spawn_collector(commands: &mut Commands, assets: &LabAssets, position: Vec3) {
    commands
        .spawn((
            LabEntity,
            Transform::from_translation(position),
            Visibility::default(),
            Obstacle(Disc { center: position, radius: crate::obstacles::COLLECTOR_RADIUS }),
        ))
        .with_children(|p| {
            chunk(p, assets, &assets.trim, Vec3::new(0.0, 0.1, 0.0), Vec3::new(1.7, 0.2, 1.7));
            chunk(p, assets, &assets.dark, Vec3::new(0.0, 0.6, 0.0), Vec3::new(1.4, 0.8, 1.4));
            chunk(p, assets, &assets.dark, Vec3::new(0.0, 1.25, 0.0), Vec3::new(1.0, 0.5, 1.0));
            chunk(p, assets, &assets.trim, Vec3::new(0.0, 1.55, 0.0), Vec3::new(1.5, 0.12, 1.5));
            for (i, material) in assets.mineral.iter().enumerate() {
                chunk(p, assets, material, Vec3::new(-0.4 + i as f32 * 0.4, 0.75, 0.72), Vec3::new(0.24, 0.24, 0.06));
            }
        });
    label(commands, CaptionKind::Collector, position + Vec3::Y * 3.0, "");
}

fn spawn_units(
    parent: &mut ChildSpawnerCommands,
    assets: &LabAssets,
    material: Mineral,
    units: u32,
    hidden: &std::collections::BTreeSet<u32>,
) {
    if units == 0 {
        for sign in [-1.0, 1.0] {
            chunk(
                parent,
                assets,
                &assets.trim,
                Vec3::new(sign * 0.35, 0.035, 0.0),
                Vec3::new(0.06, 0.04, 0.7),
            );
            chunk(
                parent,
                assets,
                &assets.trim,
                Vec3::new(0.0, 0.035, sign * 0.35),
                Vec3::new(0.7, 0.04, 0.06),
            );
        }
    }
    for (index, position) in unit_positions().iter().take(units as usize).enumerate() {
        if hidden.contains(&(index as u32)) {
            continue;
        }
        unit(
            parent,
            assets,
            &assets.mineral[mineral_index(material)],
            Vec3::from_array(*position),
            Vec3::new(0.24, 0.21, 0.24),
        );
    }
}

pub fn update_display(
    lab: Res<PileLab>,
    player: Single<&Transform, With<Player>>,
    mut labels: Query<(&mut Caption, &mut Text), Without<LabStatus>>,
    mut status: Query<&mut Text, (With<LabStatus>, Without<Caption>)>,
    mut ticks: Query<(&CapacityTick, &mut BackgroundColor)>,
) {
    let Some(stock) = &lab.stock else {
        return;
    };
    for (mut caption, mut text) in &mut labels {
        let value = match caption.kind {
            CaptionKind::Dispenser(index) => format!(
                "{} DISPENSER\n[ CLICK: +5 UNITS ]",
                mineral_name(lab.layout.as_ref().unwrap().dispensers[index].material)
            ),
            CaptionKind::Pile(id) => {
                let Some(p) = stock.piles.get(&id) else {
                    continue;
                };
                let top = unit_positions()
                    .iter()
                    .take(p.units as usize)
                    .map(|p| p[1])
                    .fold(0.14, f32::max);
                caption.anchor = place(p) + Vec3::Y * (top + 0.65);
                format!("{}  {} / 96", mineral_name(p.material), p.units)
            }
            CaptionKind::Collector => {
                let Some(at) = lab.collector else {
                    continue;
                };
                caption.anchor = at + Vec3::Y * 3.8;
                let counts: Vec<String> = MINERALS
                    .iter()
                    .map(|&m| format!("{} {}", mineral_name(m), lab.collected.get(mineral_index(m)).copied().unwrap_or(0)))
                    .collect();
                format!("COLLECTOR\n{}", counts.join("\n"))
            }
            CaptionKind::Cargo => {
                caption.anchor = player.translation + Vec3::Y * 1.5;
                format!("CARRY {} / {}", stock.cargo.units, stock.cargo.capacity)
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    let material = if stock.cargo.minerals.is_empty() {
        "EMPTY".to_string()
    } else {
        stock.cargo.minerals.iter().map(|&(m, _)| mineral_name(m)).collect::<Vec<_>>().join(" + ")
    };
    for mut text in &mut status {
        let collected: u32 = lab.collected.iter().sum();
        let value = if lab.collector.is_some() {
            format!(
                "CARRY: {material}  {} / {}\nGROUND: {}   MINED: {}   COLLECTED: {collected}\n\n{}",
                stock.cargo.units,
                stock.cargo.capacity,
                stock.ground(),
                stock.created,
                lab.notice
            )
        } else {
            format!(
                "CARRY: {material}  {} / {}\nGROUND: {}   CREATED: {}\n\n{}",
                stock.cargo.units,
                stock.cargo.capacity,
                stock.ground(),
                stock.created,
                lab.notice
            )
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (tick, mut color) in &mut ticks {
        color.0 = if let Some(material) = stock.cargo.material_at(tick.0) {
            let c = mineral_color(material);
            Color::srgb(c[0], c[1], c[2])
        } else {
            Color::srgb(0.1, 0.18, 0.22)
        };
    }
}

pub fn captions(
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(&Caption, &mut Node, &mut Visibility)>,
) {
    for (caption, mut node, mut visibility) in &mut labels {
        if let Ok(position) = camera.0.world_to_viewport(camera.1, caption.anchor) {
            let offset = if matches!(caption.kind, CaptionKind::Cargo) {
                15.0
            } else {
                -110.0
            };
            node.left = px(position.x + offset);
            node.top = px(position.y);
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

use super::*;
fn app() -> App {
    let mut app = App::new();
    let mut library = SceneLibrary::new();
    library.current = 2;
    app.insert_resource(library)
        .init_resource::<PileLab>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Controls>()
        .init_resource::<crate::effects::Feedback>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Time>()
        .insert_resource(crate::sound::Sound::silent())
        .add_systems(
            Update,
            (
                setup_scene,
                crate::gameplay::move_player,
                arrive,
                update_transit,
                render,
            )
                .chain(),
        );
    app.world_mut()
        .spawn((Player, Transform::from_xyz(0.0, 0.0, 1.0)));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
    app.update();
    app
}
fn start(app: &mut App, action: Action) {
    let player = app
        .world_mut()
        .query_filtered::<&Transform, With<Player>>()
        .single(app.world())
        .unwrap()
        .translation;
    let discs: Vec<_> = app
        .world_mut()
        .query::<&Obstacle>()
        .iter(app.world())
        .map(|o| o.0)
        .collect();
    let mut lab = app.world_mut().remove_resource::<PileLab>().unwrap();
    begin_job(
        &mut lab,
        &mut app.world_mut().resource_mut::<Controls>(),
        player,
        action,
        &discs,
    )
    .unwrap();
    app.world_mut().insert_resource(lab);
}
fn finish(app: &mut App) {
    for _ in 0..1800 {
        app.update();
        if app.world().resource::<PileLab>().job.is_none() {
            return;
        }
    }
    panic!("job did not finish");
}
fn assert_visual_quantity(app: &mut App) {
    let units = app
        .world()
        .resource::<PileLab>()
        .stock
        .as_ref()
        .unwrap()
        .total();
    let chunks = app
        .world_mut()
        .query::<&MineralUnit>()
        .iter(app.world())
        .count();
    assert_eq!(chunks as u64, units);
}
#[test]
fn character_travels_before_transferring_and_views_match_every_unit() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<PileLab>()
        .stock
        .as_mut()
        .unwrap()
        .add(0, 20)
        .unwrap();
    app.update();
    assert_visual_quantity(&mut app);
    start(&mut app, Action::Pickup(0));
    app.update();
    assert_eq!(
        app.world()
            .resource::<PileLab>()
            .stock
            .as_ref()
            .unwrap()
            .cargo
            .units,
        0
    );
    finish(&mut app);
    let stock = app.world().resource::<PileLab>().stock.as_ref().unwrap();
    assert_eq!(stock.cargo.units, 12);
    assert_eq!(stock.piles[&0].units, 8);
    assert_visual_quantity(&mut app);
    start(&mut app, Action::Drop(Vec3::new(4.0, 0.0, 4.5)));
    finish(&mut app);
    let stock = app.world().resource::<PileLab>().stock.as_ref().unwrap();
    assert_eq!(stock.cargo.units, 0);
    assert_eq!(stock.piles[&5].units, 12);
    assert_eq!(stock.total(), stock.created);
    assert_visual_quantity(&mut app);
    // Deposit back into the same pile, rather than making overlapping piles.
    start(&mut app, Action::Pickup(0));
    finish(&mut app);
    start(&mut app, Action::Deposit(5));
    finish(&mut app);
    assert_eq!(
        app.world()
            .resource::<PileLab>()
            .stock
            .as_ref()
            .unwrap()
            .piles[&5]
            .units,
        20
    );
    assert_visual_quantity(&mut app);
}
#[test]
fn scene_changes_clear_jobs_cargo_entities_and_reuse_asset_handles() {
    let mut app = app();
    let baseline = app.world().resource::<Assets<StandardMaterial>>().len();
    app.world_mut()
        .resource_mut::<PileLab>()
        .stock
        .as_mut()
        .unwrap()
        .add(0, 15)
        .unwrap();
    start(&mut app, Action::Pickup(0));
    finish(&mut app);
    start(&mut app, Action::Drop(Vec3::new(4.0, 0.0, 4.5)));
    for scene in [0, 2, 1, 2] {
        app.world_mut().resource_mut::<SceneLibrary>().current = scene;
        app.update();
        let lab = app.world().resource::<PileLab>();
        assert!(lab.job.is_none());
        if scene == 2 {
            assert_eq!(lab.stock.as_ref().unwrap().cargo.units, 0);
        } else {
            assert!(lab.stock.is_none());
        }
        if scene != 2 {
            assert_eq!(
                app.world_mut()
                    .query::<&LabEntity>()
                    .iter(app.world())
                    .count(),
                0
            );
        }
        assert_eq!(
            app.world().resource::<Assets<StandardMaterial>>().len(),
            baseline
        );
    }
    assert_visual_quantity(&mut app);
}
#[test]
fn inaccessible_drop_is_rejected_without_removing_cargo() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<PileLab>()
        .stock
        .as_mut()
        .unwrap()
        .pickup(2)
        .unwrap();
    let mut lab = app.world_mut().remove_resource::<PileLab>().unwrap();
    let total = lab.stock.as_ref().unwrap().total();
    let result = begin_job(
        &mut lab,
        &mut app.world_mut().resource_mut::<Controls>(),
        Vec3::ZERO,
        Action::Drop(Vec3::new(9.0, 0.0, 0.0)),
        &[],
    );
    assert!(result.is_err());
    assert!(lab.job.is_none());
    assert_eq!(lab.stock.as_ref().unwrap().cargo.units, 8);
    assert_eq!(lab.stock.as_ref().unwrap().total(), total);
}

#[test]
fn dispenser_button_and_manual_cancel_use_real_input_system() {
    let mut app = app();
    app.init_resource::<ButtonInput<MouseButton>>();
    app.world_mut()
        .spawn((Camera::default(), GlobalTransform::default()));
    app.world_mut().spawn(Window::default());
    app.add_systems(PreUpdate, input);
    let button = app
        .world_mut()
        .query::<(Entity, &SourceButton)>()
        .iter(app.world())
        .find(|(_, b)| b.0 == 0)
        .unwrap()
        .0;
    app.world_mut()
        .entity_mut(button)
        .insert(Interaction::Pressed);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(
        app.world()
            .resource::<PileLab>()
            .stock
            .as_ref()
            .unwrap()
            .piles[&0]
            .units,
        5
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    start(&mut app, Action::Pickup(0));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    let lab = app.world().resource::<PileLab>();
    assert!(lab.job.is_none());
    assert_eq!(lab.stock.as_ref().unwrap().cargo.units, 0);
    assert_eq!(lab.stock.as_ref().unwrap().piles[&0].units, 5);
}

//! Repeatable end-to-end hauling capture.
use super::*;

#[derive(Default)]
pub struct HaulPreview {
    configured: bool,
    enabled: bool,
    elapsed: f32,
    stage_age: f32,
    stage: u32,
    directory: Option<String>,
}
/// Repeatable real navigation/pickup/drop QA, using the same jobs as mouse input.
pub fn showcase(
    mut commands: Commands,
    time: Res<Time>,
    mut preview: Local<HaulPreview>,
    mut lab: ResMut<PileLab>,
    mut controls: ResMut<Controls>,
    player: Single<&Transform, With<Player>>,
    obstacles: Query<&Obstacle>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    if !preview.configured {
        preview.configured = true;
        let args: Vec<_> = std::env::args().collect();
        preview.enabled = args.iter().any(|arg| arg == "--haul-showcase");
        preview.directory = args
            .windows(2)
            .find(|a| a[0] == "--capture-dir")
            .map(|a| a[1].clone());
        if preview.enabled {
            if let Some(dir) = &preview.directory {
                std::fs::create_dir_all(dir).expect("capture directory");
            }
        }
    }
    if !preview.enabled {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    preview.elapsed += dt;
    preview.stage_age += dt;
    if preview.elapsed > 35.0 {
        panic!("Hauling showcase failed to finish: {}", lab.notice);
    }
    if lab.stock.is_none() {
        return;
    }
    let discs: Vec<_> = obstacles.iter().map(|o| o.0).collect();
    let capture = |commands: &mut Commands, name: &str, directory: &Option<String>| {
        if let Some(dir) = directory {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(format!("{dir}/{name}.png")));
        }
    };
    match preview.stage {
        0 if preview.stage_age > 3.0 => {
            lab.stock.as_mut().unwrap().add(0, 20).unwrap();
            preview.stage = 1;
            preview.stage_age = 0.0;
        }
        1 if preview.stage_age > 1.0 => {
            capture(&mut commands, "overview", &preview.directory);
            begin_job(
                &mut lab,
                &mut controls,
                player.translation,
                Action::Pickup(0),
                &discs,
            )
            .expect("reachable source");
            preview.stage = 2;
            preview.stage_age = 0.0;
        }
        2 if lab.job.is_none() && lab.stock.as_ref().unwrap().cargo.units == 12 => {
            capture(&mut commands, "carrying", &preview.directory);
            preview.stage = 3;
            preview.stage_age = 0.0;
        }
        3 if preview.stage_age > 1.0 => {
            begin_job(
                &mut lab,
                &mut controls,
                player.translation,
                Action::Drop(Vec3::new(4.0, 0.0, 4.5)),
                &discs,
            )
            .expect("reachable drop");
            preview.stage = 4;
            preview.stage_age = 0.0;
        }
        4 if lab.job.is_none() && lab.stock.as_ref().unwrap().cargo.units == 0 => {
            let stock = lab.stock.as_ref().unwrap();
            assert_eq!(stock.piles[&5].units, 12);
            assert_eq!(stock.total(), stock.created);
            capture(&mut commands, "delivered", &preview.directory);
            info!(
                "Haul showcase complete: 12 units delivered; {} conserved units",
                stock.total()
            );
            preview.stage = 5;
            preview.stage_age = 0.0;
        }
        5 if preview.stage_age > 1.5 => {
            exit.write(bevy::app::AppExit::Success);
        }
        _ => {}
    }
}

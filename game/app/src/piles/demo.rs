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
/// Repeatable real navigation, pickup and drop QA, through the same commands as mouse input.
pub fn showcase(
    mut commands: Commands,
    time: Res<Time>,
    mut preview: Local<HaulPreview>,
    lab: Res<PileLab>,
    mut service: ResMut<SessionService>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    if !preview.configured {
        preview.configured = true;
        let args: Vec<_> = std::env::args().collect();
        preview.enabled = args.iter().any(|arg| arg == "--haul-showcase");
        preview.directory = args.windows(2).find(|a| a[0] == "--capture-dir").map(|a| a[1].clone());
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
    let Some(stock) = &lab.stock else {
        return;
    };
    let capture = |commands: &mut Commands, name: &str, directory: &Option<String>| {
        if let Some(dir) = directory {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(format!("{dir}/{name}.png")));
        }
    };
    let idle = lab.job.is_none() && service.caught_up();
    match preview.stage {
        0 if preview.stage_age > 3.0 => {
            service.command(sim::CommandKind::Dispense { dispenser: 0, units: 20 });
            preview.stage = 1;
            preview.stage_age = 0.0;
        }
        1 if preview.stage_age > 1.0 => {
            capture(&mut commands, "overview", &preview.directory);
            haul(&mut service, PileAction::Pickup(0), 0.18, None);
            preview.stage = 2;
            preview.stage_age = 0.0;
        }
        2 if idle && stock.cargo.units == 12 => {
            capture(&mut commands, "carrying", &preview.directory);
            preview.stage = 3;
            preview.stage_age = 0.0;
        }
        3 if preview.stage_age > 1.0 => {
            let drop = service.to_sim(Vec3::new(4.0, 0.0, 4.5));
            haul(&mut service, PileAction::Drop(drop), 0.18, None);
            preview.stage = 4;
            preview.stage_age = 0.0;
        }
        4 if idle && stock.cargo.units == 0 => {
            assert_eq!(stock.piles[&5].units, 12);
            assert_eq!(stock.total(), stock.created);
            capture(&mut commands, "delivered", &preview.directory);
            info!("Haul showcase complete: 12 units delivered; {} conserved units", stock.total());
            preview.stage = 5;
            preview.stage_age = 0.0;
        }
        5 if preview.stage_age > 1.5 => {
            exit.write(bevy::app::AppExit::Success);
        }
        _ => {}
    }
}

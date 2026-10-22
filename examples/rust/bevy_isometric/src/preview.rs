//! Repeatable visual QA: `--showcase --capture-dir /tmp/kestrel`.
use crate::{
    effects::{CameraRig, Feedback},
    gameplay::{Controls, Player},
};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
};

#[derive(Default)]
pub struct Preview {
    configured: bool,
    enabled: bool,
    directory: Option<String>,
    elapsed: f32,
    started: bool,
    overview: bool,
    impact: bool,
    finished: bool,
}

pub fn playback(
    time: Res<Time>,
    mut preview: Local<Preview>,
    mut controls: ResMut<Controls>,
    mut player: Single<&mut Transform, With<Player>>,
    mut camera: Single<&mut CameraRig>,
) {
    if !preview.configured {
        preview.configured = true;
        preview.enabled = std::env::args().any(|a| a == "--showcase");
    }
    if !preview.enabled {
        return;
    }
    preview.elapsed += time.delta_secs().min(0.1);
    // Wait for shader compilation before the scripted approach and cut.
    if preview.elapsed > 3.0 && !preview.started {
        preview.started = true;
        player.translation = Vec3::new(0.1, 0.0, 2.5);
        controls.route = vec![Vec3::new(0.35, 0.0, 1.55)];
    }
    if preview.started && preview.elapsed > 5.0 && !preview.finished {
        preview.finished = true;
        camera.zoom = 17.0;
        let aim = Vec3::new(1.6, 0.0, -0.1) - player.translation;
        controls.strike(aim);
    }
}

pub fn capture(
    mut commands: Commands,
    time: Res<Time>,
    controls: Res<Controls>,
    feedback: Res<Feedback>,
    mut preview: Local<Preview>,
    mut exit: MessageWriter<AppExit>,
) {
    if !preview.configured {
        preview.configured = true;
        let args: Vec<_> = std::env::args().collect();
        preview.enabled = args.iter().any(|a| a == "--haul-showcase");
        preview.directory = args
            .windows(2)
            .find(|pair| pair[0] == "--capture-dir")
            .map(|pair| pair[1].clone());
        if let Some(dir) = &preview.directory {
            std::fs::create_dir_all(dir).expect("create capture directory");
        }
    }
    if preview.enabled {
        return;
    }
    let Some(directory) = preview.directory.clone() else {
        return;
    };
    preview.elapsed += time.delta_secs().min(0.1);
    let name = if preview.elapsed > 4.5 && !preview.overview {
        preview.overview = true;
        Some("overview.png")
    } else if controls.impacts > 0 && feedback.hit_stop <= 0.0 && !preview.impact {
        preview.impact = true;
        Some("impact.png")
    } else {
        None
    };
    if let Some(name) = name {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!("{directory}/{name}")));
    }
    if preview.elapsed > 8.0 {
        exit.write(AppExit::Success);
    }
}

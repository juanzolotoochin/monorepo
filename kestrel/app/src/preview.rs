//! Repeatable visual QA: `--showcase`, `--motion-showcase`, or `--jump-combo-showcase`.
use crate::{
    character::Player,
    effects::{CameraRig, Feedback},
    gameplay::Controls,
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
    motion: bool,
    jump_combo: bool,
    directory: Option<String>,
    elapsed: f32,
    started: bool,
    strikes: u8,
    jump_frame: bool,
    stroke_frames: [bool; 3],
    turned: bool,
    run_frame: bool,
    turn_frame: bool,
    overview: bool,
    action: bool,
    impact: bool,
    finished: bool,
}

pub fn playback(
    time: Res<Time>,
    mut preview: Local<Preview>,
    mut controls: ResMut<Controls>,
    mut service: ResMut<crate::lockstep::SessionService>,
    mut control: ResMut<crate::lockstep::PlayerControl>,
    scene: Res<crate::scene_view::SceneView>,
    mut player: Single<&mut Transform, With<Player>>,
    mut camera: Single<&mut CameraRig>,
) {
    if !preview.configured {
        preview.configured = true;
        preview.motion = std::env::args().any(|a| a == "--motion-showcase");
        preview.jump_combo = std::env::args().any(|a| a == "--jump-combo-showcase");
        preview.enabled =
            preview.motion || preview.jump_combo || std::env::args().any(|a| a == "--showcase");
    }
    if !preview.enabled {
        return;
    }
    preview.elapsed += time.delta_secs().min(0.1);
    // Wait for shader compilation before the scripted approach and cut.
    if preview.elapsed > 3.0 && !preview.started {
        preview.started = true;
        if preview.motion {
            camera.zoom = 3.0;
            control.go_to(&mut service, Vec3::new(0.0, 0.0, -3.0), true);
        } else if preview.jump_combo {
            camera.zoom = 3.0;
            if let Some(agent) = service.player() {
                service.command(sim::CommandKind::Jump { agent });
            }
        } else {
            player.translation = Vec3::new(0.1, 0.0, 2.5);
            controls.route = vec![Vec3::new(0.35, 0.0, 1.55)];
        }
    }
    if preview.motion && preview.elapsed > 3.7 && !preview.turned {
        preview.turned = true;
        control.go_to(&mut service, Vec3::new(3.0, 0.0, -1.0), true);
    }
    if preview.motion {
        return;
    }
    if preview.jump_combo {
        let times = [4.12, 4.78, 5.44];
        if let Some(&when) = times.get(preview.strikes as usize) {
            if preview.elapsed >= when {
                preview.strikes += 1;
                control.strike(&mut service, &mut controls, &scene.pack, Vec3::new(0.9, 0.0, -1.0));
            }
        }
        return;
    }
    if preview.started && preview.elapsed > 5.0 && !preview.finished {
        preview.finished = true;
        camera.zoom = 3.0;
        let aim = Vec3::new(1.6, 0.0, -0.1) - player.translation;
        controls.strike(aim);
    }
}

/// Keep the moving character in frame for the close action capture.
pub fn frame_character(
    controls: Res<Controls>,
    player: Single<&Transform, With<Player>>,
    mut camera: Single<&mut CameraRig>,
    mut showcase: Local<Option<(bool, bool, bool)>>,
) {
    let (action, motion, jump_combo) = *showcase.get_or_insert_with(|| {
        let args: Vec<_> = std::env::args().collect();
        (
            args.iter().any(|a| a == "--showcase"),
            args.iter().any(|a| a == "--motion-showcase"),
            args.iter().any(|a| a == "--jump-combo-showcase"),
        )
    });
    if !motion && !jump_combo && (!action || controls.attack.is_none()) {
        return;
    }
    let target = player.translation + Vec3::Y * 0.8;
    camera.target = target;
    camera.base = target + Vec3::new(20.0, 20.0, 20.0);
}

pub fn capture(
    mut commands: Commands,
    time: Res<Time>,
    controls: Res<Controls>,
    flight: Res<crate::jetpack::Flight>,
    player: Single<&Transform, With<Player>>,
    feedback: Res<Feedback>,
    mut preview: Local<Preview>,
    mut exit: MessageWriter<AppExit>,
) {
    if !preview.configured {
        preview.configured = true;
        let args: Vec<_> = std::env::args().collect();
        preview.enabled = args.iter().any(|a| a == "--haul-showcase");
        preview.motion = args.iter().any(|a| a == "--motion-showcase");
        preview.jump_combo = args.iter().any(|a| a == "--jump-combo-showcase");
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
    let name = if preview.jump_combo
        && !preview.jump_frame
        && matches!(
            flight.jump,
            sim::settlement::avatar::JumpPhase::Airborne { .. }
        )
        && player.translation.y - flight.support > 0.9
    {
        preview.jump_frame = true;
        Some("jump.png")
    } else if preview.jump_combo
        && controls
            .attack
            .is_some_and(|a| a.age > a.timing().0 + 0.065 && a.age < a.timing().1)
    {
        let index = match controls.attack.unwrap().stroke {
            sim::settlement::avatar::SwordStroke::Forehand => 0,
            sim::settlement::avatar::SwordStroke::Backhand => 1,
            sim::settlement::avatar::SwordStroke::Overhead => 2,
        };
        if preview.stroke_frames[index] {
            None
        } else {
            preview.stroke_frames[index] = true;
            Some(["forehand.png", "backhand.png", "overhead.png"][index])
        }
    } else if preview.jump_combo {
        None
    } else if preview.motion && preview.elapsed > 3.45 && !preview.run_frame {
        preview.run_frame = true;
        Some("run.png")
    } else if preview.motion && preview.elapsed > 3.95 && !preview.turn_frame {
        preview.turn_frame = true;
        Some("turn.png")
    } else if preview.motion {
        None
    } else if preview.elapsed > 4.5 && !preview.overview {
        preview.overview = true;
        Some("overview.png")
    } else if controls.attack.is_some_and(|attack| {
        attack.age >= attack.timing().0 + 0.065 && attack.age < attack.timing().1
    }) && !preview.action
    {
        preview.action = true;
        Some("action.png")
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
    if preview.elapsed
        > if preview.motion {
            5.5
        } else if preview.jump_combo {
            7.0
        } else {
            8.0
        }
    {
        exit.write(AppExit::Success);
    }
}

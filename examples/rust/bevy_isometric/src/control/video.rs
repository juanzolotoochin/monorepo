//! Offscreen rendering: no desktop or synthetic mouse events required.
use super::*;
use bevy::{
    camera::RenderTarget,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use std::io::Write;
#[derive(Resource, Default)]
pub struct Video {
    target: Option<Handle<Image>>,
    submitted: usize,
    completed: usize,
    trace: Option<std::fs::File>,
}
pub fn setup_render(
    mut commands: Commands,
    options: Res<Options>,
    camera: Single<Entity, With<Camera3d>>,
    mut images: ResMut<Assets<Image>>,
) {
    if options.frames_dir.is_none() {
        return;
    }
    let script = options.script.as_ref().unwrap();
    std::fs::create_dir_all(options.frames_dir.as_ref().unwrap()).expect("frame directory");
    let mut image = Image::new_target_texture(
        script.width,
        script.height,
        TextureFormat::Bgra8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = images.add(image);
    commands.entity(*camera).insert((
        RenderTarget::Image(target.clone().into()),
        IsDefaultUiCamera,
    ));
    commands.insert_resource(Video {
        target: Some(target),
        trace: options
            .trace
            .as_ref()
            .map(|p| std::fs::File::create(p).expect("frame trace")),
        ..default()
    });
}
pub fn capture_frame(
    mut commands: Commands,
    mut video: ResMut<Video>,
    mut hub: ResMut<Hub>,
    sound: Res<crate::sound::Sound>,
    mut telemetry: ResMut<crate::telemetry::Telemetry>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(target) = video.target.clone() else {
        return;
    };
    if hub.tick <= hub.warmup || hub.failed.is_some() {
        return;
    }
    let tail = u64::from(hub.options.fps());
    if hub.finished.is_some_and(|tick| hub.tick >= tick + tail) {
        if video.completed == video.submitted {
            if let Some(path) = &hub.options.audio {
                sound
                    .write_wav(path, hub.options.fps(), hub.warmup + 1, video.completed)
                    .expect("write demo audio");
            }
            hub.audio_events=sound.recorded_events().into_iter().filter(|(tick,_)|*tick>hub.warmup).map(|(tick,kind)| {
                let frame=tick-hub.warmup-1;
                let name=["swing","clang","step","stone","crystal","polymer","toss","land","dispense","jetpack","ghoul_hit"][kind as usize];
                json!({"frame":frame,"seconds":frame as f64/f64::from(hub.options.fps()),"cue":name})
            }).collect();
            hub.save_report(video.completed);
            exit.write(AppExit::Success);
        }
        return;
    }
    let frame = video.submitted;
    let path = std::path::Path::new(hub.options.frames_dir.as_ref().unwrap())
        .join(format!("frame_{frame:06}.png"));
    let trace_start = std::time::Instant::now();
    if let Some(trace) = &mut video.trace {
        writeln!(trace,"{}",json!({"frame":frame,"time":frame as f64/f64::from(hub.options.fps()),"state":hub.latest})).expect("write frame state");
    }
    telemetry.sample("trace_write", trace_start.elapsed().as_secs_f64() * 1000.);
    telemetry.gauge(
        "capture_pending",
        (video.submitted - video.completed) as f64,
    );
    telemetry.count("capture_submitted", 1);
    video.submitted += 1;
    commands.spawn(Screenshot::image(target)).observe(
        move |capture: On<ScreenshotCaptured>,
              mut video: ResMut<Video>,
              mut telemetry: ResMut<crate::telemetry::Telemetry>| {
            let start = std::time::Instant::now();
            let rgb = capture
                .image
                .clone()
                .try_into_dynamic()
                .expect("captured image")
                .to_rgb8();
            use image::{
                codecs::png::{CompressionType, FilterType, PngEncoder},
                ImageEncoder,
            };
            let file = std::io::BufWriter::new(std::fs::File::create(&path).expect("frame file"));
            PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::NoFilter)
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .expect("write video frame");
            video.completed += 1;
            telemetry.sample("capture_png_write", start.elapsed().as_secs_f64() * 1000.);
            telemetry.count("capture_completed", 1);
        },
    );
}

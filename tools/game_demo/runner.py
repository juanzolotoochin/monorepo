"""Local GPU action driver. The game owns commands, simulation and world reports."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    for name in ("game", "script", "ffmpeg", "video", "report", "trace", "audio"):
        parser.add_argument(f"--{name}", required=True, type=Path)
    args = parser.parse_args()
    game, script, ffmpeg, video, report, trace, audio = (
        getattr(args, name).absolute()
        for name in ("game", "script", "ffmpeg", "video", "report", "trace", "audio")
    )
    config = json.loads(script.read_text())
    with tempfile.TemporaryDirectory(prefix="kestrel-demo-") as temporary:
        frames = Path(temporary) / "frames"
        environment = os.environ.copy()
        environment.pop("DISPLAY", None)
        environment.pop("WAYLAND_DISPLAY", None)
        environment["XDG_RUNTIME_DIR"] = temporary
        result = subprocess.run(
            [str(game), "--demo-script", str(script), "--frames-dir", str(frames),
             "--report", str(report), "--trace", str(trace), "--audio-wav", str(audio)],
            env=environment, timeout=max(120, config["max_frames"] * 0.25 + 90),
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if result.returncode:
            print(result.stdout)
            if report.exists():
                print(report.read_text())
            raise SystemExit(f"Game demo failed with exit code {result.returncode}")
        state = json.loads(report.read_text())
        count = len(list(frames.glob("frame_*.png")))
        if state["status"] != "passed" or count == 0 or count != state["frames"]:
            raise SystemExit(f"Incomplete demo capture: {count} frames, report {state}")
        version = subprocess.check_output([str(ffmpeg), "-version"], text=True).splitlines()[0]
        subprocess.run(
            [str(ffmpeg), "-hide_banner", "-loglevel", "error", "-y",
             "-framerate", str(config["fps"]), "-i", str(frames / "frame_%06d.png"),
             "-i", str(audio), "-frames:v", str(count), "-c:v", "libx264", "-crf", "18", "-preset", "medium",
             "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-shortest", "-movflags", "+faststart", str(video)],
            check=True, timeout=120,
        )
        state["video"] = {"frames": count, "fps": config["fps"],
                          "duration_seconds": count / config["fps"], "encoder": version,
                          "width": config["width"], "height": config["height"], "audio": True, "audio_sample_rate": 48000}
        report.write_text(json.dumps(state, indent=2) + "\n")
        print(f"Rendered {count} frames to {video.name}; all scripted assertions passed.")


if __name__ == "__main__":
    main()

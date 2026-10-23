"""Local GPU action driver. The game owns commands, simulation and world reports."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

from game.tools.demo.assertions import validate


def main():
    parser = argparse.ArgumentParser()
    for name in ("game", "script", "ffmpeg", "video", "report", "trace", "audio"):
        parser.add_argument(f"--{name}", required=True, type=Path)
    parser.add_argument("--assertions", type=Path)
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
        if args.assertions:
            with trace.open() as stream:
                checks = json.loads(args.assertions.read_text())
                first_command = state["receipts"][0]["started_tick"]
                active_frames = (
                    line for line in stream
                    if (record := json.loads(line))["frame"] >= first_command
                )
                state["trace_assertions"] = validate(active_frames, checks)
        # The GPU capture and the headless test must drive the same settlement
        # state at every completed command. Warmup and video tail frames differ;
        # receipt fingerprints compare the gameplay timeline itself.
        headless_report = Path(temporary) / "headless.json"
        headless = subprocess.run(
            [str(game), "--simulation", "--demo-script", str(script),
             "--report", str(headless_report)],
            env=environment, timeout=max(120, config["max_frames"] * 0.25 + 90),
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if headless.returncode:
            raise SystemExit(f"Headless parity run failed: {headless.stdout[-2000:]}")
        reference = json.loads(headless_report.read_text())
        if reference["status"] != "passed" or len(reference["receipts"]) != len(state["receipts"]):
            raise SystemExit("Rendered and headless demo command receipts differ")
        compared = 0
        for index, (expected, actual) in enumerate(zip(reference["receipts"], state["receipts"])):
            if expected["command"] != actual["command"] or expected["status"] != actual["status"]:
                raise SystemExit(f"Rendered/headless command {index} differs")
            left = expected["state"]["settlement"]
            right = actual["state"]["settlement"]
            if (left is None) != (right is None):
                raise SystemExit(f"Rendered/headless scene state differs at command {index}")
            if left is not None:
                compared += 1
                if left["replay_fingerprint"] != right["replay_fingerprint"]:
                    raise SystemExit(f"Rendered/headless gameplay diverged at command {index}")
        state["headless_parity_receipts"] = compared
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

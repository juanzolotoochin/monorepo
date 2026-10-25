"""Record a fixed Character Studio clip and encode its frames as MP4."""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--game", required=True, type=Path)
    parser.add_argument("--ffmpeg", required=True, type=Path)
    parser.add_argument("--video", required=True, type=Path)
    parser.add_argument("--character", required=True)
    parser.add_argument("--clip", required=True)
    parser.add_argument("--angle", required=True)
    parser.add_argument("--frames", required=True, type=int)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="kestrel-studio-") as temporary:
        frames = Path(temporary) / "frames"
        environment = os.environ.copy()
        environment.pop("DISPLAY", None)
        environment.pop("WAYLAND_DISPLAY", None)
        environment["XDG_RUNTIME_DIR"] = temporary
        result = subprocess.run(
            [str(args.game.absolute()), "--character-studio",
             "--studio-character", args.character, "--studio-clip", args.clip,
             "--studio-angle", args.angle, "--frames-dir", str(frames),
             "--studio-video-frames", str(args.frames)],
            env=environment, timeout=max(120, args.frames * 0.5 + 90),
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if result.returncode:
            raise SystemExit(f"Character Studio capture failed: {result.stdout[-3000:]}")
        count = len(list(frames.glob("frame_*.png")))
        if count != args.frames:
            raise SystemExit(f"Incomplete studio video: {count}/{args.frames} frames")
        subprocess.run(
            [str(args.ffmpeg.absolute()), "-hide_banner", "-loglevel", "error", "-y",
             "-framerate", "60", "-i", str(frames / "frame_%06d.png"),
             "-frames:v", str(args.frames), "-c:v", "libx264", "-crf", "18",
             "-preset", "medium", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
             str(args.video.absolute())],
            check=True, timeout=120,
        )
        print(f"Rendered {count} Character Studio frames to {args.video.name}")


if __name__ == "__main__":
    main()

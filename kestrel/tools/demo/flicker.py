"""Detect persistent A-B-A-B pixel flashes in a rendered game demo.

The video is decoded by the repository's pinned FFmpeg. A four-frame reversal
rejects ordinary one-way motion; small clusters reject isolated AA noise.
"""
import argparse
from collections import deque
from pathlib import Path
import subprocess


def flashes(frames, width, height, *, jump=42, return_tolerance=18, tile=8):
    """Yield (last frame, peak flashing pixels in one tile). RGB input is bytes."""
    history = deque(maxlen=4)
    for number, rgb in enumerate(frames):
        history.append(rgb)
        if len(history) < 4:
            continue
        a, b, c, d = history
        counts = {}
        for pixel in range(width * height):
            offset = pixel * 3
            ay = (a[offset] * 3 + a[offset + 1] * 6 + a[offset + 2]) // 10
            by = (b[offset] * 3 + b[offset + 1] * 6 + b[offset + 2]) // 10
            cy = (c[offset] * 3 + c[offset + 1] * 6 + c[offset + 2]) // 10
            dy = (d[offset] * 3 + d[offset + 1] * 6 + d[offset + 2]) // 10
            if (abs(ay - by) >= jump and abs(ay - cy) <= return_tolerance
                    and abs(by - dy) <= return_tolerance
                    and abs(cy - dy) >= jump):
                key = ((pixel // width) // tile, (pixel % width) // tile)
                counts[key] = counts.get(key, 0) + 1
        yield number, max(counts.values(), default=0)


def decoded_frames(ffmpeg: Path, video: Path, width: int, height: int):
    command = [str(ffmpeg), "-hide_banner", "-loglevel", "error", "-i", str(video),
               "-vf", f"scale={width}:{height}:flags=area", "-f", "rawvideo",
               "-pix_fmt", "rgb24", "-"]
    process = subprocess.Popen(command, stdout=subprocess.PIPE)
    size = width * height * 3
    try:
        while True:
            frame = process.stdout.read(size)
            if not frame:
                break
            if len(frame) != size:
                raise AssertionError("incomplete decoded video frame")
            yield frame
        if process.wait() != 0:
            raise AssertionError("FFmpeg could not decode demo video")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--video", required=True, type=Path)
    parser.add_argument("--ffmpeg", required=True, type=Path)
    parser.add_argument("--window", nargs=2, type=int, action="append", required=True)
    parser.add_argument("--max-tile-flashes", type=int, default=12)
    args = parser.parse_args()
    samples = list(flashes(decoded_frames(args.ffmpeg, args.video, 320, 180), 320, 180))
    for start, end in args.window:
        window = [(frame, count) for frame, count in samples if start <= frame <= end]
        if not window:
            raise AssertionError(f"flicker window {start}-{end} has no captured frames")
        frame, peak = max(window, key=lambda sample: sample[1])
        print(f"frames {start}-{end}: peak {peak} alternating pixels in one tile at frame {frame}")
        if peak > args.max_tile_flashes:
            raise AssertionError(f"persistent visual flicker at frame {frame}: {peak} pixels")


if __name__ == "__main__":
    main()

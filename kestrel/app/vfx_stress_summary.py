"""Debug only: summarizes `--vfx-stress` reports as markdown tables.

Usage: python3 vfx_stress_summary.py <dir>, where <dir> holds
<backend>_<load>.json reports (`deck_vfx_stress.sh` writes them). Prints
frame ms (mean / p95 / p99 / max) per load for legacy and sprinkles, then
main, extract, GPU and render segment means. Standard library only, so it
runs on the Steam Deck too. Remove it with `src/vfx_stress.rs`.
"""
import json
import pathlib
import re
import sys

LOADS = ["idle", "i10", "i50", "i200", "lasers", "arcs", "mix50", "mix200"]


def mean(stats, key):
    return stats.get(key, {}).get("mean", float("nan"))


def gpu_total(stats):
    # Top-level passes only: render/<pass>/elapsed_gpu.
    return sum(v["mean"] for k, v in stats.items() if re.fullmatch(r"render/[^/]+/elapsed_gpu", k))


def particle_gpu(stats):
    return sum(v["mean"] for k, v in stats.items() if k.endswith("/elapsed_gpu") and "particle_" in k)


def frame(s):
    return f'{s["mean"]:.2f} / {s["p95"]:.2f} / {s["p99"]:.2f} / {s["max"]:.1f}'


def main(directory):
    rows = {}
    for path in sorted(pathlib.Path(directory).glob("*.json")):
        backend, load = path.stem.split("_", 1)
        report = json.loads(path.read_text())
        stats = report["stats_ms"]
        rows[(load, backend)] = dict(
            frame=stats["frame_ms"], update=mean(stats, "update_ms"), extract=mean(stats, "extract_ms"),
            render={k: v["mean"] for k, v in stats.items() if k.startswith("render_") and k.endswith("_ms")},
            gpu=gpu_total(stats), particle=particle_gpu(stats), aborted=report["aborted"],
            frames=report["measured_frames"],
            memory=max((g.get("gpu_allocated_mb", 0) for g in report.get("gauges", [])), default=0))
    aborted = lambda row: f' (aborted, {row["frames"]} frames)' if row["aborted"] else ""
    print("| Load | Legacy | Sprinkles | Δ mean |\n|---|---|---|---|")
    for load in LOADS:
        legacy, sprinkles = rows.get((load, "legacy")), rows.get((load, "sprinkles"))
        if not (legacy and sprinkles):
            continue
        delta = sprinkles["frame"]["mean"] - legacy["frame"]["mean"]
        print(f'| {load} | {frame(legacy["frame"])}{aborted(legacy)} | {frame(sprinkles["frame"])}{aborted(sprinkles)} | {delta:+.2f} |')
    print()
    print("| Load | backend | main | extract | GPU all | particle GPU | peak GPU MB | render segments |")
    print("|---|---|---|---|---|---|---|---|")
    for load in LOADS:
        for backend in ("legacy", "sprinkles"):
            row = rows.get((load, backend))
            if not row:
                continue
            segments = ", ".join(f"{k[7:-3]} {v:.2f}" for k, v in sorted(row["render"].items()))
            print(f'| {load} | {backend} | {row["update"]:.2f} | {row["extract"]:.2f} | {row["gpu"]:.2f} '
                  f'| {row["particle"]:.2f} | {row["memory"]:.0f} | {segments} |')


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])

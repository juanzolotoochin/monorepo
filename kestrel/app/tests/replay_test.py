"""Replay a controlled demo twice and compare the per-tick gameplay fingerprints."""
import argparse
import json
import subprocess
import tempfile
from pathlib import Path


def run(game: Path, script: Path, directory: Path):
    directory.mkdir()
    report = directory / "report.json"
    trace = directory / "trace.jsonl"
    subprocess.run(
        [str(game), "--simulation", "--demo-script", str(script),
         "--report", str(report), "--trace", str(trace)],
        check=True, timeout=120,
    )
    result = json.loads(report.read_text())
    assert result["status"] == "passed", result.get("error")
    frames = [json.loads(line) for line in trace.read_text().splitlines()]
    return [(f["state"]["settlement"]["simulation"]["tick"],
             f["state"]["settlement"]["replay_fingerprint"])
            for f in frames if f["state"]["settlement"] is not None]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("game", type=Path)
    parser.add_argument("script", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        first = run(args.game, args.script, root / "first")
        second = run(args.game, args.script, root / "second")
    assert len(first) > 100
    if first != second:
        raise AssertionError(next(
            (i, a, b) for i, (a, b) in enumerate(zip(first, second)) if a != b
        ))


if __name__ == "__main__":
    main()

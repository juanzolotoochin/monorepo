"""Check a completed simulation's report and recorded JSONL world-state dump."""
import argparse
import json
from pathlib import Path

from game.tools.demo.assertions import validate


def main():
    parser = argparse.ArgumentParser()
    for name in ('report', 'trace', 'assertions'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    if report['status'] != 'passed':
        raise AssertionError(f'Script failed: {report["error"]}')
    with args.trace.open() as records:
        results = validate(records, json.loads(args.assertions.read_text()))
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()

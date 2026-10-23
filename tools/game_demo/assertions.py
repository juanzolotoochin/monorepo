"""Incremental, bounded-memory assertions over the game's JSONL world snapshots.

Usable with a saved trace or a live pipe: --trace - reads stdin until EOF.
Angles are radians in snapshots and degrees in assertion specifications.
"""
import argparse
from collections import deque
import json
import math
from pathlib import Path
import sys


def value_at(state, path):
    for key in path.split('.'):
        state = state[int(key)] if isinstance(state, list) else state[key]
    return state


def angle_delta(a, b):
    return abs(math.degrees(math.atan2(math.sin(a - b), math.cos(a - b))))


class TraceAssertions:
    def __init__(self, spec):
        if spec.get('version') != 1 or set(spec) - {'version', 'motion', 'always', 'final'}:
            raise ValueError('Expected assertion version 1 with motion/always/final checks')
        self.spec = spec
        self.previous = {}
        self.windows = {}
        self.last = None
        self.count = 0
        self.peaks = {}
        for check in spec.get('motion', []):
            if set(check) != {'path', 'window_seconds', 'max_turn_degrees', 'max_rate_degrees', 'max_travel_turn_degrees'}:
                raise ValueError('Motion check requires path, window_seconds and all three limits')
            for key in ('window_seconds', 'max_turn_degrees', 'max_rate_degrees', 'max_travel_turn_degrees'):
                if not math.isfinite(check[key]) or check[key] <= 0:
                    raise ValueError(f'{key} must be finite and positive')
        for check in spec.get('always', []) + spec.get('final', []):
            if set(check) - {'path', 'min', 'max', 'equals'} or 'path' not in check or len(check) < 2:
                raise ValueError('State checks require path and min/max/equals')

    def fail(self, message):
        raise AssertionError(f'frame {self.last["frame"]}, t={self.last["time"]:.3f}s: {message}')

    def state_checks(self, checks):
        for check in checks:
            value = value_at(self.last['state'], check['path'])
            for op in ('min', 'max', 'equals'):
                if op not in check:
                    continue
                limit = check[op]
                valid = (value >= limit if op == 'min' else value <= limit if op == 'max' else value == limit)
                if not valid:
                    self.fail(f'{check["path"]}={value!r}; expected {op} {limit!r}')

    def feed(self, record):
        time = record['time']
        if not math.isfinite(time) or (self.last and (time <= self.last['time'] or record['frame'] <= self.last['frame'])):
            raise AssertionError('Trace frames and finite timestamps must strictly increase')
        self.last = record
        self.count += 1
        self.state_checks(self.spec.get('always', []))
        for i, check in enumerate(self.spec.get('motion', [])):
            actor = value_at(record['state'], check['path'])
            heading, position = actor['heading'], actor['position']
            if len(position) != 2 or not all(math.isfinite(v) for v in [heading, *position]):
                self.fail(f'{check["path"]}: invalid heading/position')
            previous = self.previous.get(i)
            travel_heading = previous[3] if previous else None
            if previous:
                dt = time - previous[0]
                turn = angle_delta(heading, previous[1])
                dx, dz = position[0] - previous[2][0], position[1] - previous[2][1]
                travel_turn = 0.0
                if math.hypot(dx, dz) > 0.02:
                    direction = math.atan2(dx, dz)
                    if travel_heading is not None:
                        travel_turn = angle_delta(direction, travel_heading)
                    travel_heading = direction
                window = self.windows.setdefault(i, deque())
                window.append((time, turn, travel_turn))
                while window and window[0][0] <= time - check['window_seconds']:
                    window.popleft()
                measurements = {'max_rate_degrees': turn / dt,
                                'max_turn_degrees': sum(v[1] for v in window),
                                'max_travel_turn_degrees': sum(v[2] for v in window)}
                for key, measured in measurements.items():
                    name = check['path'] + ':' + key
                    self.peaks[name] = max(self.peaks.get(name, 0), measured)
                    if measured > check[key] + 1e-4:
                        self.fail(f'{name} measured {measured:.2f}, limit {check[key]} (window {check["window_seconds"]}s)')
            self.previous[i] = (time, heading, position, travel_heading)

    def finish(self):
        if self.count < 2:
            raise AssertionError('At least two trace frames are required')
        self.state_checks(self.spec.get('final', []))
        return {'frames': self.count, 'peaks': self.peaks}


def validate(trace, spec):
    validator = TraceAssertions(spec)
    for line in trace:
        validator.feed(json.loads(line))
    return validator.finish()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace', required=True)
    parser.add_argument('--assertions', required=True, type=Path)
    args = parser.parse_args()
    spec = json.loads(args.assertions.read_text())
    try:
        if args.trace == '-':
            result = validate(sys.stdin, spec)
        else:
            with open(args.trace) as trace:
                result = validate(trace, spec)
        print(json.dumps(result, indent=2))
    except (AssertionError, KeyError, ValueError, TypeError) as error:
        raise SystemExit(f'Trace assertion failed: {error}') from error


if __name__ == '__main__':
    main()

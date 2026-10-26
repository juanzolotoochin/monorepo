"""Measure an empty Cargo compiler cache separately from warm checks.

Checks the unchanged current Kestrel source tree. No original files are written.
"""
import json
import argparse
import runpy
import shutil
import subprocess
import time
from pathlib import Path

def cleanup(target, cache):
    resolved = target.resolve()
    assert resolved.parent == cache.resolve() and resolved.name.startswith('cold-target-'), resolved
    shutil.rmtree(resolved)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--keep-artifacts', action='store_true')
    args = parser.parse_args()
    api = runpy.run_path(str(Path(__file__).with_name('cache.py')))
    api['check_fresh']()
    cache = api['CACHE']
    target = cache / ('cold-target-' + str(time.time_ns()))
    target.mkdir()
    env = api['cargo_env']()
    env['CARGO_TARGET_DIR'] = str(target)
    cargo, _ = api['toolchain']()
    measurements = []
    for index in range(3):
        output = cache / f'benchmark-check-{index}.jsonl'
        started = time.monotonic()
        with output.open('w', encoding='utf-8') as stream:
            result = subprocess.run([str(cargo), 'check', '--workspace', '--all-targets', '--offline',
                                     '--manifest-path', str(cache / 'Cargo.toml'), '--message-format=json'],
                                    cwd=cache, env=env, stdout=stream, stderr=subprocess.PIPE, text=True)
        elapsed = time.monotonic() - started
        (cache / f'benchmark-check-{index}.log').write_text(result.stderr, encoding='utf-8')
        errors = [e['message'] for line in output.read_text(encoding='utf-8').splitlines()
                  if (e := json.loads(line)).get('reason') == 'compiler-message' and e['message']['level'] == 'error']
        assert result.returncode == 0 and not errors, result.stderr[-3000:] + '\n' + json.dumps(errors)
        item = {'cache': 'empty compiler artifacts' if index == 0 else 'warm compiler artifacts',
                'seconds': elapsed, 'exit_code': result.returncode, 'syntax_errors': len(errors)}
        measurements.append(item)
        print(json.dumps(item), flush=True)
        (cache / 'benchmark-results.json').write_text(json.dumps({'target': target.as_posix(), 'measurements': measurements, 'artifacts_retained': True}, indent=2), encoding='utf-8')
    if not args.keep_artifacts:
        cleanup(target, cache)
        (cache / 'benchmark-results.json').write_text(json.dumps({'target': target.as_posix(), 'measurements': measurements, 'artifacts_retained': False}, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()

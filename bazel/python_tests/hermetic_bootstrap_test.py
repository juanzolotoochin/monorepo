"""Launch raw and wrapped Bazel binaries while host Python is unusable."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

# Keep the executable wrapper path: resolving its final symlink bypasses it.
LAUNCHERS = [Path(arg).absolute() for arg in sys.argv[1:]]
del sys.argv[1:]


class HermeticBootstrapTest(unittest.TestCase):
    def test_launchers_never_call_host_python(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            marker = directory / 'host-python-called'
            for name in ['python', 'python3', 'python3.11']:
                executable = directory / name
                executable.write_text(f'#!/bin/sh\necho called > "{marker}"\nexit 97\n')
                executable.chmod(0o755)
            environment = dict(os.environ)
            environment['PATH'] = str(directory) + os.pathsep + environment.get('PATH', '')
            # Exercise adjacent-runfiles discovery, as with a directly launched tool.
            environment.pop('RUNFILES_DIR', None)
            environment.pop('RUNFILES_MANIFEST_FILE', None)
            environment.pop('PYTHONPATH', None)
            for launcher in LAUNCHERS:
                with self.subTest(launcher=launcher):
                    result = subprocess.run([str(launcher)], env=environment, cwd=directory,
                                            text=True, capture_output=True, timeout=30)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertFalse(marker.exists(), 'Launcher invoked host Python')
                    payload = json.loads(result.stdout)
                    self.assertEqual(payload['version'], [3, 11])
                    self.assertEqual(payload['interpreter'], str(Path(sys.executable).resolve()))
                    self.assertEqual(payload['fonttools'], '4.62.1')


if __name__ == '__main__':
    unittest.main()

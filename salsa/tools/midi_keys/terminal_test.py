"""Exercise the real binary in a PTY without MIDI hardware or host utilities."""

import contextlib
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import termios
import time
import unittest


BINARY = Path(os.environ["TEST_SRCDIR"]) / os.environ["TEST_WORKSPACE"] / (
    "salsa/tools/midi_keys/midi_keys"
)


def resize(fd, rows, columns):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))


def read_until(fd, expected):
    output = b""
    deadline = time.monotonic() + 5
    while expected not in output and time.monotonic() < deadline:
        if select.select([fd], [], [], 0.1)[0]:
            output += os.read(fd, 65536)
    if expected not in output:
        raise AssertionError(f"Missing {expected!r} in terminal output: {output!r}")
    return output


@contextlib.contextmanager
def demo():
    master, slave = pty.openpty()
    resize(slave, 24, 80)
    original = termios.tcgetattr(slave)
    # Prove that the executable does not launch utilities or read ALSA config.
    env = dict(os.environ, PATH="/nonexistent", ALSA_CONFIG_PATH="/nonexistent")
    process = subprocess.Popen(
        [str(BINARY), "--demo", "--silent"], stdin=slave, stdout=slave, stderr=slave, env=env
    )
    try:
        read_until(master, b"Held: C4(80)  E4(92)  G4(104)")
        yield process, master, slave
        assert process.wait(timeout=5) == 0
        assert termios.tcgetattr(slave) == original, "Terminal settings not restored"
        read_until(master, b"\x1b[?1049l")
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
        os.close(master)
        os.close(slave)


class TerminalTest(unittest.TestCase):
    def test_quit_and_restore(self):
        with demo() as (_, master, _):
            os.write(master, b"q")

    def test_ctrl_c_and_restore(self):
        with demo() as (_, master, _):
            os.write(master, b"\x03")

    def test_sigterm_and_restore(self):
        with demo() as (process, _, _):
            process.send_signal(signal.SIGTERM)

    def test_resize_and_octave_controls(self):
        with demo() as (_, master, slave):
            os.write(master, b"\x1b[A]")
            read_until(master, b"C4 - C6 | manual")
            os.write(master, b"f")
            read_until(master, b"C4 - C6 | follow")
            resize(slave, 10, 40)
            read_until(master, b"Resize to 64 x 23")
            resize(slave, 24, 80)
            read_until(master, b"MIDI KEYS")
            os.write(master, b"q")

    def test_noninteractive_modes(self):
        snapshot = subprocess.run([str(BINARY), "--snapshot"], capture_output=True, check=True)
        self.assertIn(b"Held: C4(80)  E4(92)  G4(104)", snapshot.stdout)
        self.assertNotIn(b"\x1b", snapshot.stdout)
        self.assertTrue(all(len(line) <= 64 for line in snapshot.stdout.splitlines()))
        for args in [["--bad"], ["--port"], ["--demo", "--list"], ["--demo"]]:
            result = subprocess.run([str(BINARY), *args], capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0, args)
            self.assertNotIn(b"\x1b", result.stdout)


if __name__ == "__main__":
    unittest.main()

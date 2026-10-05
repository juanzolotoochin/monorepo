"""Exercise the real binary in a PTY without MIDI hardware or host utilities."""

import codecs
import contextlib
import fcntl
import os
from pathlib import Path
import pty
import re
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


class Screen:
    """Minimal VT screen for the cursor/erase sequences used by Crossterm.

    Assertions inspect screen contents, since Ratatui sends incremental updates
    instead of retransmitting whole lines after each keystroke.
    """
    def __init__(self):
        self.decoder = codecs.getincrementaldecoder("utf-8")()
        self.pending = ""
        self.x = self.y = 0
        self.width = self.height = 0
        self.cells = []

    def feed(self, fd, data):
        rows, cols, _, _ = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, bytes(8)))
        if (cols, rows) != (self.width, self.height):
            self.width, self.height = cols, rows
            self.cells = [[" "] * cols for _ in range(rows)]
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith("\x1b"):
                match = re.match(r"\x1b\[([0-9;?]*)([@-~])", self.pending)
                if not match:
                    break
                args, command = match.groups()
                self.pending = self.pending[match.end():]
                if args.startswith("?"):
                    continue
                values = [int(v or 0) for v in args.split(";")]
                n = values[0] or 1
                if command in "Hf":
                    self.y = n - 1
                    self.x = (values[1] if len(values) > 1 and values[1] else 1) - 1
                elif command == "G": self.x = n - 1
                elif command == "d": self.y = n - 1
                elif command == "A": self.y = max(0, self.y - n)
                elif command == "B": self.y += n
                elif command == "C": self.x += n
                elif command == "D": self.x = max(0, self.x - n)
                elif command == "J" and values[0] in (2, 3):
                    self.cells = [[" "] * self.width for _ in range(self.height)]
                elif command == "K" and self.y < self.height:
                    start = 0 if values[0] == 2 else self.x
                    for x in range(start, self.width): self.cells[self.y][x] = " "
                continue
            char, self.pending = self.pending[0], self.pending[1:]
            if char == "\r": self.x = 0
            elif char == "\n": self.y += 1
            elif char >= " ":
                if self.x >= self.width: self.x = 0; self.y += 1
                if 0 <= self.y < self.height: self.cells[self.y][self.x] = char
                self.x += 1

    def text(self):
        return "\n".join("".join(row) for row in self.cells)


SCREENS = {}


def read_until(fd, expected):
    output = b""
    screen = SCREENS.setdefault(fd, Screen())
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if expected in output or expected.decode() in screen.text():
            return output
        if select.select([fd], [], [], 0.1)[0]:
            data = os.read(fd, 65536)
            output += data
            screen.feed(fd, data)
    raise AssertionError(f"Missing {expected!r} in terminal screen:\n{screen.text()}")


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
        read_until(master, b"HELD NOTES")
        yield process, master, slave
        assert process.wait(timeout=5) == 0
        assert termios.tcgetattr(slave) == original, "Terminal settings not restored"
        read_until(master, b"\x1b[?1049l")
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
        SCREENS.pop(master, None)
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
            read_until(master, b"MANUAL RANGE")
            os.write(master, b"f")
            read_until(master, b"AUTO FOLLOW")
            resize(slave, 38, 179)
            read_until(master, "C1 — C9".encode())
            resize(slave, 10, 40)
            read_until(master, "Resize to at least 64 × 22".encode())
            resize(slave, 24, 80)
            read_until(master, b"MIDI KEYS")
            os.write(master, b"q")

    def test_direct_exercise_arguments_are_validated_before_devices(self):
        cases = [
            (["--debug-exercise"], b"requires a file path"),
            (["--debug-exercise", "/unused"], b"requires --exercise ID or --exercises"),
            (["--exercise", "reading.right.0", "--debug-exercise", "--silent"], b"requires a file path"),
            (["--exercise"], b"requires a skill ID"),
            (["--exercise", "--port", "32:0"], b"requires a skill ID"),
            (["--exercise", "rhythm.0"], b"Unknown exercise ID"),
            (["--exercise", "rhythm.0.60", "--free-play"], b"preview mode"),
            (["--exercise", "rhythm.0.60", "--profile", "/unused"], b"preview mode"),
            (["--exercise", "rhythm.0.60", "--snapshot"], b"preview mode"),
            (["--exercise", "rhythm.0.60"], b"An interactive terminal is required"),
            (["--port", "32:0", "--exercise", "rhythm.0.60"], b"An interactive terminal is required"),
            (["--exercises", "--exercise", "chord.build.3.11"], b"An interactive terminal is required"),
        ]
        for args, expected in cases:
            with self.subTest(args=args):
                result = subprocess.run([str(BINARY), *args], capture_output=True, timeout=5)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected, result.stderr)
                self.assertNotIn(b"Loading Salamander", result.stderr)

    def test_noninteractive_modes(self):
        snapshot = subprocess.run([str(BINARY), "--snapshot"], capture_output=True, check=True)
        self.assertIn(b"Held: C4(80)  E4(92)  G4(104)", snapshot.stdout)
        self.assertNotIn(b"\x1b", snapshot.stdout)
        self.assertTrue(all(len(line) <= 64 for line in snapshot.stdout.splitlines()))
        for args in [["--bad"], ["--port"], ["--demo", "--list"], ["--demo"],
                     ["--free-play", "--demo"], ["--trace-events"],
                     ["--silent", "--trace-events", "/tmp/unused-midi-trace"]]:
            result = subprocess.run([str(BINARY), *args], capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0, args)
            self.assertNotIn(b"\x1b", result.stdout)


if __name__ == "__main__":
    unittest.main()

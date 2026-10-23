"""Exercise the public TCP interface against the real renderer-free game process."""
import concurrent.futures
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest

GAME = Path(sys.argv.pop(1)).resolve()


class ControlProtocolTest(unittest.TestCase):
    def test_live_commands_inspection_and_rejected_exact_quantity(self):
        with tempfile.TemporaryDirectory() as temporary:
            address = Path(temporary) / "address"
            process = subprocess.Popen(
                [str(GAME), "--simulation", "--scene", "pile-lab", "--control-port", "0",
                 "--control-address-file", str(address)],
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                env={k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")},
            )
            try:
                deadline = time.monotonic() + 15
                while not address.exists() and time.monotonic() < deadline:
                    if process.poll() is not None:
                        self.fail(process.communicate()[0])
                    time.sleep(0.01)
                self.assertTrue(address.exists(), "server address was not published")
                host, port = address.read_text().split(":")

                def call(op, **parameters):
                    with socket.create_connection((host, int(port)), timeout=10) as client:
                        client.settimeout(15)
                        stream = client.makefile("rwb")
                        stream.write((json.dumps({"id": op, "command": {"op": op, **parameters}}) + "\n").encode())
                        stream.flush()
                        return json.loads(stream.readline())

                state = call("inspect")["state"]
                self.assertEqual(state["totals"]["created"], 96)
                self.assertEqual(call("add", dispenser=0, units=8)["status"], "completed")
                rejected = call("grab", pile=0, units=13)
                self.assertEqual(rejected["status"], "failed")
                self.assertEqual(rejected["state"]["piles"][0]["units"], 8)
                with concurrent.futures.ThreadPoolExecutor() as pool:
                    walking = pool.submit(call, "walk", position=[-7.0, 7.0])
                    moving = None
                    for _ in range(20):
                        snapshot = call("inspect")["state"]
                        if snapshot["active_command"] is not None:
                            moving = snapshot
                            break
                        time.sleep(0.002)
                    self.assertIsNotNone(moving, "inspection must work while a command is active")
                    self.assertEqual(walking.result()["status"], "completed")
                grabbed = call("grab", pile=0, units=4, material="AmberOre")
                self.assertEqual(grabbed["status"], "completed")
                self.assertEqual(grabbed["state"]["player"]["cargo"]["units"], 4)
                dropped = call("drop_all", position=[4.0, 4.5])
                self.assertEqual(dropped["status"], "completed")
                state = call("inspect")["state"]
                self.assertEqual(state["piles"][0]["units"], 4)
                self.assertEqual(state["piles"][-1]["units"], 4)
                self.assertEqual(state["visible_mineral_units"], 104)
                self.assertTrue(state["totals"]["conserved"])
                self.assertEqual(call("quit")["status"], "completed")
                self.assertEqual(process.wait(timeout=10), 0)
            finally:
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=10)


if __name__ == "__main__":
    unittest.main()

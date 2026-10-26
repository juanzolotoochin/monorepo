import unittest

from kestrel.tools.demo.flicker import flashes


class FlickerTest(unittest.TestCase):
    def test_repeated_flash_is_detected_but_one_way_motion_is_not(self):
        black = bytes([0, 0, 0] * 64)
        white = bytes([255, 255, 255] * 64)
        self.assertEqual(list(flashes([black, white, black, white], 8, 8))[-1], (3, 64))
        self.assertEqual(list(flashes([black, white, white, white], 8, 8))[-1], (3, 0))


if __name__ == "__main__":
    unittest.main()

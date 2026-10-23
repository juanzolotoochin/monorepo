import tempfile
import unittest
from pathlib import Path

from bazel.platforms.sdk_vfs import directory


class OverlayTest(unittest.TestCase):
    def test_preserves_physical_header_casing_and_relative_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Nested").mkdir()
            header = root / "Nested" / "Windows.H"
            header.write_text("// header", encoding="utf-8")
            node = directory(root, "sdk")["contents"][0]["contents"][0]
            self.assertEqual(node["name"], "Windows.H")
            self.assertEqual(node["external-contents"], header.as_posix())

    def test_lowercase_entry_wins_over_case_duplicate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Windows.h").write_text("upper", encoding="utf-8")
            (root / "windows.h").write_text("lower", encoding="utf-8")
            if len(list(root.iterdir())) != 2:
                self.skipTest("Requires a case-sensitive filesystem")
            self.assertEqual(directory(root, "sdk")["contents"][0]["name"], "windows.h")

    def test_ambiguous_case_collision_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Windows.h").touch()
            (root / "WINDOWS.H").touch()
            if len(list(root.iterdir())) != 2:
                self.skipTest("Requires a case-sensitive filesystem")
            with self.assertRaisesRegex(ValueError, "Ambiguous"):
                directory(root, "sdk")


if __name__ == "__main__":
    unittest.main()

import io
import pathlib
import tarfile
import tempfile
import unittest

from third_party.x11.extract_library import extract_library


class ExtractLibraryTest(unittest.TestCase):
    def test_follows_relative_soname_symlink_without_extracting_other_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            archive_path = root / "data.tar.gz"
            with tarfile.open(archive_path, "w:gz") as archive:
                contents = b"shared library contents"
                library = tarfile.TarInfo("./usr/lib/x86_64-linux-gnu/libxcb.so.1.2.0")
                library.size = len(contents)
                archive.addfile(library, io.BytesIO(contents))
                soname = tarfile.TarInfo("./usr/lib/x86_64-linux-gnu/libxcb.so.1")
                soname.type = tarfile.SYMTYPE
                soname.linkname = "libxcb.so.1.2.0"
                archive.addfile(soname)
                unrelated = tarfile.TarInfo("unrelated")
                archive.addfile(unrelated, io.BytesIO())
            output = root / "lib" / "libxcb.so"
            extract_library(archive_path, "libxcb.so.1", output)
            self.assertEqual(output.read_bytes(), contents)
            self.assertFalse(output.is_symlink())
            self.assertFalse((root / "unrelated").exists())

    def test_rejects_missing_or_ambiguous_soname(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            archive_path = root / "data.tar.gz"
            with tarfile.open(archive_path, "w:gz") as archive:
                for architecture in ("x86_64-linux-gnu", "aarch64-linux-gnu"):
                    archive.addfile(tarfile.TarInfo(f"usr/lib/{architecture}/libxcb.so.1"))
            for soname in ("libxcb.so.1", "missing.so.1"):
                with self.subTest(soname=soname), self.assertRaises(ValueError):
                    extract_library(archive_path, soname, root / "output.so")
            self.assertFalse((root / "output.so").exists())


if __name__ == "__main__":
    unittest.main()

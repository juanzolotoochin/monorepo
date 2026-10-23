"""Copy a SONAME's contents from a Debian data archive without shell tools."""

import pathlib
import shutil
import sys
import tarfile


def extract_library(archive_path, soname, output_path):
    with tarfile.open(archive_path, "r:*") as archive:
        matches = [
            member
            for member in archive.getmembers()
            if pathlib.PurePosixPath(member.name).name == soname
            and member.name.lstrip("./").startswith("usr/lib/")
        ]
        if len(matches) != 1:
            raise ValueError(f"Expected one {soname} in {archive_path}, found {len(matches)}")
        # extractfile follows the SONAME symlink inside the archive, without
        # creating symlinks or materializing any other archive entries.
        source = archive.extractfile(matches[0])
        if source is None:
            raise ValueError(f"{soname} has no file contents")
        output = pathlib.Path(output_path)
        output.parent.mkdir(parents=True, exist_ok=True)
        with source, output.open("wb") as destination:
            shutil.copyfileobj(source, destination)


if __name__ == "__main__":
    extract_library(*sys.argv[1:])

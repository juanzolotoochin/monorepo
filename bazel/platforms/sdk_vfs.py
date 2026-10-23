"""Generate Clang's case-insensitive SDK view without a C toolchain dependency."""

import argparse
import json
from pathlib import Path


def directory(path, name):
    groups = {}
    for child in sorted(path.iterdir()):
        groups.setdefault(child.name.lower(), []).append(child)
    contents = []
    for folded, candidates in sorted(groups.items()):
        preferred = next((p for p in candidates if p.name == folded), None)
        if preferred is None:
            if len(candidates) != 1:
                raise ValueError(f"Ambiguous case-insensitive SDK entries: {candidates}")
            preferred = candidates[0]
        if preferred.is_dir():
            if preferred.is_symlink():
                raise ValueError(f"Unsupported SDK directory symlink: {preferred}")
            contents.append(directory(preferred, preferred.name))
        elif preferred.is_file():
            contents.append({
                "type": "file",
                "name": preferred.name,
                "external-contents": preferred.as_posix(),
            })
        else:
            raise ValueError(f"Unsupported SDK entry: {preferred}")
    return {"type": "directory", "name": name, "contents": contents}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-root", action="append", required=True)
    parser.add_argument("-output", required=True)
    args = parser.parse_args()
    overlay = {
        "version": 0,
        "case-sensitive": False,
        "roots": [directory(Path(root), Path(root).as_posix()) for root in sorted(args.root)],
    }
    Path(args.output).write_text(json.dumps(overlay, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()

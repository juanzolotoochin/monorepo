"""Background removal using rembg.

Usage:
  rembg_tool <input.png> <output.png>
  rembg_tool <manifest.json>   manifest: [{"in": "...", "out": "..."}, ...]
"""
import sys
import json

from rembg import remove, new_session


def main():
    if len(sys.argv) == 2:
        # Batch manifest mode — model is loaded once for all sprites.
        session = new_session()
        with open(sys.argv[1]) as f:
            pairs = json.load(f)
        for pair in pairs:
            with open(pair["in"], "rb") as f:
                data = f.read()
            result = remove(data, session=session)
            with open(pair["out"], "wb") as f:
                f.write(result)
    elif len(sys.argv) == 3:
        input_path, output_path = sys.argv[1], sys.argv[2]
        with open(input_path, "rb") as f:
            data = f.read()
        result = remove(data)
        with open(output_path, "wb") as f:
            f.write(result)
    else:
        print(f"usage: {sys.argv[0]} <input.png> <output.png>", file=sys.stderr)
        print(f"       {sys.argv[0]} <manifest.json>", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()

"""Adapt the upstream Python packers to one deterministic, offline Bazel action."""

import importlib.util
import os
import shutil
import sys
import tempfile
from pathlib import Path


def main():
    source = Path(sys.argv[1]).absolute().parent
    output = Path(sys.argv[2]).absolute()
    vocab = Path(sys.argv[3]).absolute()
    spec = importlib.util.find_spec("gguf")
    os.environ["STRATA_GGUF_PY"] = str(Path(spec.origin).parent.parent)
    import mtp_pack
    import mtp_rt

    # The upstream report contains elapsed times. Only runtime weights are outputs.
    with tempfile.TemporaryDirectory(dir=output.parent) as scratch:
        gguf = str(Path(scratch) / "draft.gguf")
        sys.argv = ["mtp_pack", "--src", str(source), "--experts", "q2_0", "--out", gguf]
        mtp_pack.main()
        sys.argv = ["mtp_rt", "--gguf", gguf, "--out", str(output)]
        mtp_rt.main()
        shutil.copyfile(vocab, output / "draft_vocab.bin")


if __name__ == "__main__":
    main()

"""Serve a Bazel-built static web bundle from runfiles."""

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from python.runfiles import runfiles


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm"}

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, help="bundle directory relative to the workspace")
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()
    locator = runfiles.Create()
    repository = (locator.CurrentRepository() or "_main") if locator else None
    index = locator.Rlocation(f"{repository}/{args.bundle}/index.html") if locator else None
    if not index or not Path(index).is_file():
        parser.error(f"web bundle not found in runfiles: {args.bundle}")
    bundle = Path(index).parent
    server = ThreadingHTTPServer(
        ("127.0.0.1", args.port), partial(Handler, directory=str(bundle))
    )
    print(f"Browser app: http://127.0.0.1:{server.server_port}/", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

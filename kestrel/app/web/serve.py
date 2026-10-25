"""Serve the Bazel-built Kestrel browser bundle from runfiles."""

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm"}

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()
    bundle = Path(__file__).parent.parent / "web_bundle"
    if not (bundle / "index.html").is_file():
        parser.error(f"web bundle not found in runfiles: {bundle}")
    server = ThreadingHTTPServer(
        ("127.0.0.1", args.port), partial(Handler, directory=str(bundle))
    )
    print(f"Kestrel browser build: http://127.0.0.1:{server.server_port}/", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

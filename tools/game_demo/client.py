"""Send one JSON request to a running game's localhost control endpoint."""
import argparse
import json
from pathlib import Path
import socket
import sys


def main():
    parser = argparse.ArgumentParser()
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--address", help="127.0.0.1:PORT")
    target.add_argument("--address-file", type=Path)
    parser.add_argument("request", help="JSON request, or - to read stdin")
    args = parser.parse_args()
    address = args.address or args.address_file.read_text().strip()
    host, port = address.rsplit(":", 1)
    request = json.loads(sys.stdin.read() if args.request == "-" else args.request)
    with socket.create_connection((host, int(port)), timeout=10) as client:
        client.settimeout(125)
        stream = client.makefile("rwb")
        stream.write((json.dumps(request) + "\n").encode())
        stream.flush()
        response = json.loads(stream.readline())
        print(json.dumps(response, indent=2))
        if response["status"] == "failed":
            raise SystemExit(1)


if __name__ == "__main__":
    main()

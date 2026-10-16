#!/usr/bin/env python3
"""Serve a harvested art set to the local studio.

The set will live behind a CDN on a domain of its own, so the page reads it cross-origin and the
browser will not hand those pixels to script without an Access-Control-Allow-Origin header —
the studio measures a logo's brightness to choose its ground, which is a read, not a display.
`SimpleHTTPRequestHandler` sends no such header, so serving the folder with `python3 -m
http.server` would look like it worked and hand the page an opaque image instead. This adds the
header, so the local set behaves the way the hosted one will.

    tools/serve-art.py ~/CartStudioArt 8766
"""

import functools
import http.server
import sys
from pathlib import Path


class Cors(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()

    def log_message(self, *args):
        pass


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else '~/CartStudioArt').expanduser()
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 8766
    if not (root / 'index.json').is_file():
        sys.exit(f'no index.json under {root}: harvest a set there first')
    handler = functools.partial(Cors, directory=str(root))
    print(f'serving {root} at http://127.0.0.1:{port} (index.json present)')
    http.server.ThreadingHTTPServer(('127.0.0.1', port), handler).serve_forever()


if __name__ == '__main__':
    main()

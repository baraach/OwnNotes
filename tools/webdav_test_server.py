#!/usr/bin/env python3
"""A deliberately small WebDAV server for testing OwnNotes against.

Supports just enough of RFC 4918 for the app: PROPFIND (Depth 0/1), MKCOL,
GET, PUT and DELETE, over real directories. Files live in --root; every
request is appended to --log.
"""
import argparse
import hashlib
import os
import sys
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = "/tmp/unused"
LOG = "/tmp/unused.log"
MOUNT = "/dav/"


def etag_of(path):
    try:
        with open(path, "rb") as handle:
            return hashlib.md5(handle.read()).hexdigest()
    except OSError:
        return ""


def href_for(path):
    rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
    if rel == ".":
        return MOUNT
    quoted = urllib.parse.quote(rel)
    return MOUNT + quoted + ("/" if os.path.isdir(path) else "")


def entry(path):
    if os.path.isdir(path):
        kind = "<d:resourcetype><d:collection/></d:resourcetype>"
        etag = ""
    else:
        kind = "<d:resourcetype/>"
        etag = f"<d:getetag>&quot;{etag_of(path)}&quot;</d:getetag>"
    return (
        f"<d:response><d:href>{href_for(path)}</d:href>"
        f"<d:propstat><d:prop>{kind}{etag}"
        "<d:getlastmodified>Mon, 03 Nov 2026 10:00:00 GMT</d:getlastmodified>"
        "</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
    )


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        with open(LOG, "a") as handle:
            handle.write("%s %s\n" % (self.command, self.path))

    def body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def reply(self, status, payload=b"", headers=None):
        self.send_response(status)
        for key, value in (headers or {}).items():
            self.send_header(key, value)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Connection", "close")
        self.end_headers()
        if payload:
            self.wfile.write(payload)

    def resolve(self):
        """The filesystem path for this request, or None if outside the mount."""
        if not self.path.startswith(MOUNT):
            return None
        rest = urllib.parse.unquote(self.path[len(MOUNT):])
        parts = [p for p in rest.split("/") if p not in ("", ".", "..")]
        return os.path.join(ROOT, *parts) if parts else ROOT

    def do_PROPFIND(self):
        self.body()
        fs = self.resolve()
        if fs is None or not os.path.exists(fs):
            self.reply(404)
            return
        depth = self.headers.get("Depth", "0")
        items = [fs]
        if depth != "0" and os.path.isdir(fs):
            for name in sorted(os.listdir(fs)):
                items.append(os.path.join(fs, name))
        payload = (
            '<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">'
            + "".join(entry(item) for item in items)
            + "</d:multistatus>"
        ).encode()
        self.reply(207, payload, {"Content-Type": "application/xml; charset=utf-8"})

    def do_MKCOL(self):
        self.body()
        fs = self.resolve()
        if fs is None:
            self.reply(403)
            return
        try:
            os.makedirs(fs, exist_ok=True)
        except OSError:
            self.reply(409)
            return
        self.reply(201)

    def do_GET(self):
        fs = self.resolve()
        if fs is None or not os.path.isfile(fs):
            self.reply(404)
            return
        with open(fs, "rb") as handle:
            self.reply(200, handle.read(), {"Content-Type": "text/markdown; charset=utf-8"})

    def do_PUT(self):
        fs = self.resolve()
        if fs is None:
            self.reply(405)
            return
        data = self.body()
        os.makedirs(os.path.dirname(fs), exist_ok=True)
        with open(fs, "wb") as handle:
            handle.write(data)
        self.reply(201, b"", {"ETag": '"%s"' % hashlib.md5(data).hexdigest()})

    def do_DELETE(self):
        fs = self.resolve()
        if fs is None or not os.path.exists(fs):
            self.reply(404)
            return
        if os.path.isdir(fs):
            # Empty collections can go; anything else is left alone.
            if os.listdir(fs):
                self.reply(409)
                return
            os.rmdir(fs)
            self.reply(204)
            return
        os.remove(fs)
        self.reply(204)


def main():
    global ROOT, LOG
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--log", required=True)
    args = parser.parse_args()
    ROOT, LOG = args.root, args.log
    os.makedirs(ROOT, exist_ok=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    print(server.server_address[1], flush=True)
    server.serve_forever()


if __name__ == "__main__":
    sys.exit(main())

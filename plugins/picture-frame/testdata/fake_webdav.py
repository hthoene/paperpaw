#!/usr/bin/env python3
"""A fake WebDAV photo server for the emulator run (tools/pps/picture-frame).

    fake_webdav.py <folder of photos> [--port N] [--user U --password P]

Serves the files of the folder under /photos/: `PROPFIND` of depth 1 answers the
listing as Nextcloud does (the folder itself, then a response per file with its
size), `GET` a file. With --user the requests need that Basic account (401
otherwise). Prints `PORT <n>` once it listens; every request is printed.
"""
import argparse
import base64
import http.server
import os
import sys
import urllib.parse


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("folder")
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--user", default="")
    ap.add_argument("--password", default="")
    args = ap.parse_args()
    root = os.path.abspath(args.folder)
    want = "Basic " + base64.b64encode(f"{args.user}:{args.password}".encode()).decode()

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *a):
            print("REQ", self.command, self.path, flush=True)

        def authorised(self):
            if not args.user:
                return True
            if self.headers.get("Authorization") == want:
                return True
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="photos"')
            self.send_header("Content-Length", "0")
            self.end_headers()
            return False

        def send_body(self, status, kind, body):
            self.send_response(status)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_PROPFIND(self):
            if not self.authorised():
                return
            length = int(self.headers.get("Content-Length") or 0)
            self.rfile.read(length)
            path = urllib.parse.unquote(self.path.split("?")[0])
            if path.rstrip("/") != "/photos":
                return self.send_body(404, "text/plain", b"no such folder")
            xml = ['<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">']
            xml.append("<d:response><d:href>/photos/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>")
            for name in sorted(os.listdir(root)):
                full = os.path.join(root, name)
                if os.path.isfile(full):
                    href = "/photos/" + urllib.parse.quote(name)
                    xml.append(f"<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>{os.path.getsize(full)}</d:getcontentlength></d:prop></d:propstat></d:response>")
            xml.append("</d:multistatus>")
            self.send_body(207, "application/xml; charset=utf-8", "".join(xml).encode())

        def do_GET(self):
            if not self.authorised():
                return
            path = urllib.parse.unquote(self.path.split("?")[0])
            name = path.removeprefix("/photos/")
            full = os.path.join(root, os.path.basename(name))
            if not path.startswith("/photos/") or not os.path.isfile(full):
                return self.send_body(404, "text/plain", b"not found")
            with open(full, "rb") as f:
                self.send_body(200, "image/jpeg" if full.endswith(".jpg") else "image/png", f.read())

    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print("PORT", server.server_address[1], flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    sys.exit(main())

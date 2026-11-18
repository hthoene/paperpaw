#!/usr/bin/env python3
"""A fake WebDAV server for the emulator run (tools/pps/file-sync.pps).

    fake_webdav.py <folder> [--port N] [--user U --password P]

Serves the files of the folder under /dav/notes/ the way Nextcloud does:
`PROPFIND` of depth 1 (the folder itself, then a response per file or folder
with its size and `ETag`), `GET`, `PUT` (answers an `ETag`), `DELETE` and
`MKCOL`. With --user the requests need that Basic account (401 otherwise).
Prints `PORT <n>` once it listens; every request is printed.
"""
import argparse
import base64
import http.server
import os
import sys
import urllib.parse

PREFIX = "/dav/notes"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("folder")
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--user", default="")
    ap.add_argument("--password", default="")
    args = ap.parse_args()
    root = os.path.abspath(args.folder)
    want = "Basic " + base64.b64encode(f"{args.user}:{args.password}".encode()).decode()

    def etag(full):
        st = os.stat(full)
        return f'"{st.st_mtime_ns:x}-{st.st_size:x}"'

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *a):
            print("REQ", self.command, self.path, flush=True)

        def authorised(self):
            if not args.user or self.headers.get("Authorization") == want:
                return True
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="notes"')
            self.send_header("Content-Length", "0")
            self.end_headers()
            return False

        def send_body(self, status, kind="text/plain", body=b"", extra=()):
            self.send_response(status)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))
            for name, value in extra:
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(body)

        def local(self):
            path = urllib.parse.unquote(self.path.split("?")[0])
            if path.rstrip("/") != PREFIX and not path.startswith(PREFIX + "/"):
                return None, None
            rel = path[len(PREFIX):].strip("/")
            if ".." in rel.split("/"):
                return None, None
            return rel, os.path.join(root, rel)

        def do_PROPFIND(self):
            if not self.authorised():
                return
            self.rfile.read(int(self.headers.get("Content-Length") or 0))
            rel, full = self.local()
            if full is None or not os.path.isdir(full):
                return self.send_body(404, body=b"no such folder")
            base = PREFIX + ("/" + urllib.parse.quote(rel) if rel else "") + "/"
            xml = ['<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">']
            xml.append(f"<d:response><d:href>{base}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>")
            for name in sorted(os.listdir(full)):
                item = os.path.join(full, name)
                href = base + urllib.parse.quote(name)
                if os.path.isdir(item):
                    xml.append(f"<d:response><d:href>{href}/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>")
                else:
                    xml.append(f"<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>{os.path.getsize(item)}</d:getcontentlength><d:getetag>{etag(item)}</d:getetag></d:prop></d:propstat></d:response>")
            xml.append("</d:multistatus>")
            self.send_body(207, "application/xml; charset=utf-8", "".join(xml).encode())

        def do_GET(self):
            if not self.authorised():
                return
            rel, full = self.local()
            if full is None or not os.path.isfile(full):
                return self.send_body(404, body=b"not found")
            with open(full, "rb") as f:
                self.send_body(200, "application/octet-stream", f.read(), [("ETag", etag(full))])

        def do_PUT(self):
            if not self.authorised():
                return
            body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
            rel, full = self.local()
            if full is None or not rel or not os.path.isdir(os.path.dirname(full)):
                return self.send_body(409, body=b"no parent folder")
            tmp = full + ".part"
            with open(tmp, "wb") as f:
                f.write(body)
            os.replace(tmp, full)
            self.send_body(201, extra=[("ETag", etag(full))])

        def do_DELETE(self):
            if not self.authorised():
                return
            rel, full = self.local()
            if full is None or not os.path.isfile(full):
                return self.send_body(404, body=b"not found")
            os.remove(full)
            self.send_body(204)

        def do_MKCOL(self):
            if not self.authorised():
                return
            rel, full = self.local()
            if full is None or not rel:
                return self.send_body(405)
            if os.path.exists(full):
                return self.send_body(405)
            if not os.path.isdir(os.path.dirname(full)):
                return self.send_body(409)
            os.mkdir(full)
            self.send_body(201)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print("PORT", server.server_address[1], flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    sys.exit(main())

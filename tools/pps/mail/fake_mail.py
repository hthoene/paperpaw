#!/usr/bin/env python3
"""A small IMAP + SMTP server for the Mail plugin (#81), both over TLS from the start.

    python3 -I tools/pps/mail/fake_mail.py [--messages N] [--starttls]

Prints, one per line, once it listens (read them from its stdout):
    IMAP <port>     the IMAP server (TLS from the start, or STARTTLS with --starttls)
    SMTP <port>     the SMTP server (same)
    PIN <hex>       SHA-256 of the server certificate's public key: the plugin's `imap-pin` / `smtp-pin`
    READY

The server holds the account user@example.org / secret-pass-123 with a mailbox of N mails (default 120,
UIDs 1..N, newest last) and a self-signed certificate made at start (the plugin pins its key: it
trusts nothing else). The mails (newest first):
    N     plain text, a reply is expected
    N-1   HTML only (headings, a list, a link, entities), converted to text by the plugin
    N-2   plain text with an EPUB attached (base64, `tiny-book.epub`)
    N-3.. filler mails "Filler mail k" (plain, seen)
Every mail sent through SMTP is kept as sent/<n>.eml in --out (default: a temporary folder, printed as
`OUT <dir>`), and `SENT <n> <rcpt>` is printed. The IMAP flags set by the plugin are printed as
`FLAG <uid> <command>`.

Only what the plugin speaks: CAPABILITY, STARTTLS, AUTHENTICATE PLAIN (with SASL-IR), LOGIN, LIST,
SELECT, STATUS, FETCH, UID FETCH BODY.PEEK[]<0.n>, UID STORE, NOOP, LOGOUT; EHLO, STARTTLS, AUTH PLAIN
and LOGIN, MAIL, RCPT, DATA, RSET, QUIT.
"""
import argparse
import base64
import email.utils
import hashlib
import io
import os
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import zipfile
from email.message import EmailMessage

USER = "user@example.org"
PASSWORD = "secret-pass-123"


def make_epub() -> bytes:
    """A real, tiny EPUB 3 with two short chapters."""
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w") as z:
        z.writestr(zipfile.ZipInfo("mimetype"), "application/epub+zip", zipfile.ZIP_STORED)
        z.writestr("META-INF/container.xml",
                   '<?xml version="1.0"?><container version="1.0" '
                   'xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles>'
                   '<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>'
                   '</rootfiles></container>', zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/content.opf",
                   '<?xml version="1.0" encoding="utf-8"?>'
                   '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
                   '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
                   '<dc:identifier id="id">urn:uuid:3d3f1c10-0000-4000-8000-000000000081</dc:identifier>'
                   '<dc:title>The Mail Test Book</dc:title><dc:creator>PaperPaw</dc:creator>'
                   '<dc:language>en</dc:language><meta property="dcterms:modified">2026-11-18T00:00:00Z</meta>'
                   '</metadata><manifest>'
                   '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
                   '<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>'
                   '<item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>'
                   '</manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>',
                   zipfile.ZIP_DEFLATED)
        page = ('<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml">'
                '<head><title>{t}</title></head><body><h1>{t}</h1><p>{p}</p></body></html>')
        z.writestr("OEBPS/nav.xhtml",
                   '<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml" '
                   'xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head><body>'
                   '<nav epub:type="toc"><ol><li><a href="c1.xhtml">Arrival</a></li>'
                   '<li><a href="c2.xhtml">Departure</a></li></ol></nav></body></html>',
                   zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/c1.xhtml", page.format(
            t="Arrival", p="This book came by mail. " * 40), zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/c2.xhtml", page.format(
            t="Departure", p="And it was opened on the reader. " * 40), zipfile.ZIP_DEFLATED)
    return out.getvalue()


def build_mailbox(n: int):
    """[(uid, flags, raw bytes)] for UIDs 1..n."""
    boxes = []
    for uid in range(1, n + 1):
        msg = EmailMessage()
        date = email.utils.format_datetime(
            email.utils.parsedate_to_datetime("Tue, 17 Nov 2026 08:00:00 +0100"), usegmt=False)
        msg["Date"] = date
        msg["Message-ID"] = f"<m{uid}@example.org>"
        msg["To"] = USER
        flags = "\\Seen"
        if uid == n:
            msg["From"] = "Alice Example <alice@example.com>"
            msg["Subject"] = "Lunch on Friday?"
            msg.set_content("Hi,\n\nare you free for lunch on Friday? I know a small place near the station.\n\n"
                            "Reply with a time that suits you.\n\nAlice\n")
            flags = ""
        elif uid == n - 1:
            msg["From"] = "Bob News <bob@example.net>"
            msg["Subject"] = "Weekly news: three things"
            msg.set_content("(no plain part)")
            msg.clear_content()
            msg.set_content(
                "<html><head><style>p {color: red}</style></head><body><h1>This week</h1>"
                "<p>Three things &amp; a link: <a href=\"https://example.net/x\">the site</a>.</p>"
                "<ul><li>First thing</li><li>Second thing</li><li>Third caf&eacute;</li></ul>"
                "<script>alert(1)</script></body></html>", subtype="html")
            flags = ""
        elif uid == n - 2:
            msg["From"] = "Carol Books <carol@example.org>"
            msg["Subject"] = "The book you asked for"
            msg.set_content("Here is the book as an attachment.\n")
            msg.add_attachment(make_epub(), maintype="application", subtype="epub+zip",
                               filename="tiny-book.epub")
            flags = ""
        else:
            msg["From"] = f"Filler <filler{uid % 7}@example.com>"
            msg["Subject"] = f"Filler mail {uid}"
            msg.set_content(f"Filler body {uid}.\n")
        boxes.append([uid, flags, msg.as_bytes(policy=msg.policy.clone(linesep="\r\n"))])
    return boxes


class Server:
    def __init__(self, args):
        self.args = args
        self.mailbox = build_mailbox(args.messages)
        self.lock = threading.Lock()
        self.sent = 0
        self.out = args.out or tempfile.mkdtemp(prefix="fake-mail-")
        os.makedirs(os.path.join(self.out, "sent"), exist_ok=True)
        cert, key = os.path.join(self.out, "cert.pem"), os.path.join(self.out, "key.pem")
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key,
                        "-out", cert, "-days", "30", "-subj", "/CN=localhost",
                        "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1"],
                       check=True, capture_output=True)
        pub = subprocess.run(["openssl", "x509", "-in", cert, "-pubkey", "-noout"],
                             check=True, capture_output=True).stdout
        der = subprocess.run(["openssl", "pkey", "-pubin", "-outform", "DER"], input=pub,
                             check=True, capture_output=True).stdout
        self.pin = hashlib.sha256(der).hexdigest()
        self.ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.ctx.load_cert_chain(cert, key)

    def listen(self, handler):
        s = socket.socket()
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind(("127.0.0.1", 0))
        s.listen(8)

        def loop():
            while True:
                c, _ = s.accept()
                threading.Thread(target=self.safely, args=(handler, c), daemon=True).start()
        threading.Thread(target=loop, daemon=True).start()
        return s.getsockname()[1]

    def safely(self, handler, conn):
        print(f"CONNECT {handler.__name__}", flush=True)
        try:
            handler(conn)
        except (OSError, ssl.SSLError, ConnectionError) as error:
            print(f"ENDED {handler.__name__} {error!r}", flush=True)
        finally:
            try:
                conn.close()
            except OSError:
                pass

    def start_tls(self, conn):
        return self.ctx.wrap_socket(conn, server_side=True)

    # ---- IMAP ----
    def imap(self, conn):
        if not self.args.starttls:
            conn = self.start_tls(conn)
        f = conn.makefile("rwb", buffering=0)
        print("IMAP TLS ready", flush=True)
        secure = not self.args.starttls
        caps = "IMAP4rev1 AUTH=PLAIN SASL-IR" if secure else "IMAP4rev1 STARTTLS LOGINDISABLED"
        f.write(f"* OK [CAPABILITY {caps}] fake mail ready\r\n".encode())
        authed, selected = False, False
        while True:
            line = f.readline()
            if not line:
                return
            line = line.rstrip(b"\r\n").decode("utf-8", "replace")
            tag, _, rest = line.partition(" ")
            up = rest.upper()
            print(f"IMAP {up.split(' ')[0]} {up.split(' ')[1] if up.startswith('UID') else ''}".rstrip(), flush=True)
            if up == "CAPABILITY":
                caps = "IMAP4rev1 AUTH=PLAIN SASL-IR" if secure else "IMAP4rev1 STARTTLS LOGINDISABLED"
                f.write(f"* CAPABILITY {caps}\r\n{tag} OK done\r\n".encode())
            elif up == "STARTTLS" and not secure:
                f.write(f"{tag} OK begin TLS\r\n".encode())
                conn = self.start_tls(conn)
                f = conn.makefile("rwb", buffering=0)
                secure = True
            elif up.startswith("AUTHENTICATE PLAIN"):
                arg = rest[len("AUTHENTICATE PLAIN"):].strip()
                if not arg:
                    f.write(b"+ \r\n")
                    arg = f.readline().strip().decode()
                authed = base64.b64decode(arg) == f"\0{USER}\0{PASSWORD}".encode()
                f.write((f"{tag} OK authenticated\r\n" if authed
                         else f"{tag} NO [AUTHENTICATIONFAILED] bad credentials\r\n").encode())
            elif up.startswith("LOGIN "):
                authed = secure and f'"{PASSWORD}"' in rest and USER in rest
                f.write((f"{tag} OK logged in\r\n" if authed else f"{tag} NO login failed\r\n").encode())
            elif not authed:
                f.write(f"{tag} NO not authenticated\r\n".encode())
            elif up.startswith("LIST"):
                f.write(f'* LIST (\\HasNoChildren) "/" "INBOX"\r\n'
                        f'* LIST (\\HasNoChildren \\Sent) "/" "Sent"\r\n{tag} OK done\r\n'.encode())
            elif up.startswith("SELECT"):
                selected = True
                f.write(f"* {len(self.mailbox)} EXISTS\r\n* OK [UIDVALIDITY 77] ok\r\n"
                        f"{tag} OK [READ-WRITE] done\r\n".encode())
            elif up.startswith("STATUS"):
                with self.lock:
                    unseen = sum(1 for m in self.mailbox if "\\Seen" not in m[1])
                f.write(f'* STATUS "INBOX" (MESSAGES {len(self.mailbox)} UNSEEN {unseen})\r\n'
                        f"{tag} OK done\r\n".encode())
            elif up.startswith("FETCH ") and selected:
                a, b = (int(x) for x in rest[6:].split(" ")[0].split(":"))
                out = b""
                with self.lock:
                    for i, (uid, flags, raw) in enumerate(self.mailbox, 1):
                        if a <= i <= b:
                            head = raw.split(b"\r\n\r\n")[0] + b"\r\n\r\n"
                            out += (f'* {i} FETCH (UID {uid} FLAGS ({flags}) INTERNALDATE '
                                    f'"17-Nov-2026 08:00:00 +0100" RFC822.SIZE {len(raw)} '
                                    f'BODY[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID)] '
                                    f'{{{len(head)}}}\r\n').encode() + head + b")\r\n"
                f.write(out + f"{tag} OK done\r\n".encode())
            elif up.startswith("UID FETCH"):
                words = rest.split(" ")
                uid = int(words[2])
                limit = 1 << 30
                if "<0." in rest:
                    limit = int(rest.split("<0.")[1].rstrip(">"))
                with self.lock:
                    raw = next((m[2] for m in self.mailbox if m[0] == uid), None)
                if raw is None:
                    f.write(f"{tag} OK nothing\r\n".encode())
                else:
                    body = raw[:limit]
                    f.write(f"* 1 FETCH (UID {uid} BODY[]<0> {{{len(body)}}}\r\n".encode()
                            + body + f")\r\n{tag} OK done\r\n".encode())
            elif up.startswith("UID STORE"):
                words = rest.split(" ")
                uid = int(words[2])
                on = words[3].startswith("+")
                with self.lock:
                    for m in self.mailbox:
                        if m[0] == uid:
                            m[1] = m[1].replace("\\Flagged", "").replace("\\Seen", "")
                            if on:
                                m[1] += words[4].strip("()")
                print(f"FLAG {uid} {rest}", flush=True)
                f.write(f"{tag} OK stored\r\n".encode())
            elif up == "NOOP":
                f.write(f"{tag} OK noop\r\n".encode())
            elif up == "LOGOUT":
                f.write(f"* BYE bye\r\n{tag} OK logged out\r\n".encode())
                return
            else:
                f.write(f"{tag} BAD unknown\r\n".encode())

    # ---- SMTP ----
    def smtp(self, conn):
        if not self.args.starttls:
            conn = self.start_tls(conn)
        f = conn.makefile("rwb", buffering=0)
        secure = not self.args.starttls
        f.write(b"220 fake mail ESMTP\r\n")
        authed, rcpts, sender = False, [], ""
        while True:
            line = f.readline()
            if not line:
                return
            line = line.rstrip(b"\r\n").decode("utf-8", "replace")
            up = line.upper()
            if up.startswith("EHLO") or up.startswith("HELO"):
                ext = ["250-fake.example.org"]
                if not secure:
                    ext.append("250-STARTTLS")
                ext += ["250-SIZE 10485760", "250 AUTH PLAIN LOGIN"]
                f.write(("\r\n".join(ext) + "\r\n").encode())
            elif up == "STARTTLS" and not secure:
                f.write(b"220 go ahead\r\n")
                conn = self.start_tls(conn)
                f = conn.makefile("rwb", buffering=0)
                secure = True
            elif up.startswith("AUTH PLAIN"):
                arg = line[10:].strip()
                if not arg:
                    f.write(b"334 \r\n")
                    arg = f.readline().strip().decode()
                authed = base64.b64decode(arg) == f"\0{USER}\0{PASSWORD}".encode()
                f.write(b"235 ok\r\n" if authed else b"535 5.7.8 bad credentials\r\n")
            elif up == "AUTH LOGIN":
                f.write(b"334 VXNlcm5hbWU6\r\n")
                user = base64.b64decode(f.readline().strip()).decode()
                f.write(b"334 UGFzc3dvcmQ6\r\n")
                pw = base64.b64decode(f.readline().strip()).decode()
                authed = (user, pw) == (USER, PASSWORD)
                f.write(b"235 ok\r\n" if authed else b"535 bad\r\n")
            elif not authed and (up.startswith("MAIL") or up.startswith("RCPT") or up == "DATA"):
                f.write(b"530 5.7.0 authentication required\r\n")
            elif up.startswith("MAIL FROM:"):
                sender, rcpts = line[10:].split(" ")[0].strip("<>"), []
                f.write(b"250 ok\r\n")
            elif up.startswith("RCPT TO:"):
                rcpts.append(line[8:].split(" ")[0].strip("<>"))
                f.write(b"250 ok\r\n")
            elif up == "DATA":
                f.write(b"354 end with <CRLF>.<CRLF>\r\n")
                data = b""
                while True:
                    row = f.readline()
                    if row == b".\r\n" or not row:
                        break
                    data += row[1:] if row.startswith(b"..") else row
                with self.lock:
                    self.sent += 1
                    n = self.sent
                with open(os.path.join(self.out, "sent", f"{n}.eml"), "wb") as out:
                    out.write(data)
                print(f"SENT {n} {','.join(rcpts)} from {sender}", flush=True)
                f.write(b"250 2.0.0 queued\r\n")
            elif up == "RSET" or up == "NOOP":
                f.write(b"250 ok\r\n")
            elif up == "QUIT":
                f.write(b"221 bye\r\n")
                return
            else:
                f.write(b"500 unknown\r\n")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--messages", type=int, default=120)
    p.add_argument("--starttls", action="store_true", help="STARTTLS instead of TLS from the start")
    p.add_argument("--out", help="folder for the certificate and the sent mails")
    server = Server(p.parse_args())
    print(f"OUT {server.out}", flush=True)
    print(f"IMAP {server.listen(server.imap)}", flush=True)
    print(f"SMTP {server.listen(server.smtp)}", flush=True)
    print(f"PIN {server.pin}", flush=True)
    print("READY", flush=True)
    threading.Event().wait()


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
r"""The client of the developer link (docs/developer-link.md): a computer on the
USB cable of an Xteink device presses keys, takes screenshots and reads and
writes books and plugin files, once the reader has allowed it on the device
(Customise > Developer > USB developer tools).

Usage:
  python tools/x4-dev.py [--port /dev/ttyACM0 | rfc2217://host:4001?ign_set_control] COMMAND...

Commands (one or several; each is one line of the protocol):
  hello | ping | screen | heap | describe | close
  key confirm            (page-forward page-back up down left right confirm back menu home)
  run core.sleep         (an action, as a button bound to it would)
  shot out.pgm           (the screen, as a PGM picture)
  ls books  |  ls /
  get books/a.epub out.epub  |  put local.epub books/new.epub  |  md5 books/a.epub
  rm plugins/demo/new.txt   (only what the link wrote itself)

The device refuses what the rules do not allow (`err denied`); it says `err off`
while the reader has not allowed the link. Needs pyserial.
"""

import argparse
import hashlib
import sys
import time

import serial

PREFIX = "DEV/1 "


def open_port(name):
    port = serial.serial_for_url(name, do_not_open=True)
    port.timeout = 0.5
    # Without DTR and RTS, opening the port does not reset the chip.
    port.dtr = False
    port.rts = False
    port.open()
    return port


class Link:
    def __init__(self, port):
        self.port = port
        self.n = 0

    def send(self, text):
        self.n += 1
        self.port.write(f"{PREFIX}{self.n} {text}\n".encode())
        return self.n

    def lines(self, n, timeout=30, since=None):
        """The answers to command n (or any from `since` on), up to an `ok`/`err`
        (other lines are the log)."""
        end = time.time() + timeout
        while time.time() < end:
            raw = self.port.readline().decode(errors="replace").strip()
            if not raw.startswith(PREFIX):
                if raw:
                    print(raw, file=sys.stderr)
                continue
            number, _, rest = raw[len(PREFIX):].partition(" ")
            if number == "0":
                print(f"[device] {rest}", file=sys.stderr)
                continue
            if number != str(n) and not (since and number.isdigit() and int(number) >= since):
                continue
            yield rest
            if rest == "ok" or rest.startswith(("ok ", "err ")):
                return
        raise SystemExit("no answer")

    def ask(self, text):
        out = list(self.lines(self.send(text)))
        if out[-1].startswith("err "):
            raise SystemExit(f"{text}: {out[-1]}")
        return out


def shot(link, path):
    rows, width, height, bits = {}, 0, 0, 8
    for line in link.ask("shot"):
        words = line.split(" ")
        if words[0] == "frame":
            width, height, bits = int(words[1]), int(words[2]), int(words[3])
        elif words[0] == "row":
            data = bytearray()
            digits = words[2]
            for i in range(0, len(digits), 4):
                data += bytes([int(digits[i + 2 : i + 4], 16)]) * int(digits[i : i + 2], 16)
            rows[int(words[1])] = bytes(data)
    with open(path, "wb") as out:
        out.write(f"P5\n{width} {height}\n255\n".encode())
        for y in range(height):
            out.write(rows.get(y, b"\xff" * width)[:width].ljust(width, b"\xff"))
    print(f"{path}: {width}x{height}, {bits} bits per pixel")


def get(link, remote, local):
    data = bytearray()
    for line in link.ask(f"get {remote}"):
        if line.startswith("data "):
            data += bytes.fromhex(line[5:])
    with open(local, "wb") as out:
        out.write(data)
    print(f"{local}: {len(data)} bytes md5 {hashlib.md5(data).hexdigest()}")


def put(link, local, remote):
    data = open(local, "rb").read()
    answer = link.ask(f"put {len(data)} {remote}")
    assert answer == ["ready"], answer
    first = link.n + 1
    for at in range(0, len(data), 512):
        link.send(f"data {data[at:at + 512].hex()}")
    for line in link.lines(link.n, since=first):
        if line.startswith("err "):
            raise SystemExit(f"put {remote}: {line}")
        print(line)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--port", required=True)
    parser.add_argument("command", nargs="+")
    args = parser.parse_args()
    link = Link(open_port(args.port))
    words = args.command
    verb = words[0]
    if verb == "shot":
        shot(link, words[1])
    elif verb == "get":
        get(link, words[1], words[2])
    elif verb == "put":
        put(link, words[1], words[2])
    else:
        for line in link.ask(" ".join(words)):
            print(line)


if __name__ == "__main__":
    main()

r"""Drives PaperPaw on an Xteink X4 over USB: presses its buttons, takes
screenshots and tells what the screen shows, as the emulator's scripts do
(see shells/desktop/src/headless.rs). The commands are described in
crates/paperpaw-x4/src/remote.rs, the firmware's side is in
shells/xteink/src/remote.rs (its `remote` feature: released firmware
leaves it out).

Usage:
  python tools/x4-remote.py [--port COM10] [--script STEPS] [--screenshot FILE]
  python tools/x4-remote.py --log 30     (the X4's log for 30 seconds)
  python tools/x4-remote.py --put book.epub books/book.epub   (a file onto its card,
      checked by its MD5 once there)
  python tools/x4-remote.py --get .paperpaw/plugins/battery/log.csv log.csv   (one off it)
  python tools/x4-remote.py --push fonts/ .paperpaw/fonts   (a folder, all in it)
  python tools/x4-remote.py --pull .paperpaw/plugins/stats stats/   (a folder off it)
  python tools/x4-remote.py --ls books   --cat paperpaw.ini   --rm updates/old.bin
  python tools/x4-remote.py --mkdir books/new   --mv books/a.epub books/b.epub
  python tools/x4-remote.py --trust-code hello   (one's own build of a code plugin,
      copied into plugins/hello by hand, named as installed: only code installed runs)
  python tools/x4-remote.py --info     (version, plugins, native code, the screens open, the time)
  python tools/x4-remote.py --crashes  (the crash file, its calls named from the
      firmware's symbols: the build that crashed must be the one built here)
  python tools/x4-remote.py --profile --script "page-forward,wait:2000"
      (where the time goes during the steps: the functions the processor
      was in most, from the firmware's symbols; needs rustup's llvm-tools)
  python tools/x4-remote.py --script-file test.pps --out out/run1 --baseline out/run0/results.json
      (a script file, with measurements, checks and a report; see "The script
      language" below and docs/device-test-scripts.md)
  python tools/x4-remote.py --emulator target/debug/paperpaw-emu --library card/ --script-file test.pps
      (the same file on the emulator: write and debug a test there, run it
      unchanged on the X4)

STEPS, separated by commas:
  page-forward       a short press of a button (back, confirm, left, right,
                     page-back, page-forward, power)
  long:confirm       a long press
  hold:confirm:2000  a press held so many milliseconds
  open:books/a.epub  open a book in the reader
  run:calibre.open   run an action, as a button bound to it would (e.g. a
                     tool's, to open it at once)
  slot:core.settings:id=reader   open a slot, with parameters (key=value)
  show:calibre.screen   open a screen by its id (also :key=value …)
  set:core.night-mode=on   set a setting, as the settings screen would (the
                     value as configuration files write it; `\,` for a
                     comma in it, e.g. a home page's widgets)
  setting:reader.font-size print a setting's value
  type:hello world   text into the text field shown (the keyboard's), as if
                     typed on its keys (`\,` for a comma)
  type-enter:hello   the same, then confirmed (`type-enter:` confirms only)
  wait:500           500 ms pass
  screen             print the screen's id
  describe           print what the screen shows, part by part
  heap               print the heap used and free, and the stack never used, in bytes
  gray               four grays on the panel to judge them (bars, a gradient)
  gray:.paperpaw/…/cover.gray   with a decoded gray picture below
  graytone:middle    how dark the X4's grays between come out from now on
                     (stock, middle), to compare them
  refresh:full       refresh the panel from what the controller holds (also
                     refresh:fast), nothing sent
  input              print how the buttons and the battery are read
  shot:out/a.png     a screenshot now

  Waiting and checking (a script with a failed check ends with exit code 1):
  until:Settings     wait until the screen shows this text (at most 30 s)
  until:Settings:5000  at most 5 s
  expect:Settings    check that the screen shows this text now
  expect-not:Error   check that it does not
  screens            print the screens open, the one shown last
  boot               why the X4 last started, and the panic before if any

  Touch (the X4 Pro), where PaperPaw draws:
  tap:240:400        a tap there
  swipe:240:700:240:100   a finger from one point to the other (also :ms)
  drag:240:700:240:100:40:15  the same as the touch controller reads it
  clicks:right:30:800:100  a button clicked 30 times 800 ms apart, held
                     100 ms each, read as its own readings are
                     (40 moves 15 ms apart, also while PaperPaw is busy)
  tap-label:Search online  a tap in the middle of what shows that text

  The card (paths as PaperPaw writes them, from the card's top):
  ls:books  stat:books/a.epub  md5:books/a.epub  rm:updates  mkdir:books/new
  mv:books/a.epub>books/b.epub   sync   cat:paperpaw.ini

  What runs, and changes:
  version            the firmware's version and the plugin crates built in
  plugins            every plugin, on or off, with its version (and whether it
                     has native code, and why that does not run)
  native             where native code plugins lie, or why there is no room
  actions:reader.    the actions (those beginning so; all without)
  settings:reader.   the settings and their values
  usage              what each plugin took since start: time, calls,
                     refreshes, bytes read and written (busiest first)
  time               the clock; time:set sets it to this computer's
  log:debug          how much the log says (off, error, warn, info, debug, trace)
  wifi:status  wifi:scan  wifi:off   the radio (join with --wifi-connect)
  http:get:http://host/file>books/f.epub   a plain-HTTP request as plugins make
                     them, into memory or a file, with its time and the heap
  emit:core.book-opened:path=books/a.epub   an event, as a plugin sends it
  restart            the X4 starts again (waits until it is back)
  nap:3              one light sleep that long, then a new start; a failed check
                     unless `boot` says the nap was survived (place 10, not 11)
  panic:why          a panic, to try what a crash leaves (waits until back)

  Also: wifi-connect:NAME:PASSWORD (`${env:WIFI_PASSWORD}` keeps it out of
  the script and the logs)  reset:40 (hard reset, its boot measured)
  crashes (count of the crash file)  chip:esp32c3  flash:IMAGE:chip=esp32c3

The script language (`.pps` files, or `--script "a,b,c"`; steps above, and):
  Files: a step a line, `#` comments, `include:other.pps`. In `--script`, steps
  are told apart by commas and newlines (`\,` is a comma, `\{` `\}` braces).
  Variables: `let:book=books/a.epub`, `calc:n=$n+1`, `$book`, `${book}`,
  `${env:NAME}`, `--var name=value`; `$1`..`$9` in a `call`. Predefined:
  $target (x4, emu), $out, $date, $run.
  Blocks (`{` ends the header line, `}` closes):
    repeat:5 {…}   repeat:5:k {…} ($k counts from 1)   retry:3 {…} (again while a
    check in it fails)   try {…} (errors of steps are ignored)
    for:f in ls:books/*.epub {…}  also list:a|b|c  range:1..5  local:glob;
      $f, ${f.name}, ${f.stem}, ${f.dir}, ${f.ext}
    if:shows:Text {…} else {…}   also not-shows:T  log:RE  has:METRIC  exists:PATH
      and comparisons, `if:$n>=3`
    define:turn {…}  and  call:turn:arg1:arg2
    test:04-turns {…}   a point with its own verdict: PASS, FAIL, or NOT TESTABLE
      after `skip:why`; a failing step fails the test, the next one runs
    measure:turn:n=10:warm=1:settle=4000:gap=0[:heap=on][:usage=on] {…}   the steps
      n times, each followed by `settle` ms of watching the log; per run it records
      first_ms, last_ms (key line -> first/last frame line, host time), frames,
      grays, unprompted (frames 3 s after the last key), warns, wall_ms, drawn_ms,
      shown_ms, action_ms (`run:` lines), card_reads/card_writes (hoststat lines, when
      the build has them), hw_block (high-water lines), and with heap=on
      heap_used/free/block/peak, with usage=on use_read_kb, use_written_kb, use_refresh_*.
      Asking the device costs it time, so heap and usage are off unless wanted.
  Checks (a failed one makes the exit code 1; a step that errors outright ends a
  test, or the script, with the exit code 2 and a STOPPED file in --out):
    expect:Text  expect-not:Text  until:Text[:ms]  expect-focus:Text   on the screen
    focus:Text[:max]  select:Text   press `right` until that row/button is focused (then confirm)
    expect-log:REGEX[:ms]  no-log:REGEX  until-log:REGEX[:ms]  no-warn  known-warn:REGEX
      on the log lines since `mark` (a `test:` or `mark` starts anew)
    assert:turn.first_ms.median<=1000[:why]   METRIC[.STAT] OP NUMBER; OP one of
      <= >= < > == !=; STAT: median (default) min max mean p90 n last sum; the number may
      be baseline, baseline*1.1, baseline+50 (the same metric in --baseline; no value: SKIP)
    fill:PATH:SIZE:SEED  grow:PATH:ADD:SEED  check-file:PATH:SIZE:SEED   a card file of known bytes (saved or replaced;
      extended, whole rewrite; checked by MD5); used by tools/pps/soak/
    record:name=5  note:name=text  heap (also records heap.used/free/block/peak/stack)
    usage  clock (records clock.known, clock.skew_s)  fail:why  skip:why  abort:why  on-fail:stop
  On the emulator a step it cannot do says SKIP; so do asserts on its times and gray
  passes, which only the X4 has.
  Robustness: a lost link is opened again (--retries); asking steps are asked again, a
  key press is not repeated (the check says so). `flash:` reads the chip first and
  never writes a different one than `chip=`; `--esptool` names the command.
  Output (--out DIR): results.json (tests, checks, every metric's samples), report.md
  (tests, measurements with before -> now, checks, new WARN lines), device.log, STOPPED.

Needs pyserial (pip install pyserial). Without --port, the X4 is found by
its USB id.
"""

import argparse
import bisect
import getpass
import glob
import hashlib
import importlib.util
import json
import os
import queue
import re
import shlex
import struct
import subprocess
import sys
import threading
import time
import zlib
from pathlib import Path

import serial
from serial.tools import list_ports

# The script language's engine, next to this file (`python -I` does not look there).
if "ppscript" not in sys.modules:
    _spec = importlib.util.spec_from_file_location("ppscript", Path(__file__).resolve().with_name("ppscript.py"))
    sys.modules["ppscript"] = importlib.util.module_from_spec(_spec)
    _spec.loader.exec_module(sys.modules["ppscript"])
ppscript = sys.modules["ppscript"]
LinkError, StepError, ScriptError = ppscript.LinkError, ppscript.StepError, ppscript.ScriptError
ANSI = re.compile(r"\x1b\[[0-9;]*m")

# Espressif's USB serial and JTAG unit, built into the X4's ESP32-C3.
USB_ID = (0x303A, 0x1001)
# As the emulator: comfortably above the long-press threshold.
LONG_MS = 1200
# A command that takes longer has failed.
TIMEOUT_S = 60
# Removing a large folder (e.g. the books' caches) or a large download
# takes minutes.
REMOVE_TIMEOUT_S = 900


def find_port():
    for port in list_ports.comports():
        if (port.vid, port.pid) == USB_ID:
            return port.device
    sys.exit("No X4 found on USB; name its port with --port.")


def open_port(name, tries=None):
    # A device name (COM10, /dev/ttyACM0) or a URL (rfc2217://host:4001?ign_set_control).
    # An RFC2217 bridge sometimes takes the connection and then dies in the handshake
    # ("reader thread died", "timeout while waiting for option 'purge'"): a new
    # connection a moment later usually works, so a URL is tried a few times.
    if tries is None:
        tries = 5 if name.startswith("rfc2217://") else 1
    for attempt in range(1, tries + 1):
        port = serial.serial_for_url(name, do_not_open=True)
        port.timeout = 0.2
        # Without DTR and RTS, opening the port does not reset the chip.
        port.dtr = False
        port.rts = False
        try:
            port.open()
            return port
        except (serial.SerialException, OSError):
            try:
                port.close()
            except (serial.SerialException, OSError):
                pass
            if attempt == tries:
                raise
            time.sleep(3)


def plugin_digest(data):
    """The stored digest in a supported native plugin header (formats 2 to 5; in format 5 the last 32 bytes of its 96-byte header)."""
    if len(data) < 6 or data[:4] != b"PPNP":
        raise ValueError("not a plugin file")
    format_version = int.from_bytes(data[4:6], "little")
    digest_at = {2: 48, 3: 60, 4: 84, 5: 64}.get(format_version)
    if digest_at is None:
        raise ValueError(f"unsupported plugin format {format_version}")
    if len(data) < digest_at + 32:
        raise ValueError("truncated plugin header")
    return data[digest_at:digest_at + 32]


class X4:
    def __init__(self, port):
        self.port = port
        self.name = port.port
        self.buffer = b""
        # The X4's log is printed, too.
        self.verbose = False
        # Checks of the script that did not hold.
        self.failed = []
        # Every log line (not the `PP:` answers) with the host time it was read at:
        # what measurements and log checks of the scripts look at.
        self.logbuf = []
        self.last_heap = None
        self.last_usage = None
        self.reset_log = []

    def line(self, deadline):
        """The next line from the X4, or None once the deadline passed."""
        while b"\n" not in self.buffer:
            if time.monotonic() > deadline:
                return None
            try:
                self.buffer += self.port.read(self.port.in_waiting or 1)
            except (serial.SerialException, OSError) as error:
                raise LinkError(f"link lost: {error}") from error
        line, self.buffer = self.buffer.split(b"\n", 1)
        text = line.decode("utf-8", "replace").rstrip("\r")
        if not text.startswith("PP:"):
            self.logbuf.append((time.monotonic(), ANSI.sub("", text)))
        return text

    def drain(self):
        """Reads what the X4 said meanwhile (into the log buffer; printed with -v)."""
        deadline = time.monotonic() + 0.02
        while (line := self.line(deadline)) is not None:
            if self.verbose and not line.startswith("PP:"):
                print(line)

    def reopen(self):
        """The port opened again (after a flash, or a lost link)."""
        try:
            self.port.close()
        except (serial.SerialException, OSError):
            pass
        self.port = open_port(self.name)
        self.buffer = b""

    def reconnect(self, wait=40):
        """Opens the link again after it was lost (the RFC2217 server or the cable hiccuped)
        and checks that PaperPaw answers."""
        deadline = time.monotonic() + wait
        while True:
            try:
                self.reopen()
                self.ask("screen", "SCREEN", 10)
                return
            except (serial.SerialException, OSError, LinkError) as error:
                if time.monotonic() > deadline:
                    raise LinkError(f"no link again in {wait} s: {error}") from error
                time.sleep(2)

    def hard_reset(self, seconds):
        """Resets the chip (RTS) and logs its start for `seconds`: `reset_log` holds the lines,
        timed from the reset."""
        self.port.dtr = False
        self.port.rts = True
        time.sleep(0.2)
        self.port.rts = False
        self.buffer = b""
        start, t0 = len(self.logbuf), time.monotonic()
        while (line := self.line(t0 + seconds)) is not None:
            if self.verbose and not line.startswith("PP:"):
                print(line)
        self.reset_log = [(t - t0, text) for t, text in self.logbuf[start:]]
        self.reconnect(30)

    def heap_values(self):
        _, used, free, *more = self.ask("heap", "HEAP")[-1].split()
        got = {"used": int(used), "free": int(free), "stack": None, "block": int(free), "peak": 0}
        if more and int(more[1]):
            got["stack"] = int(more[0])
        if len(more) > 2:
            got["block"] = int(more[2])
        if len(more) > 3:
            got["peak"] = int(more[3])
        self.last_heap = got
        return got

    def usage_totals(self):
        return self.parse_usage(self.ask("usage", "END")[:-1])

    def parse_usage(self, answers):
        f, p, u, _pixels, _inputs, _sleeps, panel = (int(v) for v in answers[0].split()[1:])
        rows = [a.split()[1:] for a in answers[1:]]
        got = {"refresh_fast": f, "refresh_partial": p, "refresh_full": u, "panel_ms": panel,
               "read_kb": sum(int(r[7]) for r in rows) / 1000, "written_kb": sum(int(r[8]) for r in rows) / 1000,
               "busy_ms": sum(int(r[1]) for r in rows) / 1000}
        self.last_usage = got
        return got

    def pass_time(self, seconds):
        """Lets `seconds` go by, reading the X4's log meanwhile (kept for the script's
        measurements, with the time each line came; printed with `verbose`)."""
        deadline = time.monotonic() + seconds
        while (line := self.line(deadline)) is not None:
            if self.verbose and not line.startswith("PP:"):
                print(line)

    def ask(self, command, until, timeout=TIMEOUT_S):
        """Sends a command and returns the X4's answers (lines with "PP:")
        up to the one starting with `until`."""
        # What the log said since the last command, before it goes.
        self.drain()
        try:
            self.port.reset_input_buffer()
            self.buffer = b""
            self.port.write(command.encode() + b"\n")
        except (serial.SerialException, OSError) as error:
            raise LinkError(f"{command}: link lost: {error}") from error
        return self.expect(command, until, timeout)

    def expect(self, command, until, timeout=TIMEOUT_S):
        """The X4's answers to `command` up to the one starting with `until`."""
        answers = []
        deadline = time.monotonic() + timeout
        while True:
            line = self.line(deadline)
            if line is None:
                raise LinkError(f"{command}: no answer from the X4")
            if not line.startswith("PP:"):
                if self.verbose:
                    print(line)
                continue
            answer = line[3:]
            if answer.startswith(("ERR", "BUSY")):
                raise StepError(f"{command}: {answer}")
            answers.append(answer)
            if answer.startswith(until):
                return answers

    def put(self, local, path):
        """Copies the file `local` to `path` on the X4's card, and checks it
        there by its MD5."""
        data = Path(local).read_bytes()
        started = time.monotonic()
        for attempt in (1, 2, 3):
            try:
                self.write(path, data)
                break
            except (serial.SerialException, OSError, LinkError):
                # The link died during the copy: open it again and send the file once more.
                if attempt == 3:
                    raise
                self.reconnect(40)
        seconds = time.monotonic() - started
        print(f"{path}: {len(data) // 1000} KB in {seconds:.1f} s, checked")

    def get(self, path, local):
        """Copies the file `path` on the X4's card to `local`."""
        data = self.read(path)
        Path(local).write_bytes(data)
        print(f"{path}: {len(data)} bytes to {local}")

    def trust_code(self, folder):
        """Names the code in plugins/<folder> as installed, as the store and
        the web flasher do (.paperpaw/plugin-code.txt): PaperPaw runs only
        code installed, and this is for one's own build copied there by
        hand. Its header's SHA-256 is what is kept, by the plugin's id (its
        plugin.ini's, else the folder's name)."""
        try:
            digest = plugin_digest(self.read(f"plugins/{folder}/code.ppn"))
        except ValueError as error:
            raise StepError(f"plugins/{folder}/code.ppn: {error}") from error
        ini = self.read(f"plugins/{folder}/plugin.ini").decode("utf-8", "replace")
        ids = re.findall(r"(?m)^\s*id\s*=\s*(\S+)", ini)
        plugin = ids[0] if ids else folder
        path = ".paperpaw/plugin-code.txt"
        kept = ""
        if self.ask(f"stat {path}", "STAT")[-1].split()[1] == "f":
            kept = self.read(path).decode()
        lines = [line for line in kept.splitlines() if line.split("\t")[0] != plugin]
        lines.append(f"{plugin}\t{digest.hex()}")
        self.write(path, "".join(f"{line}\n" for line in lines).encode())
        print(f"plugins/{folder}/code.ppn: named as installed for {plugin} ({digest[:8].hex()}...)")

    def write(self, path, data):
        """Writes `data` to `path` on the X4's card, checked by its MD5."""
        self.ask(f"put {len(data)} {path}", "READY")
        for at in range(0, len(data), 512):
            self.port.write(b"data " + data[at : at + 512].hex().encode() + b"\n")
        self.expect(f"put {path}", "OK")
        there = self.ask(f"md5 {path}", "MD5")[-1].split()
        if there[1] != hashlib.md5(data).hexdigest() or int(there[2]) != len(data):
            raise StepError(f"{path}: arrived changed (MD5 {there[1]}, {there[2]} bytes)")

    def read(self, path):
        """The bytes of the file `path` on the X4's card."""
        answers = self.ask(f"get {path}", "END")
        return b"".join(bytes.fromhex(a.split(" ", 1)[1]) for a in answers if a.startswith("DATA "))

    def listing(self, path):
        """The entries of the folder `path`: (folder?, size, name)."""
        entries = []
        for answer in self.ask(f"ls {path}", "END")[:-1]:
            _, kind, size, name = answer.split(" ", 3)
            entries.append((kind == "d", int(size), name))
        return entries

    def push(self, local, path):
        """Copies the folder `local` and all in it to `path` on the card."""
        root = Path(local)
        self.ask(f"mkdir {path}", "OK")
        for item in sorted(root.rglob("*")):
            target = f"{path}/{item.relative_to(root).as_posix()}"
            if item.is_dir():
                self.ask(f"mkdir {target}", "OK")
            else:
                self.put(item, target)

    def pull(self, path, local):
        """Copies the folder `path` on the card and all in it to `local`."""
        Path(local).mkdir(parents=True, exist_ok=True)
        for is_dir, _, name in self.listing(path):
            there = f"{path}/{name}" if path else name
            if is_dir:
                self.pull(there, Path(local) / name)
            else:
                self.get(there, Path(local) / name)

    def boot(self):
        """Prints why the X4 last started, and the panic of the run
        before if it panicked, with its calls named. Returns the BOOT
        line."""
        line = self.ask("boot", "BOOT")[-1]
        print(line)
        deadline = time.monotonic() + 0.5
        while (line := self.line(deadline)) is not None:
            if line.startswith("PP:NAPS"):
                print(line[3:])
            if line.startswith("PP:PANIC"):
                print(line[3:])
                for name in calls(line):
                    print(f"    {name}")
        return line

    def parts(self):
        """What the screen shows: (role, x, y, width, height, state, label)."""
        parts = []
        for answer in self.ask("describe", "END")[:-1]:
            _, role, x, y, w, h, state, *label = answer.split(" ")
            parts.append((role, int(x), int(y), int(w), int(h), state, " ".join(label)))
        return parts

    def shows(self, text):
        return any(text in part[6] for part in self.parts())

    def touch(self, phase, x, y):
        self.ask(f"touch {phase} {x} {y}", "OK")

    def swipe(self, x1, y1, x2, y2, ms=300):
        """A finger from one point to the other, in steps."""
        self.touch("down", x1, y1)
        steps = max(1, ms // 30)
        for i in range(1, steps + 1):
            time.sleep(ms / 1000 / steps)
            self.touch("move", x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)
        self.touch("up", x2, y2)

    def back_again(self, command, until):
        """Sends a command after which the X4 starts again, and waits until
        it answers once more. Returns the BOOT line it then reports."""
        self.port.write(command.encode() + b"\n")
        time.sleep(1)
        self.port.close()
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            time.sleep(1)
            try:
                self.port = open_port(self.name)
                self.buffer = b""
                return self.boot()
            except (serial.SerialException, OSError, LinkError, StepError):
                try:
                    self.port.close()
                except serial.SerialException:
                    pass
        raise LinkError(f"{command}: the X4 did not come back in 40 s")

    def press(self, button, ms):
        self.ask(f"hold {button} {ms}", "OK")

    def screenshot(self, path):
        answers = self.ask("shot", "END")
        _, width, height, bits = answers[0].split()
        width, height, bits = int(width), max(int(height), len(answers) - 2), int(bits)
        rows = {}
        for answer in answers[1:-1]:
            _, y, runs = answer.split(" ", 2) if answer.count(" ") >= 2 else (*answer.split(), "")
            data = bytearray()
            for i in range(0, len(runs), 4):
                data += bytes([int(runs[i + 2 : i + 4], 16)]) * int(runs[i : i + 2], 16)
            rows[int(y)] = data
        levels = (1 << bits) - 1
        pixels = bytearray()
        for y in range(height):
            data = rows.get(y, b"")
            pixels.append(0)  # PNG filter: none
            for x in range(width):
                bit = x * bits
                byte = data[bit // 8] if bit // 8 < len(data) else 0xFF
                level = (byte >> (8 - bits - bit % 8)) & levels
                pixels.append(level * 255 // levels)
        write_png(path, width, height, bytes(pixels))
        print(f"screenshot saved to {path}")


class EmuPort:
    """The emulator (`paperpaw-emu --headless --repl`) as a port: its standard output is what the
    X4 sends, what is written goes to its standard input."""

    def __init__(self, command):
        self.proc = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT, bufsize=0)
        self.chunks = queue.Queue()
        self.pending = b""
        self.port = "emulator"
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        while data := self.proc.stdout.read(65536):
            self.chunks.put(data)
        self.chunks.put(b"")

    @property
    def in_waiting(self):
        while True:
            try:
                self.pending += self.chunks.get_nowait()
            except queue.Empty:
                return len(self.pending)

    def read(self, count=1):
        if not self.pending:
            try:
                self.pending = self.chunks.get(timeout=0.2)
            except queue.Empty:
                return b""
            if not self.pending:
                raise serial.SerialException("the emulator ended")
        data, self.pending = self.pending[:count], self.pending[count:]
        return data

    def write(self, data):
        try:
            self.proc.stdin.write(data)
            self.proc.stdin.flush()
        except OSError as error:
            raise serial.SerialException(f"the emulator ended: {error}") from error

    def reset_input_buffer(self):
        self.in_waiting
        self.pending = b""

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            self.proc.kill()


class EmuX4(X4):
    """The same, on the emulator: the card is the library folder, time is emulated
    (`wait` and the settling of measurements cost no real time)."""

    def __init__(self, command, root):
        super().__init__(EmuPort(command))
        self.name = "emulator"
        self.root = Path(root).resolve()

    def path(self, name):
        full = (self.root / name.strip("/")).resolve()
        if full != self.root and self.root not in full.parents:
            raise StepError(f"{name}: outside the card")
        return full

    def pass_time(self, seconds):
        self.drain()
        if seconds > 0:
            super().ask(f"wait {int(seconds * 1000)}", "OK", 600)
        self.drain()

    def reconnect(self, wait=40):
        raise LinkError("the emulator ended")

    def hard_reset(self, seconds):
        start = len(self.logbuf)
        super().ask("restart", "OK")
        self.pass_time(min(seconds, 2))
        self.reset_log = [(0.0, text) for _, text in self.logbuf[start:]]

    def back_again(self, command, until):
        super().ask(command, "OK")
        self.pass_time(0.2)

    def ask(self, command, until, timeout=TIMEOUT_S):
        word, _, rest = command.partition(" ")
        if word not in ("ls", "stat", "md5", "rm", "mkdir", "mv", "get"):
            return super().ask(command, until, timeout)
        if word == "mv":
            old, new = (bytes.fromhex(h).decode() for h in rest.split())
            self.path(old).rename(self.path(new))
            return ["OK"]
        full = self.path(rest)
        if word == "ls":
            if not full.is_dir():
                raise StepError(f"{command}: ERR no such folder")
            rows = [f"ENTRY {'d' if e.is_dir() else 'f'} {0 if e.is_dir() else e.stat().st_size} {e.name}"
                    for e in sorted(full.iterdir())]
            return rows + ["END"]
        if word == "stat":
            if full.is_file():
                return [f"STAT f {full.stat().st_size}"]
            return ["STAT d 0"] if full.is_dir() else ["STAT - 0"]
        if word == "md5":
            data = full.read_bytes()
            return [f"MD5 {hashlib.md5(data).hexdigest()} {len(data)}"]
        if word == "rm":
            import shutil
            shutil.rmtree(full) if full.is_dir() else full.unlink()
            return ["OK"]
        if word == "mkdir":
            full.mkdir(parents=True, exist_ok=True)
            return ["OK"]
        raise StepError(f"{command}: not on the emulator")

    def write(self, path, data):
        full = self.path(path)
        full.parent.mkdir(parents=True, exist_ok=True)
        full.write_bytes(data)

    def read(self, path):
        full = self.path(path)
        if not full.is_file():
            raise StepError(f"{path}: ERR no such file")
        return full.read_bytes()

    def screenshot(self, path):
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        super().ask(f"png {Path(path).resolve().as_posix().encode().hex()}", "OK")
        print(f"screenshot saved to {path}")


class Esptool:
    """Flashing through esptool, `command` with {chip} and {port} in it. The chip on the
    link is read first: a wrong one is never written."""

    def __init__(self, command, x4):
        self.command, self.x4 = command, x4

    def run(self, chip, *args, timeout=900):
        line = self.command.format(chip=chip, port=self.x4.name)
        return subprocess.run([*shlex.split(line), *args], capture_output=True, text=True, timeout=timeout)

    def check_chip(self, chip):
        self.x4.port.close()
        try:
            done = self.run("auto", "chip-id", timeout=60)
        finally:
            self.x4.reopen()
        text = ANSI.sub("", done.stdout + done.stderr)
        found = re.findall(r"Chip(?: type| is)\s*:?\s*(ESP32[-\w]*)", text)
        want = re.sub(r"[^a-z0-9]", "", chip.lower())
        if not found or re.sub(r"[^a-z0-9]", "", found[0].lower()) != want:
            raise StepError(f"the chip is {found[0] if found else 'not read'}, not {chip}: not flashed")
        return f"chip {found[0]} as wanted"

    def flash(self, image, chip, opts):
        said = self.check_chip(chip)
        offset = opts.get("offset", "0x0")
        erase = opts.get("erase", "0xd000+0x2000")
        steps = []
        if erase != "none":
            at, _, size = erase.partition("+")
            steps.append(("erase-region", at, size))
        steps.append(("write-flash", offset, image))
        steps.append(("verify-flash", offset, image))
        self.x4.port.close()
        try:
            for name, *args in steps:
                done = self.run(chip, name, *args)
                if done.returncode != 0:
                    raise StepError(f"{name} failed: {ANSI.sub('', done.stdout + done.stderr)[-300:]}")
        finally:
            self.x4.reopen()
        return f"{said}; {image} written and verified at {offset}"


FIRMWARE = Path(__file__).resolve().parent.parent / "shells/xteink/target/riscv32imc-unknown-none-elf/release/paperpaw-xteink"


def symbols(elf):
    """The firmware's functions: sorted start addresses, sizes and names."""
    pattern = os.path.expanduser("~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm*")
    nm = next(iter(glob.glob(pattern)), "llvm-nm")
    out = subprocess.run(
        [nm, "--demangle", "--numeric-sort", "--print-size", "--defined-only", str(elf)],
        capture_output=True, text=True, check=True,
    ).stdout
    starts, sizes, names = [], [], []
    for line in out.splitlines():
        parts = line.split(" ", 3)
        if len(parts) == 4 and parts[2] in "tTwW":
            starts.append(int(parts[0], 16))
            sizes.append(int(parts[1], 16))
            # Without the hash and the generic arguments, which only differ.
            names.append(parts[3].rsplit("::h", 1)[0])
    return starts, sizes, names


def calls(line):
    """The functions of the addresses a panic's message ends with (after
    `@`), innermost first, from the firmware built here."""
    if " @ " not in line or not FIRMWARE.exists():
        return []
    starts, sizes, names = symbols(FIRMWARE)
    named = []
    for word in line.rsplit(" @ ", 1)[1].split():
        try:
            pc = int(word, 16)
        except ValueError:
            continue
        i = bisect.bisect_right(starts, pc) - 1
        if i >= 0 and pc < starts[i] + max(sizes[i], 1):
            named.append(f"{pc:x} {names[i]}")
        else:
            named.append(f"{pc:x} ?")
    return named


def rom_functions(x4):
    """Where the ROM's functions lie: its table of jumps (as the linker
    scripts of esp-rom-sys name them) read from the chip and followed."""
    table = {}
    pattern = str(FIRMWARE.parent / "build/esp-rom-sys-*/out/rom/*.ld")
    for path in glob.glob(pattern):
        for line in Path(path).read_text(errors="replace").splitlines():
            name, _, value = line.partition(" = ")
            value = value.strip().rstrip(";")
            if value.startswith("0x4000") and name.strip().isidentifier():
                table[int(value, 16)] = name.strip()
    if not table:
        return [], []
    low, high = min(table), max(table)
    words = {}
    for at in range(low, high + 4, 4096 * 4):
        count = min(4096, (high + 4 - at) // 4)
        for answer in x4.ask(f"peek {at:x} {count}", "END")[:-1]:
            fields = answer.split()
            base = int(fields[1], 16)
            for i, word in enumerate(fields[2:]):
                words[base + i * 4] = int(word, 16)
    targets = []
    for entry, name in table.items():
        insn = words.get(entry)
        # A jump (jal): where it goes.
        if insn is None or insn & 0x7F != 0x6F:
            continue
        imm = ((insn >> 31) & 1) << 20 | ((insn >> 12) & 0xFF) << 12
        imm |= ((insn >> 20) & 1) << 11 | ((insn >> 21) & 0x3FF) << 1
        if imm & (1 << 20):
            imm -= 1 << 21
        targets.append((entry + imm, name))
    targets.sort()
    return [t for t, _ in targets], [n for _, n in targets]


def report(places, elf, x4, top=30):
    starts, sizes, names = symbols(elf)
    rom_starts, rom_names = rom_functions(x4) if any(0x4004_0000 <= p < 0x4006_0000 for p in places) else ([], [])
    counts = {}
    for pc in places:
        i = bisect.bisect_right(starts, pc) - 1
        if i >= 0 and pc < starts[i] + max(sizes[i], 1):
            name = names[i]
        elif 0x4000_0000 <= pc < 0x4006_0000:
            # The chip's ROM: its C library, soft floats, …
            j = bisect.bisect_right(rom_starts, pc) - 1
            name = f"ROM {rom_names[j]}" if j >= 0 else "ROM"
        else:
            name = f"? {pc:x}"
        counts[name] = counts.get(name, 0) + 1
    # Waiting (the main loop's rest, the panel's refresh) is not work.
    idle = sum(n for name, n in counts.items() if "Delay>::delay" in name)
    busy = (len(places) - idle) or 1
    print(f"{len(places)} samples, {len(places) * 5 / 1000:.1f} s, of them waiting {idle * 5 / 1000:.1f} s")
    crates = {}
    for name, n in counts.items():
        if "Delay>::delay" not in name:
            # The crate: the first path in the name, past `<` and `&`.
            first = name.lstrip("<&").split("::")[0] if "::" in name else name
            crates[first] = crates.get(first, 0) + n
    print("by crate, of the work:")
    for name, n in sorted(crates.items(), key=lambda kv: -kv[1])[:12]:
        print(f"{n * 100 / busy:5.1f} %  {name}")
    print("by function, of the work:")
    for name, n in sorted(counts.items(), key=lambda kv: -kv[1])[:top]:
        if "Delay>::delay" not in name:
            print(f"{n * 100 / busy:5.1f} %  {name[:140]}")


def write_png(path, width, height, gray_rows):
    """A gray PNG, from rows each starting with a filter byte."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
    png += chunk(b"IDAT", zlib.compress(gray_rows, 9)) + chunk(b"IEND", b"")
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_bytes(png)


def hex_pairs(text):
    """`key=value:key=value` (a value may hold colons, as an address does)
    as the remote's `key=<hex>` words."""
    pairs = re.split(r":(?=[A-Za-z0-9_.-]+=)", text) if text else []
    words = []
    for pair in pairs:
        key, _, value = pair.partition("=")
        words.append(f"{key}={value.encode().hex() or '-'}")
    return words


def run(x4, step):
    parts = step.split(":")
    # What follows the step's name, colons and all (paths, texts).
    rest = step.split(":", 1)[1] if ":" in step else ""
    match parts:
        case ["wait", ms]:
            x4.pass_time(int(ms) / 1000)
        case ["tap", x, y]:
            x4.touch("down", x, y)
            time.sleep(0.08)
            x4.touch("up", x, y)
        case ["swipe", x1, y1, x2, y2, *ms]:
            x4.swipe(int(x1), int(y1), int(x2), int(y2), int(ms[0]) if ms else 300)
        case ["slot" | "show" | "emit" as kind, *_]:
            # The id, then key=value parameters (values may hold colons).
            ident, _, given = rest.partition(":")
            x4.ask(" ".join([kind, ident, *hex_pairs(given)]), "OK")
        case ["type" | "type-enter" as kind, *_]:
            x4.ask(f"{kind} {rest.encode().hex() or '-'}", "OK")
        case ["drag", x1, y1, x2, y2, steps, ms]:
            x4.ask(f"drag {x1} {y1} {x2} {y2} {steps} {ms}", "OK")
            # Down, the moves and the lift, each read on the interrupt's
            # beat (5 ms) once due, and a little more.
            beat = max(5, -(-int(ms) // 5) * 5)
            time.sleep(((int(steps) + 1) * beat + 100) / 1000)
        case ["clicks", button, count, every, held]:
            x4.ask(f"clicks {button} {count} {every} {held}", "OK")
            # They go on in the X4's interrupt: wait for the last.
            x4.pass_time((int(count) * int(every) + 200) / 1000)
        case ["tap-label", *_]:
            found = [p for p in x4.parts() if rest in p[6]]
            if not found:
                raise StepError(f"tap-label: nothing shows {rest}")
            _, x, y, w, h, _, _ = found[-1]
            run(x4, f"tap:{x + w // 2}:{y + h // 2}")
        case ["boot"]:
            x4.boot()
        case ["wifi", "status"]:
            print(x4.ask("wifi status", "WIFI")[-1])
        case ["wifi", "scan"]:
            for answer in x4.ask("wifi scan", "END")[:-1]:
                print(answer.split(" ", 1)[1])
        case ["wifi", "off"]:
            x4.ask("wifi off", "OK")
        case ["http", *_] if rest.startswith("get:"):
            # `http:get:<url>` into memory, `http:get:<url>><path>` into a file.
            url, _, path = rest[4:].partition(">")
            started = time.monotonic()
            answer = x4.ask(f"http get {url} {path}".strip(), "HTTP", REMOVE_TIMEOUT_S)[-1]
            print(f"{answer}  ({time.monotonic() - started:.1f} s)")
        case ["screens"]:
            print(x4.ask("screens", "SCREENS")[-1].split(" ", 1)[-1])
        case ["ls", *_]:
            for is_dir, size, name in x4.listing(rest):
                print(f"{name}/" if is_dir else f"{name}  {size}")
        case ["stat", *_]:
            print(x4.ask(f"stat {rest}", "STAT")[-1])
        case ["md5", *_]:
            print(x4.ask(f"md5 {rest}", "MD5")[-1])
        case ["cat", *_]:
            sys.stdout.write(x4.read(rest).decode("utf-8", "replace"))
        case ["rm", *_]:
            x4.ask(f"rm {rest}", "OK", REMOVE_TIMEOUT_S)
        case ["mkdir", *_]:
            x4.ask(f"mkdir {rest}", "OK")
        case ["mv", *_] if ">" in rest:
            old, new = rest.split(">", 1)
            x4.ask(f"mv {old.encode().hex()} {new.encode().hex()}", "OK")
        case ["sync"]:
            x4.ask("sync", "OK")
        case ["version"]:
            for answer in x4.ask("version", "END")[:-1]:
                print(answer.split(" ", 1)[1])
        case ["plugins"]:
            for answer in x4.ask("plugins", "END")[:-1]:
                print(answer.split(" ", 1)[1])
        case ["native"]:
            for answer in x4.ask("native", "END")[:-1]:
                print(answer.split(" ", 1)[1])
        case ["actions", *_]:
            for answer in x4.ask(f"actions {rest}".strip(), "END")[:-1]:
                print(answer.split(" ", 1)[1])
        case ["settings", *_]:
            for answer in x4.ask(f"settings {rest}".strip(), "END")[:-1]:
                print(answer.split(" ", 1)[1])
        case ["time"]:
            print(x4.ask("time", "TIME")[-1])
        case ["usage"]:
            answers = x4.ask("usage", "END")[:-1]
            f, p, u, pixels, inputs, sleeps, panel = answers[0].split()[1:]
            print(f"refreshes {f} fast, {p} partial, {u} full; {pixels} pixels; "
                  f"{inputs} inputs, {sleeps} sleeps, panel {panel} ms")
            x4.parse_usage(answers)
            print(f"{'plugin':<20}{'busy ms':>10}{'calls':>8}{'shown s':>9}"
                  f"{'refreshes':>12}{'read KB':>9}{'written KB':>11}")
            rows = [a.split()[1:] for a in answers[1:]]
            for row in sorted(rows, key=lambda r: -int(r[1])):
                name, busy, calls, shown, rf, rp, ru, read, written = row
                print(f"{name:<20}{int(busy) // 1000:>10}{calls:>8}{int(shown) // 1000:>9}"
                      f"{f'{rf}/{rp}/{ru}':>12}{int(read) // 1000:>9}{int(written) // 1000:>11}")
        case ["time", "set", *ms]:
            at = int(ms[0]) if ms else int(time.time() * 1000)
            x4.ask(f"time set {at}", "OK")
        case ["log", level]:
            x4.ask(f"log {level}", "OK")
        case ["wifi-connect", *_]:
            # `wifi-connect:NAME:PASSWORD` (`${env:WIFI_PASSWORD}` keeps it out of the script).
            ssid, _, password = rest.partition(":")
            x4.drain()
            x4.port.reset_input_buffer()
            x4.buffer = b""
            x4.port.write(f"wifi connect {ssid.encode().hex()} {password.encode().hex() or '-'}\n".encode())
            print(x4.expect("wifi connect", "CONNECTED")[-1])
            print(x4.expect("wifi connect", "IP")[-1])
        case ["restart"]:
            x4.back_again("restart", "BOOT")
        case ["panic", *_]:
            x4.back_again(f"panic {(rest or 'asked').encode().hex()}", "BOOT")
        case ["long", button]:
            x4.press(button, LONG_MS)
        case ["hold", button, ms]:
            x4.press(button, int(ms))
        case ["screen"]:
            print(x4.ask("screen", "SCREEN")[-1].split(" ", 1)[-1])
        case ["input"]:
            print(x4.ask("input", "INPUT")[-1])
        case ["run", action]:
            print(x4.ask(f"run {action}", "OK")[-1])
        case ["open", path]:
            print(x4.ask(f"open {path.encode().hex()}", "OK")[-1])
        case ["set", *rest] if "=" in ":".join(rest):
            # Values may hold `:` (`22:00`).
            setting, value = ":".join(rest).split("=", 1)
            print(x4.ask(f"set {setting} {value.encode().hex()}", "OK")[-1])
        case ["setting", setting]:
            print(x4.ask(f"setting {setting}", "SETTING")[-1].split(" ", 1)[-1])
        case ["gray"]:
            print(x4.ask("gray", "OK")[-1])
        case ["gray", path]:
            print(x4.ask(f"gray {path.encode().hex()}", "OK")[-1])
        case ["graytone", tone]:
            print(x4.ask(f"graytone {tone}", "OK")[-1])
        case ["refresh", kind]:
            print(x4.ask(f"refresh {kind}", "OK")[-1])
        case ["heap"]:
            _, used, free, *stack = x4.ask("heap", "HEAP")[-1].split()
            print(f"heap: {used} bytes used, {free} free")
            if stack and int(stack[1]):
                print(f"stack: {stack[0]} of {stack[1]} bytes never used")
            if len(stack) > 2:
                print(f"largest free block: {stack[2]} bytes")
            if len(stack) > 3:
                print(f"peak: {stack[3]} bytes used at most since start")
            x4.last_heap = {"used": int(used), "free": int(free), "stack": int(stack[0]) if stack and int(stack[1]) else None,
                            "block": int(stack[2]) if len(stack) > 2 else int(free), "peak": int(stack[3]) if len(stack) > 3 else 0}
        case ["describe"]:
            for answer in x4.ask("describe", "END")[:-1]:
                _, role, x, y, w, h, state, *label = answer.split(" ")
                state = "" if state == "-" else f" ({state.replace('+', ', ')})"
                print(f'{role} "{" ".join(label)}"{state} at {x},{y} {w}x{h}')
        case ["shot", *_]:
            x4.screenshot(rest)
        case [button]:
            x4.press(button, 60)
        case _:
            raise StepError(f"unknown step: {step}")


def main():
    # Labels come in UTF-8, whatever the console's code page.
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--port")
    parser.add_argument("--script", default="", help="steps, separated by commas (see below)")
    parser.add_argument("--script-file", action="append", default=[], metavar="FILE.pps",
                        help="a script file, a step a line (run before --script)")
    parser.add_argument("--out", metavar="DIR", help="write results.json, report.md, device.log (and STOPPED) here")
    parser.add_argument("--baseline", metavar="results.json", help="an earlier run to compare with (before -> now, `baseline` in asserts)")
    parser.add_argument("--var", action="append", default=[], metavar="NAME=VALUE", help="a script variable ($NAME)")
    parser.add_argument("--stop-on-fail", action="store_true", help="end the script at the first failed check")
    parser.add_argument("--retries", type=int, default=2, help="tries to get the link back when it is lost")
    parser.add_argument("--esptool", metavar="COMMAND", help="esptool with {chip} and {port} (for `flash:` and `chip:`)")
    parser.add_argument("--emulator", metavar="PAPERPAW-EMU", help="run the script on the emulator, not on an X4")
    parser.add_argument("--library", metavar="DIR", help="the emulator's card (a folder)")
    parser.add_argument("--device-profile", default="xteink-x4", help="the emulator's device (default xteink-x4)")
    parser.add_argument("--emu-arg", action="append", default=[], metavar="ARG", help="more for the emulator, e.g. --emu-arg=--set --emu-arg=core.language=de")
    parser.add_argument("--screenshot")
    parser.add_argument("--log", type=float, metavar="SECONDS")
    parser.add_argument("-v", "--verbose", action="store_true", help="print the X4's log too")
    parser.add_argument("--profile", action="store_true", help="where the time goes during the steps")
    parser.add_argument("--trace", action="store_true", help="what the steps do on the storage")
    parser.add_argument("--put", nargs=2, action="append", default=[], metavar=("FILE", "PATH"))
    parser.add_argument("--get", nargs=2, action="append", default=[], metavar=("PATH", "FILE"))
    parser.add_argument("--push", nargs=2, action="append", default=[], metavar=("DIR", "PATH"))
    parser.add_argument("--pull", nargs=2, action="append", default=[], metavar=("PATH", "DIR"))
    parser.add_argument("--ls", action="append", default=[], metavar="PATH")
    parser.add_argument("--cat", action="append", default=[], metavar="PATH")
    parser.add_argument("--rm", action="append", default=[], metavar="PATH")
    parser.add_argument("--mkdir", action="append", default=[], metavar="PATH")
    parser.add_argument("--mv", nargs=2, action="append", default=[], metavar=("FROM", "TO"))
    parser.add_argument(
        "--trust-code",
        action="append",
        default=[],
        metavar="FOLDER",
        help="name the code in plugins/FOLDER as installed (one's own build copied by hand)",
    )
    parser.add_argument("--info", action="store_true", help="version, plugins, native code, screens, time")
    parser.add_argument("--crashes", action="store_true", help="the crash file, its calls named")
    parser.add_argument(
        "--wifi-connect", metavar="NAME", help="join a network (asks for its password, not shown)"
    )
    args = parser.parse_args()
    try:
        sys.exit(session(args))
    except (StepError, LinkError, ScriptError) as error:
        sys.exit(f"{error}")


def session(args):
    if args.emulator:
        if not args.library:
            sys.exit("--emulator needs --library (the emulator's card, a folder)")
        x4 = EmuX4([args.emulator, "--headless", "--repl", "--profile", args.device_profile,
                    "--library", args.library, *args.emu_arg], args.library)
    else:
        name = args.port or find_port()
        x4 = X4(open_port(name))
        x4.name = name
    try:
        return talk(args, x4)
    finally:
        x4.port.close()


def talk(args, x4):
    x4.verbose = args.verbose
    if args.log:
        deadline = time.monotonic() + args.log
        while (line := x4.line(deadline)) is not None:
            print(line)
        return
    if args.info:
        for step in ["version", "plugins", "native", "screens", "time"]:
            run(x4, step)
    if args.crashes:
        text = x4.read(".paperpaw/crashes.txt").decode("utf-8", "replace")
        for line in text.splitlines():
            print(line)
            for name in calls(line):
                print(f"    {name}")
    # Made, brought, fetched, moved, shown, then removed.
    for path in args.mkdir:
        run(x4, f"mkdir:{path}")
    for local, path in args.put:
        x4.put(local, path)
    for local, path in args.push:
        x4.push(local, path)
    for path, local in args.get:
        x4.get(path, local)
    for path, local in args.pull:
        x4.pull(path, local)
    for old, new in args.mv:
        run(x4, f"mv:{old}>{new}")
    for path in args.ls:
        run(x4, f"ls:{path}")
    for path in args.cat:
        run(x4, f"cat:{path}")
    for path in args.rm:
        run(x4, f"rm:{path}")
    for folder in args.trust_code:
        x4.trust_code(folder)
    if args.wifi_connect:
        password = getpass.getpass(f"Password for {args.wifi_connect} (empty if open): ")
        hexed = password.encode().hex() or "-"
        x4.port.reset_input_buffer()
        x4.buffer = b""
        x4.port.write(f"wifi connect {args.wifi_connect.encode().hex()} {hexed}\n".encode())
        # Not `ask`: it would repeat the command, password and all, on an error.
        print(x4.expect("wifi connect", "CONNECTED")[-1])
        print(x4.expect("wifi connect", "IP")[-1])
    if args.profile:
        x4.ask("profile start", "OK")
    if args.trace:
        x4.ask("trace start", "OK")
    code = 0
    if args.script or args.script_file:
        statements = []
        for path in args.script_file:
            statements += ppscript.parse(Path(path).read_text(encoding="utf-8"), path)
        statements += ppscript.parse(args.script, "--script", inline=True)
        baseline = json.loads(Path(args.baseline).read_text(encoding="utf-8")) if args.baseline else None
        esptool = args.esptool or f"{shlex.quote(sys.executable)} -m esptool --chip {{chip}} --port {{port}} --after no-reset"
        engine = ppscript.Engine(
            x4, lambda step: run(x4, step), target="emu" if args.emulator else "x4", out=args.out,
            baseline=baseline, variables=dict(v.partition("=")[::2] for v in args.var),
            stop_on_fail=args.stop_on_fail, retries=args.retries, reconnect=x4.reconnect,
            flasher=Esptool(esptool, x4))
        engine.script_name = ", ".join([Path(p).name for p in args.script_file] + (["--script"] if args.script else []))
        engine.run(statements)
        code = engine.exit_code()
    if args.trace:
        for answer in x4.ask("trace stop", "END")[:-1]:
            print(answer[3:])
    if args.profile:
        answers = x4.ask("profile stop", "END")
        places = [int(pc, 16) for a in answers[:-1] for pc in a.split()[1:]]
        report(places, FIRMWARE, x4)
    if args.screenshot:
        x4.screenshot(args.screenshot)
    if x4.failed:
        print(f"{len(x4.failed)} check(s) failed: " + "; ".join(x4.failed), file=sys.stderr)
        code = code or 1
    return code


if __name__ == "__main__":
    main()

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
  panic:why          a panic, to try what a crash leaves (waits until back)

Needs pyserial (pip install pyserial). Without --port, the X4 is found by
its USB id.
"""

import argparse
import bisect
import getpass
import glob
import hashlib
import os
import re
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

import serial
from serial.tools import list_ports

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


def open_port(name):
    port = serial.Serial()
    port.port = name
    port.timeout = 0.2
    # Without DTR and RTS, opening the port does not reset the chip.
    port.dtr = False
    port.rts = False
    port.open()
    return port


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

    def line(self, deadline):
        """The next line from the X4, or None once the deadline passed."""
        while b"\n" not in self.buffer:
            if time.monotonic() > deadline:
                return None
            self.buffer += self.port.read(self.port.in_waiting or 1)
        line, self.buffer = self.buffer.split(b"\n", 1)
        return line.decode("utf-8", "replace").rstrip("\r")

    def pass_time(self, seconds):
        """Lets `seconds` go by; with `verbose`, the X4's log meanwhile is
        printed, not dropped by the next command."""
        if not self.verbose:
            time.sleep(seconds)
            return
        deadline = time.monotonic() + seconds
        while (line := self.line(deadline)) is not None:
            if not line.startswith("PP:"):
                print(line)

    def ask(self, command, until, timeout=TIMEOUT_S):
        """Sends a command and returns the X4's answers (lines with "PP:")
        up to the one starting with `until`."""
        if self.verbose:
            # What the log said since the last command, before it goes.
            self.pass_time(0)
        self.port.reset_input_buffer()
        self.buffer = b""
        self.port.write(command.encode() + b"\n")
        return self.expect(command, until, timeout)

    def expect(self, command, until, timeout=TIMEOUT_S):
        """The X4's answers to `command` up to the one starting with `until`."""
        answers = []
        deadline = time.monotonic() + timeout
        while True:
            line = self.line(deadline)
            if line is None:
                sys.exit(f"{command}: no answer from the X4")
            if not line.startswith("PP:"):
                if self.verbose:
                    print(line)
                continue
            answer = line[3:]
            if answer.startswith(("ERR", "BUSY")):
                sys.exit(f"{command}: {answer}")
            answers.append(answer)
            if answer.startswith(until):
                return answers

    def put(self, local, path):
        """Copies the file `local` to `path` on the X4's card, and checks it
        there by its MD5."""
        data = Path(local).read_bytes()
        started = time.monotonic()
        self.write(path, data)
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
            sys.exit(f"plugins/{folder}/code.ppn: {error}")
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
            sys.exit(f"{path}: arrived changed (MD5 {there[1]}, {there[2]} bytes)")

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
        before if it panicked, with its calls named."""
        print(self.ask("boot", "BOOT")[-1])
        deadline = time.monotonic() + 0.5
        while (line := self.line(deadline)) is not None:
            if line.startswith("PP:NAPS"):
                print(line[3:])
            if line.startswith("PP:PANIC"):
                print(line[3:])
                for name in calls(line):
                    print(f"    {name}")

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
        it answers once more."""
        self.port.write(command.encode() + b"\n")
        time.sleep(1)
        self.port.close()
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            time.sleep(1)
            try:
                self.port = open_port(self.name)
                self.buffer = b""
                self.boot()
                return
            except (serial.SerialException, SystemExit):
                try:
                    self.port.close()
                except serial.SerialException:
                    pass
        sys.exit(f"{command}: the X4 did not come back in 40 s")

    def press(self, button, ms):
        self.ask(f"hold {button} {ms}", "OK")

    def screenshot(self, path):
        answers = self.ask("shot", "END")
        _, width, height, bits = answers[0].split()
        width, height, bits = int(width), int(height), int(bits)
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
        case ["until", *_]:
            text, ms = rest, 30000
            if ":" in rest and rest.rsplit(":", 1)[1].isdigit():
                text, ms = rest.rsplit(":", 1)[0], int(rest.rsplit(":", 1)[1])
            deadline = time.monotonic() + ms / 1000
            while not x4.shows(text):
                if time.monotonic() > deadline:
                    x4.failed.append(f"until {text}: not shown in {ms} ms")
                    print(f"FAILED: {text} not shown in {ms} ms")
                    return
                time.sleep(0.5)
        case ["expect", *_]:
            if not x4.shows(rest):
                x4.failed.append(f"expect {rest}")
                print(f"FAILED: {rest} not shown")
        case ["expect-not", *_]:
            if x4.shows(rest):
                x4.failed.append(f"expect-not {rest}")
                print(f"FAILED: {rest} shown")
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
                sys.exit(f"tap-label: nothing shows {rest}")
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
            if stack:
                print(f"stack: {stack[0]} of {stack[1]} bytes never used")
            if len(stack) > 2:
                print(f"largest free block: {stack[2]} bytes")
            if len(stack) > 3:
                print(f"peak: {stack[3]} bytes used at most since start")
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
            sys.exit(f"unknown step: {step}")


def main():
    # Labels come in UTF-8, whatever the console's code page.
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--port")
    parser.add_argument("--script", default="")
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
    name = args.port or find_port()
    port = open_port(name)
    x4 = X4(port)
    x4.name = name
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
    # Steps apart at commas, but for those written `\,`.
    steps = re.split(r"(?<!\\),", args.script)
    for step in filter(None, (s.strip().replace("\\,", ",") for s in steps)):
        run(x4, step)
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
        sys.exit(f"{len(x4.failed)} check(s) failed: " + "; ".join(x4.failed))


if __name__ == "__main__":
    main()

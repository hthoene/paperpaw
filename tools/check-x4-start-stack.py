"""Fails if the X4's (or with --s3 the ESP32-S3 devices') start can run into the stack's band (#2556).

The owner's first test image boot-looped because `main` -> `start` ->
`prepare` -> a flash read reached the band at the main stack's end while only
the single functions' frames were checked, never the path of the start as a
whole. This reads the frames of an X4 (ESP32-C3) firmware ELF the way
`tools/stack-frames.py` does and sums three paths:

    main + start + the deepest chain from `prepare`
    main + start + the deepest chain from `kernel` (the kernel's build, which
        reads the card; one branch cut, see START_CUTS)
    main + the deepest chain from the firmware's `run` (the main loop)

Calls through pointers are not followed, so these are floors, not ceilings;
the device's high-water log gives the rest. Against what lies above the band: the stack's top minus the band's top
(`crates/paperpaw-x4/src/stack_band.rs`: the first BAND-aligned region at or
above `end + RESERVE`).

Usage: tools/check-x4-start-stack.py <elf> [--margin BYTES] [--s3]
With --s3 the ELF is an ESP32-S3 firmware (X4 Pro, X4 Classic): its band and reserve are
`crates/paperpaw-x4/src/s3_band.rs`'s, its frames are read with the Xtensa objdump.
Exit 0: the paths fit with the margin left; 1: they do not; 2: bad input.
"""

import argparse
import glob
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


S3 = False


def constants():
    if S3:
        text = (HERE.parent / "crates/paperpaw-x4/src/s3_band.rs").read_text()

        def const(name):
            m = re.search(rf"pub const {name}: u32 = (\d+);", text)
            if not m:
                sys.exit(f"s3_band.rs: no literal `{name}`")
            return int(m.group(1))
        windows = const("WINDOWS_BELOW")
        depth = max(const("HANDLER"), const("INTERRUPT"))
        return const("BAND"), windows * const("LIMIT") + depth
    text = (HERE.parent / "crates/paperpaw-x4/src/stack_band.rs").read_text()
    def const(name):
        m = re.search(rf"pub const {name}: u32 = (\d+);", text)
        if not m:
            sys.exit(f"stack_band.rs: no literal `{name}`")
        return int(m.group(1))
    band, handler, interrupt = const("BAND"), const("HANDLER"), const("INTERRUPT")
    depth = max(handler, interrupt)
    # FRAME_LIMIT = BAND there (asserted by the file's own tests).
    return band, depth + band


def symbols(elf):
    nm = glob.glob(str(Path.home() / ".rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm"))
    if not nm:
        sys.exit("no llvm-nm in rustup's toolchains (rustup component add llvm-tools)")
    out = subprocess.run([nm[0], elf], capture_output=True, text=True, check=True).stdout
    found = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[2] in ("_stack_end", "_stack_start"):
            found[parts[2]] = int(parts[0], 16)
    if len(found) != 2:
        sys.exit("the ELF has no _stack_end/_stack_start")
    return found["_stack_end"], found["_stack_start"]


def frames(elf, *extra):
    if S3:
        objdump = glob.glob(str(Path.home() / ".rustup/toolchains/esp/xtensa-esp-elf/*/xtensa-esp-elf/bin/xtensa-esp32s3-elf-objdump"))
        # Else `stack-frames.py` looks for espup's own (it reads the ELF's arch).
        if objdump:
            extra = (*extra, "--objdump", objdump[0])
    out = subprocess.run(
        [sys.executable, str(HERE / "stack-frames.py"), elf, "--paths", "0", *extra],
        capture_output=True, text=True, check=True,
    ).stdout
    return out


def frame_of(table, name):
    for line in table.splitlines():
        m = re.match(r"\s*(\d+)\s+\d+\s+\*?\s*(.*)$", line)
        if m and m.group(2).strip() == name:
            return int(m.group(1))
    sys.exit(f"no frame found for {name}")


# What is cut from the chains, and why. `Kernel::apply` applies the effects
# the kernel asks for; its chain through `firmware_suspend` into a plugin's
# install (a signature check, 8 KB below it) is the main loop's, where it is
# counted whole (the run path below); at the start the kernel's build replays
# only a removal, which asks for none of that. Taken to be confirmed by
# Kernel (#2556): a start that does reach it overflows.
START_CUTS = [r"Kernel>::apply$"]


def chain_from(elf, root, cuts=()):
    extra = [x for cut in cuts for x in ("--cut", cut)]
    out = frames(elf, "--top", "0", "--root", root, *extra)
    m = re.search(r"deepest chain from .*?: (\d+) bytes", out)
    if not m:
        sys.exit(f"no chain from {root}")
    return int(m.group(1))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("elf")
    ap.add_argument("--margin", type=int, default=512,
                    help="bytes that must be left above the band for what the paths do not follow")
    ap.add_argument("--s3", action="store_true", help="an ESP32-S3 firmware (X4 Pro, X4 Classic)")
    a = ap.parse_args()
    global S3
    S3 = a.s3
    if not os.path.exists(a.elf):
        print(f"no such file: {a.elf}", file=sys.stderr)
        return 2
    band, reserve = constants()
    end, top = symbols(a.elf)
    start = (end + reserve + (15 if S3 else band - 1)) & ~(15 if S3 else band - 1)
    usable = top - (start + band)
    table = frames(a.elf, "--top", "40000")
    main_f = frame_of(table, "main")
    start_f = frame_of(table, "paperpaw_xteink::start")
    paths = {
        "start: main + start + prepare": main_f + start_f + chain_from(a.elf, "paperpaw_xteink::prepare"),
        "start: main + start + kernel + build and what it calls": main_f + start_f + chain_from(a.elf, "paperpaw_xteink::kernel", START_CUTS),
        "main loop: main + run and what it calls": main_f + chain_from(a.elf, "paperpaw_xteink::firmware::Firmware::run" if S3 else "<paperpaw_xteink::firmware::Firmware>::run"),
    }
    print(f"stack {top - end} B (end {end:#x}, top {top:#x}); band {start:#x}..{start + band:#x}; "
          f"{usable} B above it, margin {a.margin} B")
    print(f"frames: main {main_f}, start {start_f}")
    bad = False
    for name, depth in paths.items():
        left = usable - depth
        ok = left >= a.margin
        bad |= not ok
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: {depth} B, {left} B left")
    if bad:
        print("error: a path of the start or of the main loop runs into the stack's band "
              "(frames grew, or the statics took the stack's room: HEAP_SIZE, X4_BAND_KB in "
              "shells/xteink/src/main.rs; a large local copied or moved by value is the usual "
              "cause of grown frames)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

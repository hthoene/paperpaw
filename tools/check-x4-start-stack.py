"""Fails if the X4's start can run into the stack's band (#2556).

The owner's first test image boot-looped because `main` -> `start` ->
`prepare` -> a flash read reached the band at the main stack's end while only
the single functions' frames were checked, never the path of the start as a
whole. This reads the frames of an X4 (ESP32-C3) firmware ELF the way
`tools/stack-frames.py` does and sums the start's two deepest paths:

    main + start + the deepest chain from `prepare`
    main + start + `kernel` + `BuilderParts::build`

(`build`'s own frame only: what it calls through pointers is not followed, so
this is a floor, not a ceiling; the device's high-water log gives the rest.)
Against what lies above the band: the stack's top minus the band's top
(`crates/paperpaw-x4/src/stack_band.rs`: the first BAND-aligned region at or
above `end + RESERVE`).

Usage: tools/check-x4-start-stack.py <elf> [--margin BYTES]
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


def constants():
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


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("elf")
    ap.add_argument("--margin", type=int, default=1024,
                    help="bytes that must be left above the band for what the paths do not follow")
    a = ap.parse_args()
    if not os.path.exists(a.elf):
        print(f"no such file: {a.elf}", file=sys.stderr)
        return 2
    band, reserve = constants()
    end, top = symbols(a.elf)
    start = (end + reserve + band - 1) & ~(band - 1)
    usable = top - (start + band)
    table = frames(a.elf, "--top", "40000")
    main_f = frame_of(table, "main")
    start_f = frame_of(table, "paperpaw_xteink::start")
    kernel_f = frame_of(table, "paperpaw_xteink::kernel")
    build_f = frame_of(table, "<paperpaw_core::builder::BuilderParts>::build")
    prep = frames(a.elf, "--top", "0", "--root", "paperpaw_xteink::prepare")
    m = re.search(r"deepest chain from paperpaw_xteink::prepare: (\d+) bytes", prep)
    if not m:
        sys.exit("no chain from prepare")
    prepare_chain = int(m.group(1))
    paths = {
        "main + start + prepare's deepest chain": main_f + start_f + prepare_chain,
        "main + start + kernel + build": main_f + start_f + kernel_f + build_f,
    }
    print(f"stack {top - end} B (end {end:#x}, top {top:#x}); band {start:#x}..{start + band:#x}; "
          f"{usable} B above it, margin {a.margin} B")
    print(f"frames: main {main_f}, start {start_f}, kernel {kernel_f}, build {build_f}")
    bad = False
    for name, depth in paths.items():
        left = usable - depth
        ok = left >= a.margin
        bad |= not ok
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: {depth} B, {left} B left")
    if bad:
        print("error: the start path runs into the stack's band (frames grew: "
              "a large local copied or moved by value is the usual cause)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

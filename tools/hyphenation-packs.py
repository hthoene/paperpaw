#!/usr/bin/env python3
"""Build all thirteen interface-7 hyphenation data packs for a data release.

Usage: python3 tools/hyphenation-packs.py OUT_DIR

This is the data.py-compatible adapter for the canonical Rust pack builder.
Catalogue activation remains separate: .pphp must be accepted by the store
before native catalogue entries are replaced with data entries.
"""

import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: hyphenation-packs.py OUT_DIR")
    out = pathlib.Path(sys.argv[1]).resolve()
    subprocess.run(
        ["cargo", "run", "--locked", "-j4", "-p", "paperpaw-hyphenation-packs",
         "--", "build", str(ROOT), str(out)],
        cwd=ROOT, check=True,
    )


if __name__ == "__main__":
    main()

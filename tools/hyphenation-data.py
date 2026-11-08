#!/usr/bin/env python3
"""Build the thirteen code-free hyphenation plugins for data.py."""
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: hyphenation-data.py OUT_DIR")
    subprocess.run([
        "cargo", "run", "--locked", "-p", "paperpaw-hyphenation-packs", "--",
        "build", str(ROOT), str(Path(sys.argv[1]).resolve()),
    ], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()

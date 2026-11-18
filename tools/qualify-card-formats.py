#!/usr/bin/env python3
"""Card formats of the released betas against this tree (#229).

Every versioned fixture (`testdata/*.vN`, `tests/data/*`) a release tag
(v0.6.0-beta.1 to .3) ships is a file format a device of that beta wrote or
read. The unit tests of this tree read the fixtures of the same path, so a
fixture that is byte-identical here is a format this build still reads. One
that is missing or changed must be in EXPLAINED below, with why the format
still reads (or why nothing is lost); anything else fails the run.

Reads git objects only (no checkout, no network, no device):
    python3 tools/qualify-card-formats.py [--tag v0.6.0-beta.1 ...] [--ref HEAD]
"""

import argparse
import subprocess
import sys

TAGS = ["v0.6.0-beta.1", "v0.6.0-beta.2", "v0.6.0-beta.3"]

# path -> why it is not a lost format. Keep each reason checkable.
EXPLAINED = {
    "plugins/home/testdata/pages.ini.v1": (
        "fixture edited, not the format: 31a2906e9 names the store's tool "
        "(`tool:store.open`) where beta wrote `tool:file-manager.open`; that tool "
        "is still declared (plugins/file-manager/declarations.rs), so a beta "
        "Home page still resolves"
    ),
    "plugins/widgets/testdata/quotes.txt.v1": (
        "fixture removed with the interface-6 widgets pack (35f828e6e); the "
        "native widgets plugin reads the same `quotes.txt` (OWN_FILE, "
        "plugins/widgets/src/native_quote.rs)"
    ),
    "plugins/widgets/testdata/turns.txt.v1": (
        "fixture removed with the interface-6 widgets pack (35f828e6e); the "
        "native widgets plugin reads the same `quote.txt`/`picture.txt` turns "
        "(TURNS_FILE)"
    ),
}
# Test assets of a format plugin's unit tests, not a card format.
IGNORED_PREFIXES = ("plugins/format-epub/tests/data/atkinson-",)


def git(*args):
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def fixtures(ref):
    found = {}
    for line in git("ls-tree", "-r", ref).splitlines():
        meta, path = line.split("\t", 1)
        blob = meta.split()[2]
        if ("/testdata/" in path or "/tests/data/" in path) and "hyphenation-packs" not in path:
            if not path.startswith("fuzz"):
                found[path] = blob
    return found


def blob_of(ref, path):
    out = subprocess.run(
        ["git", "rev-parse", "-q", "--verify", f"{ref}:{path}"], capture_output=True, text=True
    )
    return out.stdout.strip() or None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tag", action="append", help="a release tag (default: beta.1 to beta.3)")
    ap.add_argument("--ref", default="HEAD", help="the tree that must still read them")
    args = ap.parse_args()
    failed = False
    print(f"{'tag':16} {'fixtures':>8} {'identical':>9} {'explained':>9} {'failed':>6}")
    for tag in args.tag or TAGS:
        found = fixtures(tag)
        same = explained = bad = 0
        notes = []
        for path, blob in sorted(found.items()):
            now = blob_of(args.ref, path)
            if now == blob:
                same += 1
            elif path.startswith(IGNORED_PREFIXES):
                explained += 1
            elif path in EXPLAINED:
                explained += 1
                notes.append(f"  explained {path}: {EXPLAINED[path]}")
            else:
                bad += 1
                notes.append(f"  FAILED    {path}: {'missing' if now is None else 'changed'} and not explained")
        failed |= bad > 0
        print(f"{tag:16} {len(found):8} {same:9} {explained:9} {bad:6}")
        for note in notes:
            print(note)
    if not failed:
        print("every format fixture of the betas reads as before, or is explained")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

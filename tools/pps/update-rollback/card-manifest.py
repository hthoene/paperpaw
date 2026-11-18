#!/usr/bin/env python3
"""What an update or a rollback did to a card (#229): a manifest of the card's
files (SHA-256, size) before, another after, and a verdict.

    card-manifest.py snapshot CARD_DIR > before.json     (the card mounted on a computer)
    card-manifest.py diff before.json after.json         (exit 1: something of the user's is gone or changed)

Books and plugin files must be byte-identical. A file may go (or change) only
where an update or a rollback moves it by design (EXPECTED below: the reader's
old position and annotation files move to the shared folders, a password file
of an older plugin moves into secrets.txt, caches and the update file are
made again or let go); those are listed, not failed. Everything else gone or
changed fails. New files are listed.
"""

import hashlib
import json
import os
import re
import sys

# Gone or changed by design (docs/data-files.md); regexes on the path from the card's top.
EXPECTED = [
    (r"\.paperpaw/plugins/reader/books/[0-9a-f]+/(progress\.ini|annotations\.txt)", "moved to the shared positions/annotations folder"),
    (r"\.paperpaw/plugins/(stats/webdav|kosync/account)\.ini", "its password moves into secrets.txt"),
    (r"\.paperpaw/plugins/opds/catalogs\.txt", "a password in it moves into secrets.txt"),
    (r"\.paperpaw/secrets\.txt", "rewritten on a save (sealed on Beta 4, not readable by Beta 3: enter passwords again after a rollback)"),
    (r"paperpaw\.ini", "rewritten on a settings save (unknown sections must stay: checked by the host test)"),
    (r"\.paperpaw/(positions|annotations)/.*", "the book's own place and highlights: changes as it is read"),
    (r"\.paperpaw/(library/(thumbs|covers|catalog.*|status\.log|walk\.list)|plugins/(format-[a-z]+|update|sleep|battery|diagnostics|picture-frame/cache.*)/.*|slept|crashes\.txt|power\.ini|cleaned-up\.txt)", "cache, log or state made again"),
    (r"updates/.*", "the update file is let go once kept"),
]
# Never allowed to change or go.
STRICT = [r"books/.*", r"plugins/.*", r"Flashcards/.*"]


def snapshot(root):
    files = {}
    for here, _, names in os.walk(root):
        for name in names:
            path = os.path.join(here, name)
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            data = open(path, "rb").read()
            files[rel] = {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
    return files


def why(path):
    return next((note for pattern, note in EXPECTED if re.fullmatch(pattern, path)), None)


def diff(before, after):
    bad = []
    for path, info in sorted(before.items()):
        now = after.get(path)
        if now == info:
            continue
        kind = "gone" if now is None else "changed"
        strict = any(re.fullmatch(p, path) for p in STRICT)
        note = why(path)
        if strict or note is None:
            bad.append(f"LOST    {kind}: {path}")
        else:
            print(f"expected {kind}: {path} ({note})")
    for path in sorted(set(after) - set(before)):
        print(f"new      {path}")
    for line in bad:
        print(line)
    return 1 if bad else 0


def main(argv):
    if len(argv) == 3 and argv[1] == "snapshot":
        json.dump(snapshot(argv[2]), sys.stdout, indent=1, sort_keys=True)
        print()
        return 0
    if len(argv) == 4 and argv[1] == "diff":
        return diff(*(json.load(open(p)) for p in argv[2:]))
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))

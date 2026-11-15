#!/usr/bin/env python3
"""The text of a release page: `docs/release-notes/<version>.md`.

    check-release-notes.py FILE

Exit 0 and the length printed where FILE is UTF-8, has something besides white
space in it and is at most 125,000 characters (GitHub's limit for a release's
text); exit 1 with a message otherwise (`release.yml`, the guard and the last
job).
"""
import pathlib
import sys

LIMIT = 125_000


def problem(path):
    """What is wrong with the notes at `path`, or None; and their length."""
    path = pathlib.Path(path)
    if not path.is_file():
        return f"no {path}: the release page's text is written for the release", 0
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return f"{path} is not UTF-8", 0
    if not text.lstrip("\ufeff").strip():
        return f"{path} is empty", 0
    if len(text) > LIMIT:
        return f"{path} is {len(text)} characters: GitHub takes at most {LIMIT}", len(text)
    return None, len(text)


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    found, length = problem(sys.argv[1])
    if found:
        print(f"::error::{found}")
        sys.exit(1)
    print(f"The release page's text: {length} characters (at most {LIMIT})")


if __name__ == "__main__":
    main()

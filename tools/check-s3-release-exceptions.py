#!/usr/bin/env python3
"""The release check for the S3 linked-plugin frame budget (todo 639, Manager ruling).

    check-s3-release-exceptions.py [BUDGET]

BUDGET defaults to shells/xteink/frame-budget-s3-linked.txt. An owner test image or
a release image of the X4 Pro or X4 Classic with native plugins may leave the team
only when no `allow frame` line is left but `main` (the firmware's entry on the main
task's stack, which the start-stack check counts in every route). Every other line
names a callback frame of 2,048 B or more that is still waiting to be cut.

Prints each remaining exception, then CHECK_S3_RELEASE_EXCEPTIONS_EXIT=<0|1>.
Exit status: 0 none left but `main`, 1 some left, 2 usage or no such file.
"""
import sys
from pathlib import Path

DEFAULT = Path(__file__).resolve().parent.parent / "shells/xteink/frame-budget-s3-linked.txt"
PERMANENT = {"main"}


def exceptions(text: str) -> list[tuple[int, str, str]]:
    """The `allow frame <function> <bytes> <why>` lines: (line number, function, rest)."""
    found = []
    for number, line in enumerate(text.splitlines(), 1):
        words = line.split(None, 4)
        if len(words) >= 3 and words[0] == "allow" and words[1] == "frame":
            found.append((number, words[2], " ".join(words[3:])))
    return found


def left(text: str) -> list[tuple[int, str, str]]:
    return [e for e in exceptions(text) if e[1] not in PERMANENT]


def main(argv: list[str]) -> int:
    if len(argv) > 1 or argv[:1] in (["-h"], ["--help"]):
        print(__doc__)
        return 2
    path = Path(argv[0]) if argv else DEFAULT
    try:
        text = path.read_text()
    except OSError as e:
        print(f"{path}: {e}")
        return 2
    remaining = left(text)
    for number, function, rest in remaining:
        print(f"{path}:{number}: still an exception: {function} {rest[:120]}")
    print(f"{len(remaining)} exception line(s) left besides `main`")
    print(f"CHECK_S3_RELEASE_EXCEPTIONS_EXIT={1 if remaining else 0}")
    return 1 if remaining else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

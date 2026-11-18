#!/usr/bin/env python3
"""Hold an X4-family install image against its size budget (ticket #258).

    size-report.py check --device x4 --image install.bin [--budget FILE]

The budget file (`shells/xteink/size-budget.txt`) has one rule per line:
`image <device> <bytes>` is the most the install image of that device may
weigh; `layer <device> <name> <bytes>` is read and checked for its form but not
yet held against anything; `#` starts a comment. Any other line is an error: a
typing mistake must not switch a check off.

Exit codes: 0 within budget, 1 over budget, 2 the check could not be made (no
budget file, no image, no line for the device, a line that is not understood).
Nothing here skips quietly.
"""

import argparse
import os
import sys

DEFAULT_BUDGET = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "shells", "xteink", "size-budget.txt"
)
DEVICES = ("x4", "x3", "x4-pro", "x4-classic")


class Unusable(Exception):
    """The check cannot be made (exit code 2)."""


def _bytes(word, where):
    if not word.isdigit():
        raise Unusable(f"{where}: `{word}` is not a number of bytes")
    return int(word)


def read_budget(path):
    """The budget's `image` lines as {device: bytes}; `layer` lines are checked and dropped."""
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except OSError as e:
        raise Unusable(f"the size budget {path} cannot be read: {e}")
    images = {}
    for number, raw in enumerate(lines, 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        where = f"{path}:{number}"
        words = line.split()
        if words[0] == "image":
            if len(words) != 3:
                raise Unusable(f"{where}: expected `image <device> <bytes>`")
            if words[1] not in DEVICES:
                raise Unusable(f"{where}: unknown device `{words[1]}` (one of {', '.join(DEVICES)})")
            if words[1] in images:
                raise Unusable(f"{where}: a second `image` line for {words[1]}")
            images[words[1]] = _bytes(words[2], where)
        elif words[0] == "layer":
            if len(words) != 4:
                raise Unusable(f"{where}: expected `layer <device> <name> <bytes>`")
            if words[1] not in DEVICES:
                raise Unusable(f"{where}: unknown device `{words[1]}`")
            _bytes(words[3], where)
        else:
            raise Unusable(f"{where}: unknown kind of line `{words[0]}`")
    return images


def check(device, image, budget_path):
    """Returns (exit code, message)."""
    images = read_budget(budget_path)
    if device not in images:
        raise Unusable(f"{budget_path} has no `image {device}` line")
    if not os.path.isfile(image):
        raise Unusable(f"the install image {image} does not exist")
    size = os.path.getsize(image)
    if size == 0:
        raise Unusable(f"the install image {image} is empty")
    budget = images[device]
    room = budget - size
    line = f"{device} install image: {size} bytes, budget {budget}, room {room}"
    if room < 0:
        return 1, f"{line}: {-room} bytes over its budget (raise the line only as the file says)"
    return 0, line


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    one = sub.add_parser("check", help="hold one image against its budget line")
    one.add_argument("--device", required=True, choices=DEVICES)
    one.add_argument("--image", required=True)
    one.add_argument("--budget", default=DEFAULT_BUDGET)
    args = parser.parse_args(argv)
    try:
        code, message = check(args.device, args.image, args.budget)
    except Unusable as e:
        print(f"::error::size gate: {e}")
        return 2
    print(("::error::" if code else "::notice::") + message)
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

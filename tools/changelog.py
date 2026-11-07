#!/usr/bin/env python3
"""Check PR fragments and collect them into a release before tagging."""

import argparse
from datetime import date
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
SECTIONS = ("Added", "Changed", "Deprecated", "Removed", "Fixed", "Security")
FRAGMENT_SECTIONS = ("Added", "Changed", "Fixed")
NAME = re.compile(r"(?:pr-)?[1-9][0-9]*-[a-z0-9]+(?:-[a-z0-9]+)*\.md")
VERSION = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


def sections(text, allowed, source):
    """Read headings and bullets, rejecting prose that would be lost on collection."""
    if any(ord(char) < 32 and char not in "\n\r\t" for char in text):
        raise ValueError(f"{source}: control character in text")
    result = {}
    current = None
    for line in text.splitlines():
        if any(ord(char) < 32 and char != "\t" for char in line):
            raise ValueError(f"{source}: control character in text")
        if line.startswith("### "):
            current = line[4:]
            if current not in allowed or current in result:
                raise ValueError(f"{source}: unknown or repeated section {current!r}")
            result[current] = []
        elif line.strip():
            if current is None:
                raise ValueError(f"{source}: expected ### Added, ### Changed or ### Fixed")
            if line.startswith("- ") and line[2:].strip():
                result[current].append(line)
            elif line.startswith("  ") and any(s.strip() for s in result[current]):
                result[current].append(line)
            else:
                raise ValueError(f"{source}: expected a bullet or indented continuation")
        elif current is not None:
            result[current].append("")
    for heading, lines in result.items():
        if not any(line.startswith("- ") for line in lines):
            raise ValueError(f"{source}: empty section {heading}")
    return {heading: "\n".join(lines).strip() for heading, lines in result.items()}


def fragments(root):
    found = []
    for path in sorted((root / "changelog.d").iterdir()):
        if path.name == "README.md":
            continue
        if path.is_symlink() or not path.is_file() or not NAME.fullmatch(path.name):
            raise ValueError(f"{path}: use <ticket>-<slug>.md or pr-<number>-<slug>.md")
        parsed = sections(path.read_text(encoding="utf-8"), FRAGMENT_SECTIONS, path)
        if not parsed:
            raise ValueError(f"{path}: empty fragment")
        found.append((path, parsed))
    return sorted(found, key=lambda item: (int(re.search(r"[0-9]+", item[0].name)[0]), item[0].name))


def git(root, *args, text=None):
    return subprocess.run(
        ["git", *args], cwd=root, input=text, text=True, encoding="utf-8",
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True,
    ).stdout


def documentation(path):
    return (
        path.endswith(".md") or path.startswith(("docs/", ".claude/", "changelog.d/"))
    )


def check(root, base=None, release=False):
    found = fragments(root)
    if release and found:
        raise ValueError("uncollected fragments: run collect --version <version> before tagging")
    if base is None:
        return
    ancestor = git(root, "merge-base", base, "HEAD").strip()
    changed = git(root, "diff", "--name-only", "-z", ancestor, "HEAD").split("\0")
    if all(documentation(path) for path in changed if path):
        return
    added = set(git(root, "diff", "--name-only", "--diff-filter=A", "-z", ancestor, "HEAD").split("\0"))
    if any(path.relative_to(root).as_posix() in added for path, _ in found):
        return
    message = git(root, "log", "-1", "--format=%B")
    trailers = git(root, "interpret-trailers", "--parse", text=message)
    if any(re.fullmatch(r"No-changelog:\s*\S.*", line, re.IGNORECASE) for line in trailers.splitlines()):
        return
    raise ValueError("add a changelog.d fragment or a No-changelog: <reason> trailer to the head commit")


def collected(text, found, version, day):
    if not VERSION.fullmatch(version):
        raise ValueError("version must be a semantic version without a leading v")
    prerelease = version.split("+", 1)[0].partition("-")[2]
    if any(part.isdigit() and len(part) > 1 and part.startswith("0") for part in prerelease.split(".")):
        raise ValueError("numeric prerelease identifiers must not have leading zeros")
    if re.search(r"^## \[" + re.escape(version) + r"\]", text, re.MULTILINE):
        raise ValueError(f"release {version} already exists; no files changed")
    heads = list(re.finditer(r"^## \[([^\]\n]+)\][^\n]*$", text, re.MULTILINE))
    if not heads or heads[0][1] != "Unreleased" or any(h[1] == "Unreleased" for h in heads[1:]):
        raise ValueError("CHANGELOG.md must start with exactly one ## [Unreleased] section")
    start = heads[0].start()
    end = heads[1].start() if len(heads) > 1 else len(text)
    merged = sections(text[heads[0].end():end], SECTIONS, "CHANGELOG.md Unreleased")
    for _, parsed in found:
        for heading, body in parsed.items():
            merged[heading] = (merged.get(heading, "") + "\n\n" + body).strip()
    if not merged:
        raise ValueError("nothing to release")
    body = "\n\n".join(f"### {heading}\n\n{merged[heading]}" for heading in SECTIONS if heading in merged)
    return text[:start] + f"## [Unreleased]\n\n## [{version}] - {day}\n\n{body}\n\n" + text[end:]


def collect(root, version, day):
    found = fragments(root)
    path = root / "CHANGELOG.md"
    if path.is_symlink():
        raise ValueError("CHANGELOG.md must not be a symlink")
    updated = collected(path.read_text(encoding="utf-8"), found, version, day)
    # Publish the complete release before removing its sources. If interrupted
    # during cleanup, the release remains and the remaining fragments are visible.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=root, delete=False) as stream:
            temporary = stream.name
            stream.write(updated)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, path.stat().st_mode & 0o777)
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)
    for fragment, _ in found:
        fragment.unlink()
    print(f"Collected {len(found)} fragment(s) into {version}; review git diff before committing.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    checker = commands.add_parser("check", help="validate fragments and optionally a PR diff")
    checker.add_argument("--base", help="compare HEAD with the merge base of this git ref")
    checker.add_argument("--release", action="store_true", help="require no uncollected fragments")
    collector = commands.add_parser("collect", help="cut a dated release from Unreleased and fragments")
    collector.add_argument("--version", required=True)
    collector.add_argument("--date", type=date.fromisoformat, default=date.today())
    args = parser.parse_args()
    try:
        if args.command == "check":
            check(ROOT, args.base, args.release)
        else:
            collect(ROOT, args.version, args.date)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"changelog: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

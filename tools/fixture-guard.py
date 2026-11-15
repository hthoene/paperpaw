#!/usr/bin/env python3
"""The changed lines of a test port that are not fixtures (#244 safety net, recipe Q2).

    tools/fixture-guard.py BASE HEAD [--builder-from RE] [--builder-to RE] [--root DIR]

For every changed `.rs` file with test code, prints each changed line that is
not inside a *fixture block* (an `impl Plugin for X`, `impl Screen for X` or
`impl Layer for X` item, the `struct`/`enum` X it is for, and `use` lines), as

    path:line: -old text            (line numbers of BASE for `-`, of HEAD for `+`)

and every change of the number of `#[test]`, `#[ignore`, `#[cfg` and
`#[should_panic` lines of a file. The output goes into the port PR's text; an
empty list is the only one the recipe allows: the exit code is 1 when anything
is printed.

Test code is a whole file under `tests/` (or named `tests.rs`, `*_tests.rs`,
`test_*.rs`, in a `test_support` folder) or the `#[cfg(test)] mod` blocks of a
source file; of such a source file the part outside those blocks is shown too
when it changed (`OUTSIDE cfg(test)`).

The builder-call lines (`.plugin(...)` replaced by the new call) are one fixed
allowed pattern: a removed line matching `--builder-from` (default
`\\.plugin\\(`) paired in its hunk with an added line matching `--builder-to`
(default `\\.native_plugin\\(`; set it to the real new call when D4a starts) is
counted per file and not printed.
"""
import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURE_TRAITS = ("Plugin", "Screen", "Layer")
ATTRIBUTES = ("#[test]", "#[ignore", "#[cfg", "#[should_panic")
TEST_PATH = re.compile(r"(^|/)(tests?|test_support|test-support)/|(^|/)(tests?|[\w-]+_tests?|test_[\w-]+)\.rs$")


def mask(text):
    """`text` with comments and the insides of strings and char literals blanked
    (newlines kept), so that braces can be counted."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        two = text[i:i + 2]
        if two == "//":
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth:
                if text[j:j + 2] == "/*":
                    depth, j = depth + 1, j + 2
                elif text[j:j + 2] == "*/":
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append("".join("\n" if ch == "\n" else " " for ch in text[i:j]))
            i = j
        elif c == "r" and re.match(r'r#*"', text[i:i + 12]) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            hashes = len(re.match(r"r(#*)", text[i:]).group(1))
            end = text.find('"' + "#" * hashes, i + 2 + hashes)
            end = n if end < 0 else end + 1 + hashes
            out.append("".join("\n" if ch == "\n" else " " for ch in text[i:end]))
            i = end
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            j = min(j + 1, n)
            out.append('"' + "".join("\n" if ch == "\n" else " " for ch in text[i + 1:j - 1]) + '"')
            i = j
        elif c == "'":
            literal = re.match(r"'(?:\\.[^']*|[^\\'])'", text[i:i + 12])
            if literal:
                out.append("' '" if False else " " * len(literal.group(0)))
                i += len(literal.group(0))
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def block_end(lines, start):
    """The index of the line that closes the first `{` at or after line `start`
    (or ends the item with `;` before any `{`); `start` if neither is found."""
    depth, opened = 0, False
    for index in range(start, len(lines)):
        for ch in lines[index]:
            if ch == "{":
                depth, opened = depth + 1, True
            elif ch == "}":
                depth -= 1
                if opened and depth == 0:
                    return index
            elif ch == ";" and not opened and depth == 0:
                return index
    return start


def attributes_above(lines, index):
    """The first line of the attributes and doc comments just above `index`."""
    first = index
    while first > 0 and re.match(r"\s*(#\[|///|//!|#!\[)", lines[first - 1]):
        first -= 1
    return first


def regions(text, fixture_traits=FIXTURE_TRAITS):
    """`(fixture, test_code)`: sets of 0-based line numbers of `text`."""
    raw = text.split("\n")
    masked = mask(text).split("\n")
    fixture, test_code = set(), set()
    types = set()
    impl = re.compile(r"^\s*(?:unsafe\s+)?impl(?:<[^>]*>)?\s+(?:[\w:]+::)?(?:%s)(?:<[^>]*>)?\s+for\s+(?P<ty>[\w:]+)" % "|".join(fixture_traits))
    for index, line in enumerate(masked):
        found = impl.match(line)
        if found:
            end = block_end(masked, index)
            fixture.update(range(attributes_above(masked, index), end + 1))
            types.add(found.group("ty").split("::")[-1])
        elif re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?use\s", line):
            end = block_end(masked, index)
            fixture.update(range(index, end + 1))
    for index, line in enumerate(masked):
        definition = re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum)\s+(\w+)", line)
        if definition and definition.group(1) in types:
            end = block_end(masked, index)
            fixture.update(range(attributes_above(masked, index), end + 1))
        if re.match(r"\s*#\[cfg\(test\)\]", line):
            nxt = index + 1
            while nxt < len(masked) and re.match(r"\s*#\[", masked[nxt]):
                nxt += 1
            if nxt < len(masked) and re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{", masked[nxt]):
                test_code.update(range(index, block_end(masked, nxt) + 1))
    return fixture, test_code, len(raw)


def counts(text, within=None):
    """How many lines start each attribute of ATTRIBUTES (only `within` line
    numbers when given)."""
    out = {a: 0 for a in ATTRIBUTES}
    for index, line in enumerate(text.split("\n")):
        if within is not None and index not in within:
            continue
        stripped = line.strip()
        for attribute in ATTRIBUTES:
            if stripped.startswith(attribute):
                out[attribute] += 1
    return out


def hunks(diff):
    """`[(old_start, [removed], new_start, [added])]` of a `git diff -U0`."""
    out, current = [], None
    for line in diff.split("\n"):
        header = re.match(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", line)
        if header:
            current = (int(header.group(1)), [], int(header.group(3)), [])
            out.append(current)
        elif current and line.startswith("-") and not line.startswith("---"):
            current[1].append(line[1:])
        elif current and line.startswith("+") and not line.startswith("+++"):
            current[3].append(line[1:])
    return out


def git(root, *args):
    done = subprocess.run(["git", *args], cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return done.returncode, done.stdout


def file_at(root, ref, path):
    code, text = git(root, "show", f"{ref}:{path}")
    return text if code == 0 else ""


def is_test_path(path):
    return bool(TEST_PATH.search(path))


def guard(root, base, head, builder_from=r"\.plugin\(", builder_to=r"\.native_plugin\(", fixture_traits=FIXTURE_TRAITS):
    """`(listed, builders)`: the lines to list and the count of builder-call
    lines per file."""
    code, names = git(root, "diff", "--name-only", "--no-renames", base, head)
    if code != 0:
        raise SystemExit(f"git diff {base} {head} failed")
    frm, to = re.compile(builder_from), re.compile(builder_to)
    listed, builders = [], {}
    for path in names.split():
        if not path.endswith(".rs"):
            continue
        old, new = file_at(root, base, path), file_at(root, head, path)
        old_fix, old_tests, old_n = regions(old, fixture_traits)
        new_fix, new_tests, new_n = regions(new, fixture_traits)
        whole = is_test_path(path)
        if whole:
            old_tests, new_tests = set(range(old_n)), set(range(new_n))
        if not old_tests and not new_tests:
            continue
        _, diff = git(root, "diff", "-U0", "--no-renames", base, head, "--", path)
        for old_start, removed, new_start, added in hunks(diff):
            pairs = []
            for r_index, r_line in enumerate(removed):
                if frm.search(r_line):
                    for a_index, a_line in enumerate(added):
                        if to.search(a_line) and a_index not in [p[1] for p in pairs]:
                            pairs.append((r_index, a_index))
                            break
            paired_removed = {p[0] for p in pairs}
            paired_added = {p[1] for p in pairs}
            builders[path] = builders.get(path, 0) + len(pairs)
            for k, line in enumerate(removed):
                number = old_start + k   # 1-based line in BASE (a pure deletion has old_start the line before)
                if k in paired_removed or (number - 1) in old_fix:
                    continue
                where = "" if (number - 1) in old_tests else " OUTSIDE cfg(test)"
                listed.append(f"{path}:{number}:{where} -{line}")
            for k, line in enumerate(added):
                number = new_start + k
                if k in paired_added or (number - 1) in new_fix:
                    continue
                where = "" if (number - 1) in new_tests else " OUTSIDE cfg(test)"
                listed.append(f"{path}:{number}:{where} +{line}")
        before = counts(old, None if whole else old_tests)
        after = counts(new, None if whole else new_tests)
        for attribute in ATTRIBUTES:
            if before[attribute] != after[attribute]:
                listed.append(f"{path}: number of `{attribute}` lines {before[attribute]} -> {after[attribute]}")
    return listed, {p: n for p, n in builders.items() if n}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("base")
    parser.add_argument("head")
    parser.add_argument("--root", default=str(ROOT))
    parser.add_argument("--builder-from", default=r"\.plugin\(")
    parser.add_argument("--builder-to", default=r"\.native_plugin\(")
    parser.add_argument("--fixture-impl", action="append", help="a trait whose `impl ... for` is a fixture (default: Plugin, Screen, Layer)")
    args = parser.parse_args()
    listed, builders = guard(args.root, args.base, args.head, args.builder_from, args.builder_to, tuple(args.fixture_impl or FIXTURE_TRAITS))
    for line in listed:
        print(line)
    total = sum(builders.values())
    if total:
        print(f"builder-call lines (allowed pattern, not listed): {total} in {len(builders)} file(s)")
    print(f"{len(listed)} line(s) to explain" if listed else "nothing to explain")
    return 1 if listed else 0


if __name__ == "__main__":
    sys.exit(main())

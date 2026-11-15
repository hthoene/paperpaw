#!/usr/bin/env python3
"""The names of the tests CI runs, per test set (#244 safety net, recipe Q1).

    tools/test-names.py --list-sets
    tools/test-names.py --write [--set ID ...]
    tools/test-names.py --check [--set ID ...]

A *set* is one `cargo test` line of `.github/workflows/ci.yml` (flags that do not
change what is tested are dropped; equal lines count once), the SDK's tests with
`kernel` on, and a static scan of the firmware crate (`shells/xteink`, outside
the workspace: it does not build on the host). For each set the tests are
listed with `cargo test <set> -- --list --format terse` and again with
`--ignored`; a line is written for each as

    binary<TAB>test<TAB>run|ignored

to `tools/ci/test-names-244/<set>.txt`. `binary` is `package/target` without the
build hash (`paperpaw-app/flows`); doc tests are `package/doc`, their names
without the line number. The sets and their commands are in
`tools/ci/test-names-244/sets.txt`.

`--check` lists again and compares: a name that is not written yet is
reported; a name that is gone, or that was run and is ignored now, fails unless
`tools/ci/test-names-244/removed.txt` has a line `binary<TAB>test<TAB>reason`
for it. A set in `ci.yml` that is not in `sets.txt` (or the reverse) fails:
run `--write`. Needs the toolchains of the CI jobs and builds each set (heavy:
use `~/bin/heavy-slot.sh`).
"""
import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CI = ROOT / ".github" / "workflows" / "ci.yml"
DIR = ROOT / "tools" / "ci" / "test-names-244"

# Flags of a `cargo test` line that change how it runs, not what it tests.
DROP_FLAGS = {"--no-fail-fast", "-q", "--quiet", "--no-run"}
DROP_WITH_VALUE = {"-j", "--jobs"}
# Harness flags after `--` that are not a filter of names.
DROP_HARNESS = {"--ignored", "--nocapture", "--show-output", "--include-ignored", "--exact"}

# Sets that CI has no `cargo test` line for, but the net needs (R2).
EXTRA_SETS = [
    ("sdk-kernel", ["-p", "paperpaw-sdk", "--features", "kernel"]),
]
# Crates outside the workspace that do not build on the host: their tests are
# found in the source (`#[test]`, `#[ignore]`).
STATIC_SETS = [("shells-xteink-static", "shells/xteink")]

CARGO_TEST = re.compile(
    r"^\s*(?:-\s*)?(?:run:\s*)?(?:[A-Za-z_][A-Za-z0-9_]*=\S+\s+)*cargo(?:\s+\+\S+)?\s+test\s+(?P<args>.*?)\s*$"
)


def normalize(args):
    """The tokens of a `cargo test` line without what only changes how it runs.
    Returns `(cargo_args, harness_args)`."""
    if "--" in args:
        at = args.index("--")
        before, after = args[:at], args[at + 1:]
    else:
        before, after = list(args), []
    kept, skip = [], False
    for token in before:
        if skip:
            skip = False
            continue
        if token in DROP_FLAGS:
            continue
        if token in DROP_WITH_VALUE:
            skip = True
            continue
        kept.append(token)
    harness = [t for t in after if t not in DROP_HARNESS]
    # `--exact` is a filter mode: keep it when a name follows
    if "--exact" in after and harness:
        harness.insert(0, "--exact")
    return kept, harness


def slug(kept, harness):
    """A stable id: readable words and a short hash of the whole line."""
    words = []
    take = False
    for token in kept:
        if token in ("-p", "--package", "--test", "--features", "--manifest-path"):
            take = True
            continue
        if take:
            words.append(token.replace("paperpaw-", "").replace(",", "+").split("/")[-2 if token.endswith("Cargo.toml") else -1])
            take = False
        elif token == "--workspace":
            words.append("workspace")
        elif token == "--lib":
            words.append("lib")
        elif token == "--no-default-features":
            words.append("nodefault")
    digest = hashlib.sha1(" ".join(kept + ["--"] + harness).encode()).hexdigest()[:6]
    text = re.sub(r"[^a-z0-9+_-]+", "-", "-".join(words).lower()).strip("-")[:28] or "set"
    return f"{text}-{digest}"


def ci_sets(text):
    """The sets of the `cargo test` lines of ci.yml, in order, each once."""
    seen, out = set(), []
    for line in text.splitlines():
        if line.lstrip().startswith("#"):
            continue
        found = CARGO_TEST.match(line)
        if not found:
            continue
        kept, harness = normalize(found.group("args").split())
        key = (tuple(kept), tuple(harness))
        if key in seen:
            continue
        seen.add(key)
        out.append((slug(kept, harness), kept, harness))
    return out


def all_sets(ci_text):
    sets = ci_sets(ci_text)
    for name, kept in EXTRA_SETS:
        sets.append((slug(kept, []), kept, []))
    return sets


def describe(sets):
    """The text of sets.txt."""
    lines = ["# set<TAB>cargo test arguments (<TAB>-- harness filters); written by tools/test-names.py --write"]
    for sid, kept, harness in sets:
        lines.append("\t".join([sid, " ".join(kept)] + ([" ".join(harness)] if harness else [])))
    for sid, folder in STATIC_SETS:
        lines.append(f"{sid}\tstatic: {folder}")
    return "\n".join(lines) + "\n"


# Listing -----------------------------------------------------------------

RUNNING = re.compile(r"^\s*Running .*\((?P<path>[^)]+)\)\s*$")
DOCTESTS = re.compile(r"^\s*Doc-tests (?P<crate>\S+)\s*$")
LISTED = re.compile(r"^(?P<name>.+): (?:test|benchmark)$")
LINE_NO = re.compile(r" \(line \d+\)")


def binary_of(path, exe_map):
    """`package/target` of an executable path, from cargo's JSON map."""
    path = os.path.normpath(path)
    for exe, label in exe_map.items():
        if os.path.normpath(exe) == path or os.path.normpath(exe).endswith(path):
            return label
    stem = re.sub(r"-[0-9a-f]{16}$", "", os.path.basename(path))
    return stem


def exe_map_of(json_lines):
    """executable path -> `package/target` from `--message-format=json`."""
    out = {}
    for line in json_lines:
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-artifact" or not message.get("executable"):
            continue
        if not message.get("profile", {}).get("test"):
            continue
        package = message["package_id"]
        pkg = None
        if "#" in package:
            tail = package.rsplit("#", 1)[1]
            pkg = tail.split("@")[0] if "@" in tail else package.rsplit("/", 1)[-1].split("#")[0]
        pkg = pkg or "?"
        target = message["target"]
        kind = target["kind"][0] if target.get("kind") else "test"
        label = f"{pkg}/{'lib' if kind in ('lib', 'rlib', 'cdylib', 'proc-macro') else target['name']}"
        if kind == "bin":
            label = f"{pkg}/bin-{target['name']}"
        out[message["executable"]] = label
    return out


def parse_listing(text, exe_map):
    """The `(binary, test)` pairs a `-- --list --format terse` run printed."""
    pairs, binary = [], None
    for line in text.splitlines():
        running = RUNNING.match(line)
        if running:
            binary = binary_of(running.group("path"), exe_map)
            continue
        doc = DOCTESTS.match(line)
        if doc:
            binary = doc.group("crate").replace("_", "-") + "/doc"
            continue
        listed = LISTED.match(line)
        if listed and binary:
            name = listed.group("name")
            if binary.endswith("/doc"):
                name = LINE_NO.sub("", name)
            pairs.append((binary, name))
    return pairs


def lines_of(run, ignored):
    """The file lines of one set: sorted, `binary<TAB>test<TAB>run|ignored`."""
    ignored = set(ignored)
    out = set()
    for pair in set(run) | ignored:
        out.add("\t".join([pair[0], pair[1], "ignored" if pair in ignored else "run"]))
    return sorted(out)


def static_lines(folder):
    """Tests of a crate that does not build on the host: `#[test]` functions."""
    out = set()
    for path in sorted((ROOT / folder).rglob("*.rs")):
        if "target" in path.parts:
            continue
        rel = path.relative_to(ROOT / folder).as_posix()
        pending, ignore = False, False
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            stripped = line.strip()
            if stripped.startswith("#[test]"):
                pending = True
            elif stripped.startswith("#[ignore"):
                ignore = pending or ignore
            found = re.match(r"(?:pub\s+)?fn\s+(\w+)", stripped)
            if pending and found:
                out.add("\t".join([f"{folder}/static", f"{rel}::{found.group(1)}", "ignored" if ignore else "run"]))
                pending, ignore = False, False
    return sorted(out)


# Comparing ---------------------------------------------------------------

def parse_lines(text):
    out = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) >= 3:
            out[(parts[0], parts[1])] = parts[2]
    return out


def parse_removed(text):
    out = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) >= 3 and parts[2].strip():
            out[(parts[0], parts[1])] = parts[2].strip()
    return out


def compare(old, new, removed):
    """`(added, failures, notes)` of the names `new` against the written `old`.
    A name gone, or run before and ignored now, fails unless `removed` has it."""
    added = sorted(k for k in new if k not in old)
    failures, notes = [], []
    for key in sorted(old):
        if key not in new:
            if key not in removed:
                failures.append(f"missing: {key[0]}\t{key[1]}")
        elif old[key] == "run" and new[key] == "ignored":
            if key not in removed:
                failures.append(f"now ignored: {key[0]}\t{key[1]}")
        elif old[key] == "ignored" and new[key] == "run":
            notes.append(f"no longer ignored: {key[0]}\t{key[1]}")
    return added, failures, notes


# Running cargo --------------------------------------------------------------

def cargo(args, env_extra=None, capture_json=False):
    env = dict(os.environ)
    env.update(env_extra or {})
    cmd = ["cargo", "test", *args]
    return subprocess.run(
        cmd, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE if capture_json else subprocess.STDOUT,
    )


def list_set(kept, harness):
    """`(run, ignored)` pairs of one set. Raises on a build or listing failure."""
    build = subprocess.run(
        ["cargo", "test", *kept, "--no-run", "--message-format=json"],
        cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    if build.returncode != 0:
        raise RuntimeError(f"build of `cargo test {' '.join(kept)}` failed:\n{build.stderr[-3000:]}")
    exe_map = exe_map_of(build.stdout.splitlines())
    tail = ["--", "--list", "--format", "terse", *harness]
    run = cargo([*kept, *tail])
    if run.returncode != 0:
        raise RuntimeError(f"listing `{' '.join(kept)}` failed:\n{run.stdout[-3000:]}")
    ign = cargo([*kept, *tail, "--ignored"])
    if ign.returncode != 0:
        raise RuntimeError(f"listing ignored of `{' '.join(kept)}` failed:\n{ign.stdout[-3000:]}")
    run_pairs = parse_listing(run.stdout, exe_map)
    ign_pairs = parse_listing(ign.stdout, exe_map)
    if not run_pairs:
        raise RuntimeError(f"`cargo test {' '.join(kept)}` listed no test at all")
    return run_pairs, ign_pairs


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--list-sets", action="store_true")
    parser.add_argument("--set", action="append", default=[], help="only this set (id), repeatable")
    args = parser.parse_args()

    sets = all_sets(CI.read_text(encoding="utf-8"))
    if args.list_sets:
        sys.stdout.write(describe(sets))
        return 0

    ids = [s[0] for s in sets] + [s[0] for s in STATIC_SETS]
    unknown = [s for s in args.set if s not in ids]
    if unknown:
        print(f"unknown set: {', '.join(unknown)} (see --list-sets)", file=sys.stderr)
        return 2
    wanted = args.set or ids

    DIR.mkdir(parents=True, exist_ok=True)
    problems = []
    if args.check:
        written = (DIR / "sets.txt").read_text(encoding="utf-8") if (DIR / "sets.txt").exists() else ""
        if written != describe(sets):
            problems.append("the `cargo test` lines of ci.yml are not the sets of tools/ci/test-names-244/sets.txt: run --write")
    else:
        (DIR / "sets.txt").write_text(describe(sets), encoding="utf-8")

    removed = parse_removed((DIR / "removed.txt").read_text(encoding="utf-8")) if (DIR / "removed.txt").exists() else {}
    errors = 0
    for sid, kept, harness in sets:
        if sid not in wanted:
            continue
        try:
            run, ign = list_set(kept, harness)
        except RuntimeError as error:
            print(f"ERROR {sid}: {error}", file=sys.stderr)
            errors += 1
            continue
        report(sid, lines_of(run, ign), args, removed, problems)
    for sid, folder in STATIC_SETS:
        if sid in wanted:
            report(sid, static_lines(folder), args, removed, problems)
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems or errors else 0


def report(sid, lines, args, removed, problems):
    path = DIR / f"{sid}.txt"
    if args.write:
        path.write_text("".join(line + "\n" for line in lines), encoding="utf-8")
        print(f"{sid}: {len(lines)} names written")
        return
    old = parse_lines(path.read_text(encoding="utf-8")) if path.exists() else {}
    if not old:
        problems.append(f"{sid}: no written names (run --write)")
        return
    added, failures, notes = compare(old, parse_lines("\n".join(lines)), removed)
    for key in added:
        print(f"{sid}: added {key[0]}\t{key[1]}")
    for note in notes:
        print(f"{sid}: {note}")
    for failure in failures:
        problems.append(f"{sid}: {failure}")
    print(f"{sid}: {len(lines)} names, {len(added)} added, {len(failures)} failing")


if __name__ == "__main__":
    sys.exit(main())

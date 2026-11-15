#!/usr/bin/env python3
"""The canary runner of #244 (safety net, recipe Q3). Use `run.sh`, which makes
the worktree; this is what it runs.

    canaries.py list FILE
    canaries.py apply-check --tree DIR [--file FILE]      # every canary applies (no build)
    canaries.py run   --tree DIR [options]                # write the `.fails` files
    canaries.py check --tree DIR [options]                # compare with the committed ones

A canary is a one-line breakage of the kernel (`canaries.txt`, written by the
kernel's owner):

    == <id> (<side>)
    # <what breaks>
    <file>
    -<the exact old line>
    +<the new line>

For each: the old line must be in the file exactly once (else ERROR); the
tree is built (`cargo test <set> --no-run`) and **a failing build is an ERROR,
never a catch**; then each set is run with `--no-fail-fast`, the failing
`binary<TAB>test` lines go to `<id>.fails` (sorted); the file is restored. A
canary no test catches has an empty `.fails`: a hole in the net, reported. The
sets are those of `tools/test-names.py` (every `cargo test` line of ci.yml); `check`
runs only the sets whose test binaries hold names of the committed `.fails`
files. The exit code is 1 when anything is an ERROR or (check) a committed name
no longer fails.

Options: `--file FILE` (the canaries, default: beside this script), `--out DIR`
(where `.fails` live, default: beside this script), `--only ID[,ID]`,
`--sets ID[,ID]` (default: all), `--set-args "ARGS"` (an ad-hoc set instead of
those of ci.yml, repeatable), `--target DIR` (the target folder, default
`<tree>/target`), `-j N`.
"""
import argparse
import importlib.util
import os
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent


def load_test_names():
    spec = importlib.util.spec_from_file_location("test_names", ROOT / "tools" / "test-names.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Canary:
    def __init__(self, ident, side, comment, path, old, new):
        self.id, self.side, self.comment, self.path, self.old, self.new = ident, side, comment, path, old, new


def parse(text):
    """The canaries of a `canaries.txt`. A malformed block raises ValueError."""
    out, block = [], None
    for number, line in enumerate(text.split("\n"), 1):
        head = re.match(r"^== (\S+) \((\w+)\)\s*$", line)
        if head:
            block = {"id": head.group(1), "side": head.group(2), "comment": "", "path": None, "old": None, "new": None, "line": number}
            out.append(block)
            continue
        if block is None or not line.strip() and block["new"] is None:
            continue
        if line.startswith("#"):
            block["comment"] = line[1:].strip()
        elif block["path"] is None:
            block["path"] = line.strip()
        elif line.startswith("-") and block["old"] is None:
            block["old"] = line[1:]
        elif line.startswith("+") and block["new"] is None:
            block["new"] = line[1:]
        elif line.strip():
            raise ValueError(f"line {number}: unexpected `{line}` in canary {block['id']}")
    canaries = []
    for block in out:
        if None in (block["path"], block["old"], block["new"]):
            raise ValueError(f"canary {block['id']} (line {block['line']}) lacks its file, its old line or its new line")
        canaries.append(Canary(block["id"], block["side"], block["comment"], block["path"], block["old"], block["new"]))
    ids = [c.id for c in canaries]
    if len(ids) != len(set(ids)):
        raise ValueError("two canaries have one id")
    return canaries


def apply(tree, canary):
    """Put the new line in the place of the old one. Raises ValueError unless the
    old line is in the file exactly once."""
    path = pathlib.Path(tree) / canary.path
    if not path.is_file():
        raise ValueError(f"{canary.id}: no file {canary.path}")
    lines = path.read_text(encoding="utf-8").split("\n")
    hits = [i for i, line in enumerate(lines) if line == canary.old]
    if len(hits) != 1:
        raise ValueError(f"{canary.id}: the old line is in {canary.path} {len(hits)} times, not exactly once")
    lines[hits[0]] = canary.new
    path.write_text("\n".join(lines), encoding="utf-8")


def git(tree, *args):
    return subprocess.run(["git", *args], cwd=tree, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def restore(tree, canary):
    done = git(tree, "checkout", "--", canary.path)
    if done.returncode != 0:
        raise RuntimeError(f"could not restore {canary.path}: {done.stderr}")


def tree_is_clean(tree):
    done = git(tree, "status", "--porcelain", "--untracked-files=no")
    return done.returncode == 0 and not done.stdout.strip()


FAILED = re.compile(r"^test (?P<name>.+?) \.\.\. FAILED\s*$")
RUNNING = re.compile(r"^\s*Running .*\((?P<path>[^)]+)\)\s*$")
DOCTESTS = re.compile(r"^\s*Doc-tests (?P<crate>\S+)\s*$")
RERUN = re.compile(r"^error: (?:test|doctest) failed, to rerun pass `(?P<args>[^`]*)`")


def failing(output, exe_map, tn):
    """The `binary<TAB>test` lines of the failures in a `cargo test --no-fail-fast`
    output, and the binaries that failed without a test name (a crash)."""
    names, binary, with_names = set(), None, set()
    for line in output.splitlines():
        running = RUNNING.match(line)
        if running:
            binary = tn.binary_of(running.group("path"), exe_map)
            continue
        doc = DOCTESTS.match(line)
        if doc:
            binary = doc.group("crate").replace("_", "-") + "/doc"
            continue
        failed = FAILED.match(line)
        if failed and binary:
            name = failed.group("name")
            if binary.endswith("/doc"):
                name = tn.LINE_NO.sub("", name)
            names.add(f"{binary}\t{name}")
            with_names.add(binary)
    crashed = set()
    for line in output.splitlines():
        rerun = RERUN.match(line)
        if rerun:
            target = rerun.group("args").replace("--test ", "").replace("-p ", "").replace("--lib", "lib").replace("--doc", "doc").split()
            if len(target) >= 2:
                label = f"{target[0]}/{target[1]}"
                if label not in with_names and not any(n.startswith(label + "\t") for n in names):
                    crashed.add(label)
    return names, crashed


def cargo_env(args):
    env = dict(os.environ)
    env["CARGO_INCREMENTAL"] = "0"
    if args.target:
        env["CARGO_TARGET_DIR"] = args.target
    elif "CARGO_TARGET_DIR" not in env or not env["CARGO_TARGET_DIR"].startswith(str(args.tree)):
        env["CARGO_TARGET_DIR"] = str(pathlib.Path(args.tree) / "target")
    return env


def run_set(args, tn, kept, harness):
    """`('error', text)` when the build fails, else `('ok', names, crashed)`."""
    env = cargo_env(args)
    jobs = ["-j", str(args.jobs)] if args.jobs else []
    build = subprocess.run(["cargo", "test", *jobs, *kept, "--no-run", "--message-format=json"], cwd=args.tree, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if build.returncode != 0:
        return ("error", build.stderr[-2000:])
    exe_map = tn.exe_map_of(build.stdout.splitlines())
    tail = ["--", *harness] if harness else []
    test = subprocess.run(["cargo", "test", *jobs, *kept, "--no-fail-fast", *tail], cwd=args.tree, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    names, crashed = failing(test.stdout, exe_map, tn)
    if test.returncode != 0 and not names and not crashed:
        crashed.add("(a test run failed without a test name)")
    return ("ok", names, crashed)


def selected_sets(args, tn):
    if args.set_args:
        return [(f"custom{i}", a.split(), []) for i, a in enumerate(args.set_args)]
    sets = tn.all_sets((pathlib.Path(args.tree) / ".github/workflows/ci.yml").read_text(encoding="utf-8"))
    if args.sets:
        wanted = args.sets.split(",")
        unknown = [w for w in wanted if w not in [s[0] for s in sets]]
        if unknown:
            raise SystemExit(f"unknown set: {unknown}")
        sets = [s for s in sets if s[0] in wanted]
    return sets


def binaries_of_sets(tn, sets, tree):
    """set id -> the binaries its written names hold (tools/ci/test-names-244)."""
    out = {}
    folder = pathlib.Path(tree) / "tools/ci/test-names-244"
    for sid, _, _ in sets:
        path = folder / f"{sid}.txt"
        out[sid] = {line.split("\t")[0] for line in path.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#")} if path.exists() else set()
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["list", "apply-check", "run", "check"])
    parser.add_argument("file_pos", nargs="?")
    parser.add_argument("--tree")
    parser.add_argument("--file", default=str(HERE / "canaries.txt"))
    parser.add_argument("--out", default=str(HERE))
    parser.add_argument("--only")
    parser.add_argument("--sets")
    parser.add_argument("--set-args", action="append")
    parser.add_argument("--target")
    parser.add_argument("-j", "--jobs", type=int)
    args = parser.parse_args()

    canaries = parse(pathlib.Path(args.file_pos or args.file).read_text(encoding="utf-8"))
    if args.command == "list":
        for c in canaries:
            print(f"{c.id}\t{c.side}\t{c.path}\t{c.comment}")
        return 0
    if not args.tree:
        raise SystemExit("--tree DIR (a clean worktree of its own) is needed")
    tree = pathlib.Path(args.tree).resolve()
    args.tree = str(tree)
    if args.only:
        canaries = [c for c in canaries if c.id in args.only.split(",")]
    if not tree_is_clean(tree):
        raise SystemExit(f"{tree} is not a clean tree: the runner works in a worktree of its own")

    if args.command == "apply-check":
        bad = 0
        for c in canaries:
            try:
                apply(tree, c)
                print(f"{c.id}: applies")
            except ValueError as error:
                print(f"ERROR {error}")
                bad += 1
            finally:
                restore(tree, c)
        return 1 if bad else 0

    tn = load_test_names()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    sets_all = selected_sets(args, tn)
    results = []
    for c in canaries:
        committed = out / f"{c.id}.fails"
        sets = sets_all
        if args.command == "check":
            if not committed.exists():
                results.append((c, "ERROR", f"no committed {c.id}.fails"))
                continue
            want = [l for l in committed.read_text(encoding="utf-8").splitlines() if l]
            if not want:
                results.append((c, "HOLE", "committed as not caught; not run"))
                continue
            held = binaries_of_sets(tn, sets_all, tree)
            wanted_bins = {l.split("\t")[0] for l in want}
            sets = [s for s in sets_all if held.get(s[0]) is None or held[s[0]] & wanted_bins or not held[s[0]]]
        try:
            apply(tree, c)
        except ValueError as error:
            results.append((c, "ERROR", str(error)))
            continue
        try:
            names, crashed, error = set(), set(), None
            for sid, kept, harness in sets:
                done = run_set(args, tn, kept, harness)
                if done[0] == "error":
                    error = f"does not compile in set {sid}:\n{done[1]}"
                    break
                names |= done[1]
                crashed |= {f"{b}\t(binary failed)" for b in done[2]}
        finally:
            restore(tree, c)
        (out / f"{c.id}.error").unlink(missing_ok=True)
        if error:
            (out / f"{c.id}.error").write_text(error, encoding="utf-8")
            results.append((c, "ERROR", error.splitlines()[0]))
            continue
        got = sorted(names | crashed)
        if args.command == "run":
            committed.write_text("".join(f"{l}\n" for l in got), encoding="utf-8")
            results.append((c, "CAUGHT" if got else "NOT CAUGHT", f"{len(got)} failing: " + "; ".join(l.replace(chr(9), '::') for l in got[:3])))
        else:
            missing = [l for l in want if l not in got]
            extra = [l for l in got if l not in want]
            if missing:
                results.append((c, "NOT SAME", f"{len(missing)} committed name(s) no longer fail, e.g. {missing[0].replace(chr(9), '::')}"))
            else:
                results.append((c, "SAME", f"{len(want)} names fail again" + (f", {len(extra)} more" if extra else "")))

    width = max([len(c.id) for c, _, _ in results] + [2])
    for c, status, note in results:
        print(f"{c.id:<{width}}  {c.side:<6}  {status:<10}  {note}")
    bad = [s for _, s, _ in results if s in ("ERROR", "NOT SAME")]
    holes = [c.id for c, s, _ in results if s in ("NOT CAUGHT", "HOLE")]
    if holes:
        print(f"HOLES in the net (no test fails): {', '.join(holes)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())

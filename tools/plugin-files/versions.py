#!/usr/bin/env python3
"""Whether each of PaperPaw's own plugins the store offers (plugins.txt)
that changed since the last release has a newer version of its own:
devices update a plugin only to a newer version, so a fix under the
same one would never reach them.

    python3 tools/plugin-files/versions.py [<since>] [--release]

<since> is the release's tag compared with, the newest `v<version>` tag
before HEAD where not given (CI runs it with the history and the tags).
A plugin updated alone (a tag `<id>-v<version>`, store-plugin.yml) is
compared with those releases too: a change none of them had, under a
version no newer than the newest of them, fails, and so does a version
older than one published.
A plugin's files are `plugins/<id>/` and its binary here
(`src/bin/<id>.rs`). A change no reader sees (a comment, a test, a
rename inside) keeps the version when each commit with it says so in a
trailer of its message:

    Plugin-unchanged: <id>, <id>      (or `all`)

The release rebuilds such a plugin anyway, for its firmware, at the
version it has.

The plugins built into the firmware (every other crate of `plugins/` that
is published) are held to the version of the release too, but the other
way: crates.io keeps the sources of a published version, so any change in
`plugins/<id>/` since the release (a comment, a test, a rename included;
`Plugin-unchanged:` does not apply) needs a newer version before the
crates are published (docs/releases.md). That is a warning, so that the
bump can be made once, when the beta's changes are in; with `--release`
(the step before the release's version bump) it fails.

Where the plugin interface was raised since the release
(`native::INTERFACE`), or its draft moved on or was frozen
(`native::DRAFT`, part of the fingerprint like the interface), every
plugin needs a newer version, changed or not: its code built before does
not run on the new firmware, and devices fetch the new build only as an
update. Drafts that come and go between two releases count once, and
it is counted from the build that carries the plugin's newest version
(interface and draft compared with now): a plugin released alone at the
current draft owes nothing, one released at an older draft owes one bump
(beta drafts can reach devices), and the `Plugin-unchanged` trailer excuses
none of it.

The data plugins of data.toml are held to the same: one whose builder or
inputs changed since the release needs a newer version in data.toml (or
the trailer in each commit no reader sees). They have no code, so a
raised interface leaves them as they are.

Built store files (`--bytes`, the release's check):

    python3 tools/plugin-files/versions.py --release --bytes

Source commits cannot say what changes a built file: the SDK library and
the built-in plugin crates are no part of `plugins/<id>/`, and a bump
ripples (a plugin that reads another's version is rebuilt with it). So the
check builds the store files of the release's tag (once; kept in
`$PAPERPAW_STORE_BASELINE`, `~/.cache/paperpaw/store-baseline` where not
set) and of HEAD, with build.sh, and fails for each plugin whose version
did not change but whose file did: a store would serve other code under
the same version. The message names it and its next patch version.
Files built for another firmware (the fingerprint, the plugin interface
or the compiler of the two differ) cannot be compared; the version rules
above hold for those (a raised interface: every plugin has a newer
version). `--built <dir>` takes HEAD's files from a build already made
(`build.sh <device> <dir>`, unsigned), `--baseline <dir>` the release's;
`--device` is the device they are built for (x4, where not given).

A commit that changed a data plugin's inputs without changing what the
build writes, and that is in shared history (its message cannot take the
trailer any more), is listed in unchanged.txt: its hash and the plugins,
and the proof (the build's output compared with the release's) as a
comment.
"""

import argparse
import hashlib
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
VERSION = re.compile(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?")
TRAILER = re.compile(r"^Plugin-unchanged:\s*(.+)$", re.IGNORECASE | re.MULTILINE)
INTERFACE = re.compile(r"^pub const INTERFACE: u16 = (\d+);$", re.MULTILINE)
DRAFT = re.compile(r"^pub const DRAFT: u16 = (\d+);$", re.MULTILINE)
NATIVE = "crates/paperpaw-sdk/src/native.rs"
UNCHANGED = "tools/plugin-files/unchanged.txt"


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True,
                          text=True, encoding="utf-8").stdout


def plugins():
    """The ids of plugins.txt."""
    out = []
    for line in (HERE / "plugins.txt").read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].split()
        if line:
            out.append(line[0])
    return out


def semver(version):
    """A key that orders versions as SemVer does (build metadata left out)."""
    version = version.split("+", 1)[0]
    core, _, pre = version.partition("-")
    numbers = tuple(int(n) for n in core.split("."))
    if not pre:
        return numbers, (1,)
    ids = tuple((0, int(i), "") if i.isdigit() else (1, 0, i) for i in pre.split("."))
    return numbers, (0, ids)


def version_in(manifest, workspace):
    """The [package] version of `manifest` (Cargo.toml's text), the
    workspace's (its text) where it takes that one."""
    version = tomllib.loads(manifest)["package"].get("version")
    if isinstance(version, dict):
        return tomllib.loads(workspace)["workspace"]["package"]["version"]
    return version


def unchanged(message, plugin):
    """Whether the commit `message` says it changed nothing a reader of
    `plugin` sees."""
    for names in TRAILER.findall(message):
        ids = {name.strip() for name in names.split(",")}
        if plugin in ids or "all" in ids:
            return True
    return False


def exempt_commits(text):
    """Of unchanged.txt's `text` (lines `<hash> <id>, <id>  # proof`), the
    (hash, plugin) pairs; a hash may be an abbreviation."""
    out = set()
    for line in text.splitlines():
        words = line.split("#", 1)[0].replace(",", " ").split()
        out.update((words[0], plugin) for plugin in words[1:])
    return out


def exempted(commit, plugin, pairs):
    """Whether `pairs` (exempt_commits) say the `commit` (its full hash)
    changed nothing a reader of `plugin` sees."""
    return any(plugin == who and len(h) >= 7 and commit.startswith(h) for h, who in pairs)


def interface_at(text):
    """The plugin interface `native.rs` (its text) names."""
    found = INTERFACE.search(text)
    return int(found.group(1)) if found else None


def draft_at(text):
    """The draft of the plugin interface `native.rs` (its text) names; None
    where it names none (frozen, or from before drafts)."""
    found = DRAFT.search(text)
    return int(found.group(1)) if found else None


def interface_raised(since):
    """The plugin interface now (with its draft, if one), where it or its
    draft changed since `since`; None where neither did (or `native.rs` is
    not there to say)."""
    try:
        now_text = (ROOT / NATIVE).read_text(encoding="utf-8")
        then_text = git("show", f"{since}:{NATIVE}")
    except (OSError, subprocess.CalledProcessError):
        return None
    now, then = interface_at(now_text), interface_at(then_text)
    if None in (now, then):
        return None
    draft, then_draft = draft_at(now_text), draft_at(then_text)
    if (now, draft) == (then, then_draft):
        return None
    return f"{now}" if draft is None else f"{now} (draft {draft})"


def raised_for(versions, before, since):
    """Whether the plugin interface was raised since the build that carries
    `before` (see `interface_raised`), the version a plugin has to better:
    the release `since`'s, or the plugin's own newer one (`versions`, tag ->
    version). The interface and draft of that build are compared with now:
    a plugin released alone at the current draft owes nothing, one released
    at an older draft owes a bump (beta drafts can reach devices), and the
    drafts that come and go between two releases count once."""
    tag = next((t for t, v in versions.items() if semver(v) == semver(before)), since)
    return interface_raised(tag)


def problem(plugin, before, now, changes, raised=None):
    """What is wrong with `plugin` at version `now`, `before` at the
    release, `changes` the messages of the commits that changed it since;
    `raised` the plugin interface now where it was raised since; None
    where nothing is."""
    if semver(now) < semver(before):
        return f"its version {now} is older than the release's {before}"
    if raised is not None and semver(now) == semver(before):
        return (
            f"the plugin interface was raised to {raised} since the release, its version "
            f"{now} did not: raise it in plugins/{plugin}/Cargo.toml (and the Cargo.lock files "
            f"that list it), so that devices fetch its new build"
        )
    seen = [m for m in changes if not unchanged(m, plugin)]
    if seen and semver(now) == semver(before):
        first = seen[0].splitlines()[0] if seen[0] else ""
        return (
            f"changed since the release ({len(seen)} commits, e.g. \"{first}\"), its version "
            f"{now} did not: raise it in plugins/{plugin}/Cargo.toml (and the Cargo.lock files "
            f"that list it), or say `Plugin-unchanged: {plugin}` in each commit no reader sees"
        )
    return None


def own_tags(plugin, tags):
    """Of `tags`, the plugin's own releases (`<id>-v<version>`)."""
    prefix = f"{plugin}-v"
    return [t for t in tags if t.startswith(prefix) and VERSION.fullmatch(t[len(prefix):])]


def version_at(tag, manifest):
    """The plugin's version at `tag`; None where it was not there yet."""
    try:
        return version_in(git("show", f"{tag}:{manifest}"), git("show", f"{tag}:Cargo.toml"))
    except subprocess.CalledProcessError:
        return None


def is_ancestor(tag):
    """Whether `tag` is in HEAD's history."""
    done = subprocess.run(["git", "merge-base", "--is-ancestor", tag, "HEAD"], cwd=ROOT,
                          capture_output=True)
    return done.returncode == 0


def same_files(tag, paths):
    """Whether `paths` are at `tag` as they are at HEAD."""
    done = subprocess.run(["git", "diff", "--quiet", tag, "HEAD", "--", *paths], cwd=ROOT)
    return done.returncode == 0


def builtin_plugins(listed):
    """The ids of the crates in plugins/ that are built into the firmware:
    published, and not among `listed` (plugins.txt)."""
    out = []
    for manifest in sorted((ROOT / "plugins").glob("*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text(encoding="utf-8")).get("package", {})
        if manifest.parent.name not in listed and package.get("publish", True) is not False:
            out.append(manifest.parent.name)
    return out


def builtin_problem(plugin, before, now, changes):
    """What is wrong with the built-in `plugin` at version `now`, `before`
    at the release (published then), `changes` the messages of the commits
    that changed its `plugins/<id>/` since; None where nothing is. A trailer
    cannot excuse a change: the published sources would differ."""
    if semver(now) < semver(before):
        return f"its version {now} is older than the release's {before}"
    if changes and semver(now) == semver(before):
        first = changes[0].splitlines()[0] if changes[0] else ""
        return (
            f"built into the firmware, changed since the release ({len(changes)} commits, "
            f"e.g. \"{first}\"), its version {now} did not: crates.io keeps the sources of the "
            f"published {before}, so raise it in plugins/{plugin}/Cargo.toml (and the Cargo.lock "
            f"files that list it) before the crates are published"
        )
    return None


DATA = "tools/plugin-files/data.toml"


def data_version_at(tag, plugin):
    """Data plugin `plugin`'s version in data.toml at `tag`; None where it
    was not there yet."""
    try:
        listed = tomllib.loads(git("show", f"{tag}:{DATA}"))
    except subprocess.CalledProcessError:
        return None
    return listed.get(plugin, {}).get("version")


def data_problems(since, tags=()):
    """The data plugins of data.toml whose builder or inputs changed since
    `since` or their own release (`<id>-v<version>` of `tags`) without a
    newer version: (id, what is wrong). Commits listed in unchanged.txt
    count as no change."""
    if not (ROOT / DATA).exists():
        return []
    now = tomllib.loads((ROOT / DATA).read_text(encoding="utf-8"))
    listed = ROOT / UNCHANGED
    pairs = exempt_commits(listed.read_text(encoding="utf-8") if listed.exists() else "")
    out = []
    for plugin, fields in now.items():
        versions = {tag: data_version_at(tag, plugin) for tag in [since, *own_tags(plugin, tags)]}
        versions = {tag: v for tag, v in versions.items() if v is not None}
        if not versions:
            continue  # new since the release
        before = max(versions.values(), key=semver)
        paths = [fields["build"], *fields.get("inputs", [])]
        had = [f"^{tag}" for tag in versions if is_ancestor(tag)]
        log = git("log", "--no-merges", "--format=%H %B%x00", "HEAD", *had, "--", *paths)
        changes = []
        for entry in log.split("\0"):
            commit, _, message = entry.strip().partition(" ")
            if message.strip() and not exempted(commit, plugin, pairs):
                changes.append(message.strip())
        found = problem(plugin, before, fields["version"], changes)
        if found:
            out.append((plugin, found.replace(
                f"plugins/{plugin}/Cargo.toml (and the Cargo.lock files that list it)", DATA)))
    return out


def builtin_problems(since, listed, workspace_now):
    """The built-in plugins whose sources changed since `since` without a
    newer version: (id, what is wrong)."""
    out = []
    for plugin in builtin_plugins(listed):
        manifest = f"plugins/{plugin}/Cargo.toml"
        before = version_at(since, manifest)
        if before is None:
            continue  # new since the release
        now = version_in((ROOT / manifest).read_text(encoding="utf-8"), workspace_now)
        if same_files(since, [f"plugins/{plugin}"]):
            continue  # commits that undid each other leave the published sources
        log = git("log", "--no-merges", "--format=%B%x00", "HEAD", f"^{since}", "--",
                  f"plugins/{plugin}") if is_ancestor(since) else ""
        changes = [m.strip() for m in log.split("\0") if m.strip()]
        found = builtin_problem(plugin, before, now, changes)
        if found:
            out.append((plugin, found))
    return out


FILE = re.compile(r"(?P<id>.+)-(?P<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)-(?P<device>x4(?:-[a-z]+)?)\.ppn")
IDENTITY = ("fingerprint", "interface", "compiler")


def store_files(folder, device):
    """The store files of `device` in `folder` (build.sh's
    `<id>-<version>-<device>.ppn`): id -> (version, path)."""
    out = {}
    for path in sorted(pathlib.Path(folder).glob("*.ppn")):
        found = FILE.fullmatch(path.name)
        if found and found["device"] == device:
            out[found["id"]] = (found["version"], path)
    return out


def next_patch(version):
    """The version after `version` by one patch (1.0.4 -> 1.0.5); a
    pre-release is left for the author (None)."""
    core = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    return f"{core[1]}.{core[2]}.{int(core[3]) + 1}" if core else None


def describe(path, sign):
    """What `paperpaw-sign plugin` says of the file (the firmware it is for):
    the lines of IDENTITY."""
    done = subprocess.run([str(sign), "plugin", str(path)], capture_output=True, text=True,
                          encoding="utf-8")
    if done.returncode != 0:
        raise RuntimeError(f"{sign} plugin {path}: {done.stderr.strip() or done.stdout.strip()}")
    lines = dict(line.split(":", 1) for line in done.stdout.splitlines() if ":" in line)
    return tuple(lines.get(key, "").strip() for key in IDENTITY)


def digest(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def built_problems(then, now, identity=lambda path: ()):
    """The store files of `now` (store_files) that other bytes than those
    of `then`, the release's, under the same version: [(id, version)];
    and the ids that cannot be compared (built for another firmware, as
    `identity(path)` tells), and those new since the release."""
    wrong, apart, new = [], [], []
    for plugin, (version, path) in sorted(now.items()):
        if plugin not in then:
            new.append(plugin)
            continue
        before, kept = then[plugin]
        if semver(version) != semver(before):
            continue  # a newer version carries other bytes; an older one is not this check's
        if identity(kept) != identity(path):
            apart.append(plugin)
        elif digest(kept) != digest(path):
            wrong.append((plugin, version))
    return wrong, apart, new


def build_store(root, out, device, target=None):
    """build.sh's files for `device` of the tree at `root` into `out`
    (unsigned); raises with the end of its output where it fails."""
    # Never the trial-build override of build.sh: these files are hashed
    # against the release's, so they must be built with the paths remapped.
    env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "PAPERPAW_ALLOW_UNREMAPPED")}
    if target:
        env["CARGO_TARGET_DIR"] = str(target)
    done = subprocess.run(["bash", "tools/plugin-files7/build.sh", device, str(out)], cwd=root,
                          env=env, capture_output=True, text=True, encoding="utf-8")
    if done.returncode != 0:
        tail = "\n".join((done.stdout + done.stderr).strip().splitlines()[-25:])
        raise RuntimeError(f"build.sh {device} in {root} failed:\n{tail}")


def baseline_cache():
    return pathlib.Path(os.environ.get("PAPERPAW_STORE_BASELINE")
                        or pathlib.Path.home() / ".cache" / "paperpaw" / "store-baseline")


def baseline_files(since, device, build=build_store):
    """The folder with the store files of `device` as the tree at `since`
    builds them: kept by the commit, built (in a worktree of its own, its
    target folder removed after) where it is not there yet."""
    commit = git("rev-parse", f"{since}^{{commit}}").strip()
    folder = baseline_cache() / f"{commit}-{device}"
    if (folder / ".done").exists():
        return folder
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    tree = pathlib.Path(tempfile.mkdtemp(prefix="store-baseline-tree-"))
    target = baseline_cache() / f"target-{commit}"
    try:
        git("worktree", "add", "--detach", "-f", str(tree / "src"), commit)
        build(tree / "src", folder, device, target)
    except BaseException:
        shutil.rmtree(folder, ignore_errors=True)
        raise
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(tree / "src")], cwd=ROOT,
                       capture_output=True)
        shutil.rmtree(tree, ignore_errors=True)
        shutil.rmtree(target, ignore_errors=True)
    (folder / ".done").write_text(commit + "\n", encoding="utf-8")
    return folder


def sign_tool():
    """paperpaw-sign, as the build left it (the first of the target folders)."""
    for base in (os.environ.get("CARGO_TARGET_DIR"), ROOT / "target"):
        tool = pathlib.Path(base or "/nonexistent") / "release" / "paperpaw-sign"
        if tool.exists():
            return tool
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-sign"], cwd=ROOT, check=True)
    return ROOT / "target" / "release" / "paperpaw-sign"


def bytes_check(since, device, built=None, baseline=None, sign=None):
    """Compares HEAD's built store files with the release's (see the head
    of this file); prints what is wrong, and says whether nothing is."""
    with tempfile.TemporaryDirectory(prefix="store-built-") as scratch:
        if built is None:
            built = pathlib.Path(scratch) / "now"
            build_store(ROOT, built, device)
        if baseline is None:
            baseline = baseline_files(since, device)
        then, now = store_files(baseline, device), store_files(built, device)
        if not then:
            print(f"::error::no store files of {device} in the release's build ({baseline})")
            return False
        tool = pathlib.Path(sign) if sign else sign_tool()
        wrong, apart, new = built_problems(then, now, lambda path: describe(path, tool))
    for plugin, version in wrong:
        suggest = next_patch(version)
        advice = f"raise it to {suggest}" if suggest else "raise it"
        print(f"::error file=plugins/{plugin}/Cargo.toml::{plugin}: its store file for {device} "
              f"differs from {since}'s build (same code, other bytes: the SDK or a crate it "
              f"depends on changed, or another plugin's bump rippled), its version {version} did "
              f"not: {advice} in plugins/{plugin}/Cargo.toml (and the Cargo.lock files that list "
              f"it), then check again: a bump can ripple on")
    if apart:
        print(f"{len(apart)} store files were built for another firmware than {since}'s "
              f"(fingerprint, interface or compiler), so their bytes cannot be compared: "
              f"{', '.join(apart)}")
    if new:
        print(f"{len(new)} plugins are new since {since}: {', '.join(new)}")
    if not wrong:
        print(f"every store file of {device} compared with {since}'s build ({len(now) - len(new) - len(apart)} "
              f"of {len(now)}) is the same or has a newer version")
    return not wrong


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("since", nargs="?")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--bytes", action="store_true")
    parser.add_argument("--built")
    parser.add_argument("--baseline")
    parser.add_argument("--device", default="x4")
    parser.add_argument("--sign")
    given = parser.parse_args(sys.argv[1:])
    args = [given.since] if given.since else []
    release = given.release
    since = args[0] if args else git(
        "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", "HEAD").strip()
    tags = git("tag", "--list").split()
    workspace_now = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    failed = False
    for plugin in plugins():
        manifest = f"plugins/{plugin}/Cargo.toml"
        # The release's build and the plugin's own releases: the newest
        # version of them, and the commits none of those in HEAD's
        # history had.
        versions = {tag: version_at(tag, manifest) for tag in [since, *own_tags(plugin, tags)]}
        versions = {tag: v for tag, v in versions.items() if v is not None}
        if not versions:
            continue  # new since the release
        before = max(versions.values(), key=semver)
        now = version_in((ROOT / manifest).read_text(encoding="utf-8"), workspace_now)
        paths = [f"plugins/{plugin}", f"tools/plugin-files7/src/bin/{plugin}.rs"]
        # A plugin released alone from a branch beside this one (its tag
        # not in HEAD's history) with the files HEAD has: nothing new.
        if now == before and any(
                v == now and not is_ancestor(tag) and same_files(tag, paths)
                for tag, v in versions.items()):
            continue
        had = [f"^{tag}" for tag in versions if is_ancestor(tag)]
        log = git("log", "--no-merges", "--format=%B%x00", "HEAD", *had, "--", *paths)
        changes = [m.strip() for m in log.split("\0") if m.strip()]
        # The raise is counted from the build that carries `before`.
        found = problem(plugin, before, now, changes, raised_for(versions, before, since))
        if found:
            print(f"::error file={manifest}::{plugin}: {found}")
            failed = True
    for plugin, found in data_problems(since, tags):
        print(f"::error file=tools/plugin-files/data.toml::{plugin}: {found}")
        failed = True
    waiting = builtin_problems(since, plugins(), workspace_now)
    for plugin, found in waiting:
        level = "error" if release else "warning"
        print(f"::{level} file=plugins/{plugin}/Cargo.toml::{plugin}: {found}")
        failed = failed or release
    if given.bytes or given.built:
        failed = not bytes_check(since, given.device, given.built, given.baseline,
                                 given.sign) or failed
    if failed:
        sys.exit(1)
    print(f"every plugin that changed since {since} or its own release has a newer version")
    if waiting:
        print(f"{len(waiting)} built-in plugins still wait for their bump "
              "(a warning; versions.py --release fails on them)")


if __name__ == "__main__":
    main()

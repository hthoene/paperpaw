#!/usr/bin/env python3
"""Whether each of PaperPaw's own plugins the store offers (plugins.txt)
that changed since the last release has a newer version of its own:
devices update a plugin only to a newer version, so a fix under the
same one would never reach them.

    python3 tools/plugin-files/versions.py [<since>]

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

Where the plugin interface was raised since the release
(`native::INTERFACE`), every plugin needs a newer version, changed or
not: its code built before does not run on the new firmware, and devices
fetch the new build only as an update.

The data plugins of data.toml are held to the same: one whose builder or
inputs changed since the release needs a newer version in data.toml (or
the trailer in each commit no reader sees). They have no code, so a
raised interface leaves them as they are.
"""

import pathlib
import re
import subprocess
import sys
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
VERSION = re.compile(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?")
TRAILER = re.compile(r"^Plugin-unchanged:\s*(.+)$", re.IGNORECASE | re.MULTILINE)
INTERFACE = re.compile(r"pub const INTERFACE: u16 = (\d+);")
NATIVE = "crates/paperpaw-sdk/src/native.rs"


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


def interface_at(text):
    """The plugin interface `native.rs` (its text) names."""
    found = INTERFACE.search(text)
    return int(found.group(1)) if found else None


def interface_raised(since):
    """The plugin interface now, where it was raised since `since`; None
    where it was not (or `native.rs` is not there to say)."""
    try:
        now = interface_at((ROOT / NATIVE).read_text(encoding="utf-8"))
        then = interface_at(git("show", f"{since}:{NATIVE}"))
    except (OSError, subprocess.CalledProcessError):
        return None
    return now if None not in (now, then) and now != then else None


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
    newer version: (id, what is wrong)."""
    if not (ROOT / DATA).exists():
        return []
    now = tomllib.loads((ROOT / DATA).read_text(encoding="utf-8"))
    out = []
    for plugin, fields in now.items():
        versions = {tag: data_version_at(tag, plugin) for tag in [since, *own_tags(plugin, tags)]}
        versions = {tag: v for tag, v in versions.items() if v is not None}
        if not versions:
            continue  # new since the release
        before = max(versions.values(), key=semver)
        paths = [fields["build"], *fields.get("inputs", [])]
        had = [f"^{tag}" for tag in versions if is_ancestor(tag)]
        log = git("log", "--no-merges", "--format=%B%x00", "HEAD", *had, "--", *paths)
        changes = [m.strip() for m in log.split("\0") if m.strip()]
        found = problem(plugin, before, fields["version"], changes)
        if found:
            out.append((plugin, found.replace(
                f"plugins/{plugin}/Cargo.toml (and the Cargo.lock files that list it)", DATA)))
    return out


def main():
    since = sys.argv[1] if len(sys.argv) > 1 else git(
        "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", "HEAD").strip()
    tags = git("tag", "--list").split()
    workspace_now = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    raised = interface_raised(since)
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
        paths = [f"plugins/{plugin}", f"tools/plugin-files/src/bin/{plugin}.rs"]
        # A plugin released alone from a branch beside this one (its tag
        # not in HEAD's history) with the files HEAD has: nothing new.
        if now == before and any(
                v == now and not is_ancestor(tag) and same_files(tag, paths)
                for tag, v in versions.items()):
            continue
        had = [f"^{tag}" for tag in versions if is_ancestor(tag)]
        log = git("log", "--no-merges", "--format=%B%x00", "HEAD", *had, "--", *paths)
        changes = [m.strip() for m in log.split("\0") if m.strip()]
        found = problem(plugin, before, now, changes, raised)
        if found:
            print(f"::error file={manifest}::{plugin}: {found}")
            failed = True
    for plugin, found in data_problems(since, tags):
        print(f"::error file=tools/plugin-files/data.toml::{plugin}: {found}")
        failed = True
    if failed:
        sys.exit(1)
    print(f"every plugin that changed since {since} or its own release has a newer version")


if __name__ == "__main__":
    main()

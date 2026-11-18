#!/usr/bin/env python3
"""What store-plugin.yml checks before it builds one of PaperPaw's own code
plugins between releases:

    store_update.py <plugin> <base-tag> --source <dir> [--accept <path>,<path>...]

Run from a trusted checkout of the workflow's own commit
(`github.workflow_sha`), never from the source it checks: the source is
the branch to publish, and its own copy of this script, like its
fingerprint check and its signing, would be the branch's to change.
<dir> is a checkout of the source (`HEAD`), with the base's tag fetched.
It prints the plugin's version (its own, its manifest's) on
standard output and fails, saying why on standard error, when

- the source is not a descendant of the base's tag (a branch made from it);
- the plugin is in the base (an update): its version is not above the
  base's. A plugin that is not in the base (a new one, first published
  between releases) needs no older version to compare with;
- the source differs from the base's anywhere but in the plugin, the
  plugin files' project (`tools/plugin-files/`: its manifest, lock file,
  list and binaries, no scripts), the CHANGELOG and docs: anything else
  (the kernel, the SDK, the shells, other plugins, `crates/`, any script
  or workflow) is firmware, or what a firmware shares with a plugin (the
  tables of shared functions), or what runs with the key, and the
  published firmware is the base's, not this source's. A path can be
  accepted by name (`--accept`, a path or a folder, each one reported):
  a decision of whoever runs the workflow. The root `Cargo.toml` may
  gain the plugin's member and its entry of workspace.dependencies and
  nothing else ([patch], [profile.*] or another dependency would build
  the plugin against another SDK); `Cargo.lock` may gain the plugin's own
  package (and a dependency on it) and no existing package changes, and no
  new paperpaw-* package or one from git or a registry. Git's
  rename detection is off, so a moved file shows both its paths;
- the plugin depends by path on anything but another crate of the
  workspace (a copy of the SDK in its own folder).

With `--main` (the source is the repository's main, #171): the same, except
that a change elsewhere since the base is no refusal but a note (main moves on
with the next firmware; what a plugin may use of it is the firmware's table,
whose fingerprint check_files.sh holds the files to), and Cargo.toml and
Cargo.lock may differ too. The plugin must still come from a descendant of the
base's tag, have a newer version, and depend on no copy of the SDK.

The plugin's fingerprint is not checked here: check_files.sh does, with
the build's files, before they are signed.
"""

import argparse
import pathlib
import copy
import posixpath
import subprocess
import sys
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import versions  # noqa: E402

# Where a change cannot reach the firmware of the base, nor the key. Of
# tools/plugin-files7 only the project that builds the plugin files: the
# scripts (build.sh, sign.sh, the Python tools) and the linker script stay
# the trusted checkout's, and a change to one is for `accept` to name.
OWN_FOLDERS = ("tools/plugin-files7/src/", "docs/", "changelog.d/")
OWN_FILES = ("CHANGELOG.md", "README.md", "tools/plugin-files7/Cargo.toml", "tools/plugin-files7/Cargo.lock",
             "tools/plugin-files/plugins.txt")
# The workspace's list of members and versions: changed only as far as a
# new plugin needs (manifest_problems, lock_problems), or by name.
WORKSPACE_FILES = ("Cargo.toml", "Cargo.lock")


def accepted_paths(accepted):
    """The names in `accept`, stripped, without empty ones and trailing slashes."""
    return [a.strip().rstrip("/") for a in accepted if a.strip()]


def classify(paths, plugin, accepted=()):
    """The changed `paths` in four lists: (own, workspace, accepted, refused).

    `workspace` are the root Cargo.toml and Cargo.lock, for the checks of
    what changed in them (unless accepted by name)."""
    own, workspace, named, refused = [], [], [], []
    accepted = accepted_paths(accepted)
    for path in paths:
        if any(path == a or path.startswith(a + "/") for a in accepted):
            named.append(path)
        elif path.startswith(f"plugins/{plugin}/") or path.startswith(OWN_FOLDERS) or path in OWN_FILES:
            own.append(path)
        elif path in WORKSPACE_FILES:
            workspace.append(path)
        else:
            refused.append(path)
    return own, workspace, named, refused


def manifest_problems(plugin, then, now):
    """What the workspace manifest gained beyond the plugin: its member and its entry of workspace.dependencies.

    `then` and `now` are the base's and the source's Cargo.toml, as text."""
    try:
        before, after = tomllib.loads(then), tomllib.loads(now)
    except tomllib.TOMLDecodeError as error:
        return [f"Cargo.toml: {error}"]
    rest = copy.deepcopy(after)
    workspace = rest.get("workspace", {})
    members = workspace.get("members", [])
    was = before.get("workspace", {}).get("members", [])
    added = [m for m in members if m not in was]
    if any(m != f"plugins/{plugin}" for m in added):
        return [f"Cargo.toml: workspace.members gains {', '.join(m for m in added if m != f'plugins/{plugin}')}: "
                f"only plugins/{plugin} may be added"]
    workspace["members"] = [m for m in members if m in was]
    dependencies = workspace.get("dependencies", {})
    old = before.get("workspace", {}).get("dependencies", {})
    for name in [n for n in dependencies if n not in old]:
        spec = dependencies[name]
        path = spec.get("path", "") if isinstance(spec, dict) else ""
        if path == f"plugins/{plugin}":
            del dependencies[name]
    if rest != before:
        return ["Cargo.toml: changed beyond a new plugin's member and workspace.dependencies entry "
                "([patch], [profile], another dependency, a version ...)"]
    return []


def lock_problems(package, then, now):
    """What Cargo.lock gained beyond the plugin's own package: no existing one changes its version, source or checksum.

    `package` is the plugin's crate name. A new package may be the plugin alone: one named paperpaw-* is
    another crate that could stand for the SDK, one with a source (git, registry) is new code from outside.
    An existing package may list a new package among its dependencies (the app, with a new plugin)."""
    try:
        before, after = tomllib.loads(then), tomllib.loads(now)
    except tomllib.TOMLDecodeError as error:
        return [f"Cargo.lock: {error}"]
    old = {(p["name"], p["version"], p.get("source")): p for p in before.get("package", [])}
    new = {(p["name"], p["version"], p.get("source")): p for p in after.get("package", [])}
    problems = [f"{name} {version}: gone, or its version or source changed" for name, version, source in old
                if (name, version, source) not in new]
    added = {key[0] for key in new if key not in old}
    for name, version, source in sorted(k for k in new if k not in old):
        if name != package and name.startswith("paperpaw-"):
            problems.append(f"{name} {version}: a new paperpaw-* package that is not the plugin")
        if source:
            problems.append(f"{name} {version}: a new package from {source.split('+')[0]}")
    for key, package in old.items():
        if key not in new:
            continue
        if package.get("checksum") != new[key].get("checksum"):
            problems.append(f"{key[0]} {key[1]}: checksum changed")
        gained = set(new[key].get("dependencies", [])) - set(package.get("dependencies", []))
        # "name" or "name version" or "name version (source)".
        extra = sorted(d for d in gained if d.split(" ")[0] not in added)
        lost = set(package.get("dependencies", [])) - set(new[key].get("dependencies", []))
        if extra or lost:
            problems.append(f"{key[0]} {key[1]}: dependencies changed ({', '.join(extra + sorted(lost))})")
    return [f"Cargo.lock: {p}" for p in problems]


def plugin_problems(plugin, manifest):
    """The plugin's own dependencies by path: none inside its folder or outside crates/ and plugins/.

    A copy of paperpaw-sdk in the plugin's folder would be built instead of the workspace's."""
    try:
        data = tomllib.loads(manifest)
    except tomllib.TOMLDecodeError as error:
        return [f"plugins/{plugin}/Cargo.toml: {error}"]
    tables = [data.get(k, {}) for k in ("dependencies", "dev-dependencies", "build-dependencies")]
    for target in data.get("target", {}).values():
        tables += [target.get(k, {}) for k in ("dependencies", "dev-dependencies", "build-dependencies")]
    problems = []
    own = f"plugins/{plugin}"
    for table in tables:
        for name, spec in table.items():
            if not isinstance(spec, dict) or "path" not in spec:
                continue
            path = posixpath.normpath(posixpath.join(own, spec["path"]))
            if path == own or path.startswith(own + "/") or not path.startswith(("crates/", "plugins/")):
                problems.append(f"plugins/{plugin}/Cargo.toml: {name} depends on the path {spec['path']}, "
                                "which is not another crate of the workspace")
    return problems


def run(source, *args):
    return subprocess.run(args, cwd=source, capture_output=True, text=True, encoding="utf-8")


def base_manifest(source, plugin, base):
    """The plugin's manifest in the base, or None where the base has no such plugin."""
    result = run(source, "git", "show", f"{base}:plugins/{plugin}/Cargo.toml")
    return result.stdout if result.returncode == 0 else None


def check(source, plugin, base, accepted=(), main=False):
    """The plugin's version; a list of problems, and one of notes."""
    source = pathlib.Path(source)
    problems, notes = [], []
    manifest = source / "plugins" / plugin / "Cargo.toml"
    if not manifest.is_file():
        return None, [f"plugins/{plugin}/Cargo.toml: there is no such plugin"], notes
    now = versions.version_in(manifest.read_text(encoding="utf-8"),
                              (source / "Cargo.toml").read_text(encoding="utf-8"))
    if run(source, "git", "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}").returncode != 0:
        return now, [f"{base} is not a tag or commit here: fetch the base's tag"], notes
    if run(source, "git", "merge-base", "--is-ancestor", base, "HEAD").returncode != 0:
        problems.append(f"the source is not made from {base}: make a branch from the base's tag with the plugin")
    then_manifest = base_manifest(source, plugin, base)
    if then_manifest is None:
        notes.append(f"{plugin} is not in {base}: a new plugin, first published at {now}")
    else:
        then = versions.version_in(then_manifest, run(source, "git", "show", f"{base}:Cargo.toml").stdout)
        if versions.semver(now) <= versions.semver(then):
            problems.append(f"{plugin} is at {now}, the base has {then}: raise it in plugins/{plugin}/Cargo.toml")
    problems += plugin_problems(plugin, manifest.read_text(encoding="utf-8"))
    try:
        package = tomllib.loads(manifest.read_text(encoding="utf-8")).get("package", {}).get("name", "")
    except tomllib.TOMLDecodeError:
        package = ""
    # Renames are paths of both sides (a moved SDK file would show only where it went).
    changed = run(source, "git", "diff", "--name-only", "--no-renames", "-z", base, "HEAD").stdout.split("\0")
    own, workspace, named, refused = classify([c for c in changed if c], plugin, accepted)
    for entry in accepted_paths(accepted):
        files = [c for c in named if c == entry or c.startswith(entry + "/")]
        notes.append(f"accepted by name: {entry} ({', '.join(files) if files else 'nothing changed there'})")
    checks = (("Cargo.toml", lambda then, now: manifest_problems(plugin, then, now)),
              ("Cargo.lock", lambda then, now: lock_problems(package, then, now)))
    for path, check_file in checks:
        if path in workspace and not main:
            then_text = run(source, "git", "show", f"{base}:{path}").stdout
            now_text = run(source, "git", "show", f"HEAD:{path}").stdout
            problems += [f"{p} (the workspace's {path} may gain a new plugin only; name {path} in `accept` "
                         "if it must change otherwise)" for p in check_file(then_text, now_text)]
    if refused and main:
        notes.append(f"main differs from {base} outside the plugin ({len(refused)} paths, e.g. "
                     + ", ".join(refused[:5]) + "): the base's fingerprint decides whether the file fits its firmware")
    elif refused:
        problems.append(f"changed outside the plugin since {base}, so not the firmware's source: "
                        + ", ".join(refused)
                        + " (a change there may differ from the published firmware; name a path in `accept` "
                        "only when it cannot)")
    return now, problems, notes


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("plugin")
    parser.add_argument("base")
    parser.add_argument("--source", required=True, help="the checkout of the source to publish")
    parser.add_argument("--accept", default="", help="paths or folders outside the plugin that may differ")
    parser.add_argument("--main", action="store_true", help="the source is main: other changes are notes")
    args = parser.parse_args(argv)
    version, problems, notes = check(args.source, args.plugin, args.base, args.accept.split(","), args.main)
    for note in notes:
        print(f"::notice::{note}", file=sys.stderr)
    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    if problems:
        return 1
    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

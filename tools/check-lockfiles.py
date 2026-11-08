#!/usr/bin/env python3
"""Fail where a Cargo.lock is not what its manifests say.

The repository has several lockfiles (the root workspace, shells/xteink,
tools/plugin-files, fuzz, the samples, ...), each its own workspace. A
version bump or a removed dependency changes the lock of every one that
lists the crate, and `cargo metadata --locked` run at the repository root
looks at the root's lock only. Two things go wrong that way:

  * a lock that is stale for its own manifest (a plugin at a version it no
    longer has): found by `cargo metadata --locked --manifest-path ...`,
    which this runs for every lockfile in git;
  * a lock that still lists a package no workspace member reaches any more
    (a crate removed from the manifests): cargo does not complain about
    these even with --locked, so the lock is read here and the packages
    are walked from the workspace members.

    python3 tools/check-lockfiles.py            # every Cargo.lock in git
    python3 tools/check-lockfiles.py shells/xteink/Cargo.lock
    python3 tools/check-lockfiles.py --offline  # only from cargo's own cache

Run it after changing a version or a dependency, and before pushing.
"""
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def reference(entry, by_name):
    """The (name, version) a lock `dependencies` entry points at.

    An entry is "name", "name version" or "name version (source)"; the
    version is only there where the lock holds more than one version of
    the name."""
    parts = entry.split(" ")
    name = parts[0]
    if len(parts) > 1:
        return name, parts[1]
    versions = by_name.get(name, [])
    return (name, versions[0]) if len(versions) == 1 else (name, None)


def unreachable(lock, members):
    """Packages of the lock (a parsed Cargo.lock) that no member reaches.

    `members` is a set of (name, version) of the workspace members."""
    packages = {}
    by_name = {}
    for package in lock.get("package", []):
        key = (package["name"], package["version"])
        packages[key] = package
        by_name.setdefault(package["name"], []).append(package["version"])
    seen = set()
    todo = [key for key in members if key in packages]
    while todo:
        key = todo.pop()
        if key in seen:
            continue
        seen.add(key)
        for entry in packages[key].get("dependencies", []):
            target = reference(entry, by_name)
            if target in packages:
                todo.append(target)
    return sorted(set(packages) - seen)


def lockfiles(args):
    if args:
        return [Path(a).resolve() for a in args]
    listed = subprocess.run(
        ["git", "ls-files", "-z", "*Cargo.lock"], cwd=ROOT, check=True,
        stdout=subprocess.PIPE, text=True,
    ).stdout
    return [ROOT / name for name in listed.split("\0") if name]


def toolchain_env():
    """One toolchain for every manifest: the shells' rust-toolchain.toml
    files would otherwise make rustup install theirs just to read a lock."""
    env = dict(os.environ)
    if "RUSTUP_TOOLCHAIN" not in env:
        shown = subprocess.run(
            ["rustup", "show", "active-toolchain"], cwd=ROOT,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        if shown.returncode == 0 and shown.stdout.split():
            env["RUSTUP_TOOLCHAIN"] = shown.stdout.split()[0]
    return env


def check(lock_path, env, offline=False):
    """The problems of one lockfile, as lines."""
    manifest = lock_path.with_name("Cargo.toml")
    shown = lock_path.relative_to(ROOT).as_posix() if lock_path.is_relative_to(ROOT) else str(lock_path)
    if not manifest.is_file():
        return [f"{shown}: no Cargo.toml beside it, so nothing says what it should hold "
                "(delete the lock, or keep the manifest next to it)"]
    command = ["cargo", "metadata", "--locked", "--format-version", "1", "--manifest-path", str(manifest)]
    if offline:
        command.append("--offline")
    run = subprocess.run(
        command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env, cwd=ROOT,
    )
    if run.returncode != 0:
        errors = [l for l in run.stderr.splitlines() if l.startswith("error")]
        reason = errors[0].replace(str(lock_path.parent) + "/", "") if errors else run.stderr.strip()[-200:]
        if "cannot update the lock file" not in run.stderr:
            # Not a stale lock: cargo could not read the crates (no network, an
            # empty cache with --offline, a broken manifest). Say so.
            return [f"{shown}: cargo could not read the packages, so the lock was not checked "
                    f"(no network, or an empty cache with --offline?): {reason}"]
        return [f"{shown}: stale for its manifests: {reason}"
                + f" (fix: cargo metadata --manifest-path {manifest.relative_to(ROOT).as_posix()}"
                + " > /dev/null, then commit the lock)"]
    metadata = json.loads(run.stdout)
    member_ids = set(metadata["workspace_members"])
    members = {(p["name"], p["version"]) for p in metadata["packages"] if p["id"] in member_ids}
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    return [
        f"{shown}: lists {name} {version}, which no workspace member needs any more "
        "(delete its [[package]] entry and its mentions in other entries' dependencies)"
        for name, version in unreachable(lock, members)
    ]


def main(argv):
    env = toolchain_env()
    problems = []
    args = argv[1:]
    offline = "--offline" in args
    paths = lockfiles([a for a in args if a != "--offline"])
    for path in paths:
        problems += check(path, env, offline)
    for line in problems:
        print("check-lockfiles:", line, file=sys.stderr)
    if not problems:
        print(f"check-lockfiles: {len(paths)} lockfiles agree with their manifests")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

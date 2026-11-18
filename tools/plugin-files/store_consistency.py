#!/usr/bin/env python3
"""Whether PaperPaw's own store plugins (plugins.txt) are consistent for a
publication against the firmware of a release already out (#171): each
plugin's source version, the version last published, and what a device of
that firmware would accept. Nothing is built, uploaded or tagged here.

    store_consistency.py select  all|changed  --base <tag> [--source <dir>] [--releases <releases.json>]
    store_consistency.py check   <id>|all|changed ... --base <tag> [--source <dir>] [--releases <releases.json>]
    store_consistency.py files   <dir> --abi 7.<minor> --device <x4|x4-pro|x4-classic> <id>=<version> ...
    store_consistency.py entries --abi 7.<minor> --catalogue <zip|dir> ... [--releases <releases.json>] <id>=<version> ...

`--source` is a checkout of what is to be published (the repository the
script is in, where not given); `--base` is the release's tag (or any
commit of the same interface, for a local try); `--releases` is GitHub's
list of the releases (`gh api --paginate --slurp repos/<owner>/<repo>/releases`),
where not given nothing is published yet. Run from a trusted checkout, as
store_update.py is: the source is data to it.

The released interface is what the base's tree says: `native::INTERFACE`
and `DRAFT` (crates/paperpaw-sdk/src/native.rs) and the host's ABI
`MAJOR`.`MINOR` (crates/paperpaw-native/src/host_glue/paperpaw_abi.rs). A
file of interface 7 (format 5, abi/ppn5.md) has no fingerprint: a device
accepts it if its target, `abi_major` and `needs_minor` fit, and the draft
plays no part. So a plugin is refused here if

- the source has another interface than the base: the released firmware
  cannot load it, it needs a firmware release;
- its generated metadata (`plugins/<id>/native/metadata.bin`, the X4's, and
  `metadata.s3.bin`, the X4 Pro's and Classic's) has another ABI major, or
  needs a minor above the released firmware's: the very numbers the file's
  header gets;
- its version is older than the one published, or the same as the one
  published although its files (`plugins/<id>/`, `tools/plugin-files7/src/bin/<id>.rs`)
  differ from those of the commit that published it (`<id>-v<version>`, else
  the oldest firmware tag with that version), or it was published under
  another plugin interface (devices fetch a build only as an update).

`select changed` names the plugins with something to publish: not published at
the version they have, or changed (or published for another interface) since.
`files` checks built files before they are signed (header only: format 5,
the device's chip, abi major 7, `needs_minor` not above `--abi`, the name
`<id>-<version>-<device>.ppn`); `entries` checks the plugins' catalogue
entries (`catalogue/<id>.toml` in the entry ZIPs catalogue.py makes): the
version is the source's, not older than the published one, and each device has
a build of `7.<minor>` the released firmware accepts. `check` prints the
released `7.<minor>` on standard output; problems go to standard error as
`::error::` lines (a notice for what only informs).

A data plugin (data.toml) is not covered: it has no code, no ABI and no
file; versions.py and data.py hold it to its version.
"""

import argparse
import io
import json
import pathlib
import re
import struct
import subprocess
import sys
import tomllib
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import published  # noqa: E402
import store_release  # noqa: E402
import versions  # noqa: E402

NATIVE = versions.NATIVE
ABI_FILE = "crates/paperpaw-native/src/host_glue/paperpaw_abi.rs"
LISTED = "tools/plugin-files/plugins.txt"
DEVICES = {"x4": 5, "x4-pro": 9, "x4-classic": 9}
# The metadata record that goes into each device's file (the S3's is generated from the X4's).
METADATA = {"x4": "metadata.bin", "x4-pro": "metadata.s3.bin"}
ABI_MAJOR = re.compile(r"^pub const MAJOR: u16 = (\d+);$", re.MULTILINE)
ABI_MINOR = re.compile(r"^pub const MINOR: u16 = (\d+);$", re.MULTILINE)
FIRMWARE_TAG = re.compile(r"v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?")


class Refusal(Exception):
    pass


def git(source, *args):
    return subprocess.run(["git", *args], cwd=source, capture_output=True, text=True, encoding="utf-8")


def show(source, rev, path):
    """The text of `path` at `rev`; None where it is not there."""
    done = git(source, "show", f"{rev}:{path}")
    return done.stdout if done.returncode == 0 else None


def listed(source):
    """The ids of the source's plugins.txt."""
    text = (pathlib.Path(source) / LISTED).read_text(encoding="utf-8")
    words = (line.split("#", 1)[0].split() for line in text.splitlines())
    return [w[0] for w in words if w]


def interface_now(source):
    return versions.interface_at((pathlib.Path(source) / NATIVE).read_text(encoding="utf-8"))


def released(source, base):
    """What the base's firmware accepts: `{interface, draft, major, minor}`."""
    if git(source, "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}").returncode != 0:
        raise Refusal(f"{base} is not a tag or commit here: fetch the base's tag")
    native, abi = show(source, base, NATIVE), show(source, base, ABI_FILE)
    interface = versions.interface_at(native) if native else None
    major, minor = ABI_MAJOR.search(abi or ""), ABI_MINOR.search(abi or "")
    if interface is None or not (major and minor):
        raise Refusal(f"{base} has no interface-7 ABI ({ABI_FILE}): its firmware takes no format-5 file")
    return {"interface": interface, "draft": versions.draft_at(native), "major": int(major[1]), "minor": int(minor[1])}


def record_abi(path):
    """(major, minor) of a metadata record (`PPAM`, abi/metadata.md: bytes 12..16)."""
    data = pathlib.Path(path).read_bytes()
    if data[:4] != b"PPAM" or len(data) < 16:
        raise Refusal(f"{path}: not a metadata record")
    return struct.unpack_from("<HH", data, 12)


def published_names(releases_json):
    """The names of every finished asset of the releases (none where no list is given)."""
    if not releases_json:
        return []
    return list(published.assets(json.dumps([store_release.load(releases_json)])))


def reference(source, plugin, version):
    """The commit that published `plugin` at `version`: its own tag, else the oldest
    firmware tag whose manifest has that version; None where there is none."""
    tag = f"{plugin}-v{version}"
    if git(source, "rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}").returncode == 0:
        return tag
    manifest = f"plugins/{plugin}/Cargo.toml"
    firmware = [t for t in git(source, "tag", "--list", "v[0-9]*").stdout.split() if FIRMWARE_TAG.fullmatch(t)]
    for found in sorted(firmware, key=lambda t: versions.semver(t[1:])):
        text = show(source, found, manifest)
        if text and versions.version_in(text, show(source, found, "Cargo.toml") or "") == version:
            return found
    return None


def source_version(source, plugin):
    source = pathlib.Path(source)
    manifest = source / "plugins" / plugin / "Cargo.toml"
    if not manifest.is_file():
        raise Refusal(f"plugins/{plugin}/Cargo.toml: there is no such plugin")
    return versions.version_in(manifest.read_text(encoding="utf-8"), (source / "Cargo.toml").read_text(encoding="utf-8"))


def state(source, plugin, names):
    """`(now, before, changed, was, ref)`: the version of the source, the version published
    (or None), whether the files differ from those that published it, the plugin interface
    that commit had (or None), and the commit (or None)."""
    now = source_version(source, plugin)
    before = store_release.newest_published(names, plugin)
    if before is None:
        return now, None, True, None, None
    ref = reference(source, plugin, before)
    if ref is None:
        return now, before, False, None, None
    paths = [f"plugins/{plugin}", f"tools/plugin-files7/src/bin/{plugin}.rs"]
    changed = git(source, "diff", "--quiet", ref, "HEAD", "--", *paths).returncode != 0
    then = show(source, ref, NATIVE)
    return now, before, changed, (versions.interface_at(then) if then else None), ref


def plugin_problems(source, plugin, names, base_abi):
    """(problems, notes) of one plugin."""
    problems, notes = [], []
    now, before, changed, was, ref = state(source, plugin, names)
    interface = interface_now(source)
    if before is None:
        notes.append(f"{plugin} {now}: nothing of it is published yet")
    elif versions.semver(now) < versions.semver(before):
        problems.append(f"{plugin} is at {now}, {before} is published: its version cannot go back")
    elif versions.semver(now) == versions.semver(before):
        if ref is None:
            notes.append(f"{plugin} {now}: published, but no tag or release says which commit: its files could not be compared")
        elif changed:
            problems.append(f"{plugin} changed since {ref}, which published {before}, and its version is still {now}: "
                            f"raise it in plugins/{plugin}/Cargo.toml (devices update only to a newer version)")
        elif was is not None and was != interface:
            problems.append(f"{plugin} {now} was published for plugin interface {was} ({ref}), the source has interface {interface}: "
                            "a device of it takes only a build of its own, raise the version")
        else:
            notes.append(f"{plugin} {now}: published already, nothing new (the upload skips the same bytes)")
    for device, name in METADATA.items():
        path = pathlib.Path(source) / "plugins" / plugin / "native" / name
        if not path.is_file():
            problems.append(f"plugins/{plugin}/native/{name}: missing (the {device} file's ABI cannot be known)")
            continue
        major, minor = record_abi(path)
        if major != base_abi["major"]:
            problems.append(f"{plugin}: {name} has ABI major {major}, the released firmware {base_abi['major']}: a firmware release is needed")
        elif minor > base_abi["minor"]:
            problems.append(f"{plugin}: needs ABI {major}.{minor} ({name}), the released firmware has {major}.{base_abi['minor']}: "
                            "it would refuse the file; use only what the release has, or publish with a firmware release")
    return problems, notes


def interface_problems(source, base, base_abi):
    """The interface and draft of the source against the base's: (problems, notes)."""
    text = (pathlib.Path(source) / NATIVE).read_text(encoding="utf-8")
    interface, draft = versions.interface_at(text), versions.draft_at(text)
    if interface != base_abi["interface"]:
        return [f"the source has plugin interface {interface}, {base} has {base_abi['interface']}: "
                "its plugins are for another firmware, publish them with one"], []
    if draft != base_abi["draft"]:
        return [], [f"the source is at draft {draft}, {base} at {base_abi['draft']}: a format-5 file is not checked by "
                    "draft, only by target, major and needs_minor"]
    return [], []


def select(source, base, names, which):
    """The ids of plugins.txt: all, or those with something to publish."""
    ids = listed(source)
    if which == "all":
        return ids
    interface = released(source, base)["interface"]
    out = []
    for plugin in ids:
        now, before, changed, was, _ = state(source, plugin, names)
        if before is None or changed or versions.semver(now) > versions.semver(before) or (was is not None and was != interface):
            out.append(plugin)
    return out


def expand(source, base, names, words):
    """`words` (ids, or all/changed; commas or spaces) as ids, once each."""
    ids = []
    for word in [w for chunk in words for w in chunk.replace(",", " ").split()]:
        ids += select(source, base, names, word) if word in ("all", "changed") else [word]
    return list(dict.fromkeys(ids))


def file_problems(folder, device, abi, plugins):
    """The built files of `device` in `folder` for `plugins` (id, version), against the released `abi` (major, minor)."""
    problems = []
    for plugin, version in plugins:
        name = f"{plugin}-{version}-{device}.ppn"
        path = pathlib.Path(folder) / name
        if not path.is_file():
            problems.append(f"{name}: not built")
            continue
        data = path.read_bytes()
        if data[:4] != b"PPNP" or len(data) < 12:
            problems.append(f"{name}: not a plugin file")
            continue
        fmt, target, major, minor = struct.unpack_from("<HHHH", data, 4)
        if fmt != 5:
            problems.append(f"{name}: format {fmt}, not 5 (interface 7)")
        elif target != DEVICES[device]:
            problems.append(f"{name}: target {target}, the {device}'s is {DEVICES[device]}")
        elif major != abi[0] or minor > abi[1]:
            problems.append(f"{name}: ABI {major}.{minor}, the released firmware accepts {abi[0]}.{abi[1]} and below: it would refuse the file")
    return problems


def entry_texts(sources):
    """`catalogue/<id>.toml` of each ZIP or folder: id -> text."""
    out = {}
    for source in sources:
        path = pathlib.Path(source)
        if path.is_dir():
            out.update({f.stem: f.read_text(encoding="utf-8") for f in (path / "catalogue").glob("*.toml")})
            continue
        with zipfile.ZipFile(io.BytesIO(path.read_bytes())) as archive:
            for name in archive.namelist():
                if name.startswith("catalogue/") and name.endswith(".toml"):
                    out[pathlib.PurePosixPath(name).stem] = archive.read(name).decode("utf-8")
    return out


def entry_problems(entries, abi, plugins, names):
    """The catalogue `entries` (id -> text) of `plugins` (id, version) against the released `abi` and what is published."""
    problems = []
    for plugin, version in plugins:
        if plugin not in entries:
            problems.append(f"{plugin}: no catalogue entry")
            continue
        listed_versions = tomllib.loads(entries[plugin]).get("versions", [])
        mine = [v for v in listed_versions if v.get("version") == version]
        if not mine:
            problems.append(f"{plugin}: the catalogue entry has no version {version} "
                            f"(it lists {', '.join(v.get('version', '?') for v in listed_versions) or 'none'})")
            continue
        before = store_release.newest_published(names, plugin)
        if before is not None and versions.semver(version) < versions.semver(before):
            problems.append(f"{plugin}: the entry's {version} is older than {before}, which is published")
        got = {b.get("device"): b.get("abi", "") for b in mine[0].get("builds", [])}
        for device in DEVICES:
            have = got.get(device)
            found = re.fullmatch(r"(\d+)\.(\d+)", have or "")
            if have is None:
                problems.append(f"{plugin} {version}: the entry has no build for {device}")
            elif not found:
                problems.append(f"{plugin} {version}: the {device} build says abi {have!r}, not 7.<minor> (a fingerprint is interface 6)")
            elif int(found[1]) != abi[0] or int(found[2]) > abi[1]:
                problems.append(f"{plugin} {version}: the {device} build says abi {have}, the released firmware accepts {abi[0]}.{abi[1]} and below")
    return problems


def parse_abi(text):
    found = re.fullmatch(r"(\d+)\.(\d+)", text)
    if not found:
        raise Refusal(f"--abi {text}: expected 7.<minor>")
    return int(found[1]), int(found[2])


def pairs(words):
    out = []
    for word in words:
        plugin, _, version = word.partition("=")
        if not version:
            raise Refusal(f"{word}: expected <id>=<version>")
        out.append((plugin, version))
    return out


def report(problems, notes):
    for note in notes:
        print(f"::notice::{note}", file=sys.stderr)
    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    return 1 if problems else 0


def check(args):
    names = published_names(args.releases)
    base_abi = released(args.source, args.base)
    problems, notes = interface_problems(args.source, args.base, base_abi)
    known = listed(args.source)
    for plugin in expand(args.source, args.base, names, args.ids):
        if plugin not in known:
            problems.append(f"{plugin} is not in {LISTED}")
            continue
        more, extra = plugin_problems(args.source, plugin, names, base_abi)
        problems += more
        notes += extra
    status = report(problems, notes)
    if not status:
        print(f"{base_abi['major']}.{base_abi['minor']}")
    return status


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    sub = parser.add_subparsers(dest="what", required=True)
    for name in ("select", "check"):
        one = sub.add_parser(name)
        one.add_argument("ids", nargs="+" if name == "check" else 1)
        one.add_argument("--base", required=True)
        one.add_argument("--source", default=str(HERE.parent.parent))
        one.add_argument("--releases")
    one = sub.add_parser("files")
    one.add_argument("folder")
    one.add_argument("--abi", required=True)
    one.add_argument("--device", required=True, choices=sorted(DEVICES))
    one.add_argument("plugins", nargs="+")
    one = sub.add_parser("entries")
    one.add_argument("--abi", required=True)
    one.add_argument("--releases")
    one.add_argument("--catalogue", action="append", required=True)
    one.add_argument("plugins", nargs="+")
    args = parser.parse_args(argv)
    try:
        if args.what == "check":
            return check(args)
        if args.what == "select":
            print(" ".join(select(args.source, args.base, published_names(args.releases), args.ids[0])))
            return 0
        if args.what == "files":
            return report(file_problems(args.folder, args.device, parse_abi(args.abi), pairs(args.plugins)), [])
        return report(entry_problems(entry_texts(args.catalogue), parse_abi(args.abi), pairs(args.plugins),
                                     published_names(args.releases)), [])
    except Refusal as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

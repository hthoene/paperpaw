#!/usr/bin/env python3
"""What a firmware release holds, and the check that every plugin file its
catalogue names has an asset (#89).

The firmware release `v<version>` keeps only what belongs to the firmware:
the app, the images of each device, the emulators, the handbook, the web
emulator, the ESP32-S3's core libraries and the plugin library's catalogue
with its checksums (73 files). Plugin files are in the store release
(`store_release.py`).

    release_assets.py names <version>
        the names of the firmware release's assets, one a line (release.yml
        uploads exactly these, from dist/)
    release_assets.py catalogue --repo <owner/repo> [--plan plan.json] [--releases releases.json] <zip>...
        the same check of the addresses alone, for the catalogue ZIPs of
        plugins published between releases (store-plugin.yml)
    release_assets.py check --dist <dir> --version <version> --repo <owner/repo>
        [--plan plan.json] [--releases releases.json]
        fails where a file of the list is missing in <dist>, the list holds
        a plugin file, or a catalogue entry names an address that no asset
        has: a file of the plan (`store_release.py plan`, with its
        SHA-256) or one the releases list (releases.json) holds already
"""

import argparse
import json
import pathlib
import re
import sys
import tomllib
import zipfile


HERE = pathlib.Path(__file__).resolve().parent
# Run with `python3 -I` from a checkout that is not this one (store-plugin.yml):
# this folder, and no other, is where its modules come from.
sys.path.insert(0, str(HERE))
import published  # noqa: E402

DEVICES = ["x4", "x4-pro", "x4-classic", "x3"]
DEVICE_PARTS = [
    ".bin", ".bin.sha256", ".bin.sig", "-update.bin", "-update.bin.sha256",
    "-install.bin", "-install.bin.sha256", "-plugins.txt", "-abi.txt",
]
EMULATORS = ["windows-x64", "macos-arm64", "macos-x64", "linux-x64"]
SIGNED = ["", ".sha256", ".sig"]
# What a store release holds, never a firmware release: a plugin's file
# (`.ppn`) or its folder's ZIP (a plugin's id never begins with `paperpaw-`,
# as every firmware asset does).
PLUGIN_FILE = re.compile(r"(.*\.ppn|(?!paperpaw-).*\.zip)$")
EXPECTED = 73


def names(version):
    """The firmware release's assets for `version` (without the `v`)."""
    out = [f"paperpaw-{version}.apk", f"paperpaw-{version}.apk.sha256"]
    for device in DEVICES:
        out += [f"paperpaw-{device}-{version}{part}" for part in DEVICE_PARTS]
    out += [f"paperpaw-web-{version}.zip{part}" for part in SIGNED]
    for target in EMULATORS:
        out += [f"paperpaw-emu-{version}-{target}.zip{part}" for part in SIGNED]
        out += [f"paperpaw-fontgen-{version}-{target}.zip{part}" for part in SIGNED]
    out += [f"paperpaw-handbook-{version}.zip{part}" for part in SIGNED]
    out += [f"paperpaw-xtensa-std-{version}.tar.gz{part}" for part in SIGNED]
    out += [f"paperpaw-{version}-catalogue.zip", f"paperpaw-{version}-plugins.sha256"]
    return out


def entry_urls(zip_path):
    """(url, sha256) of every file the catalogue ZIP's entries name."""
    out = []
    with zipfile.ZipFile(zip_path) as z:
        for member in sorted(z.namelist()):
            if not member.endswith(".toml"):
                continue
            entry = tomllib.loads(z.read(member).decode("utf-8"))
            for version in entry.get("versions", []):
                out.append((member, version["url"], version["sha256"]))
                for build in version.get("builds", []):
                    out.append((member, build["url"], build["sha256"]))
    return out


def check_catalogue(urls, repo, planned, published_assets):
    """What is wrong with the addresses of a catalogue: `urls` as
    `entry_urls` gives them, `planned` name to (tag, sha256) of the files a
    run adds, `published_assets` name to (tag, address) of those out
    already."""
    prefix = f"https://github.com/{repo}/releases/download/"
    wrong = []
    for member, url, digest in urls:
        if not url.startswith(prefix):
            wrong.append(f"{member}: {url} is not under {prefix}")
            continue
        tag, _, name = url[len(prefix):].partition("/")
        if name in planned:
            if planned[name][0] != tag:
                wrong.append(f"{member}: {name} is planned for {planned[name][0]}, the entry says {tag}")
            elif planned[name][1] != digest:
                wrong.append(f"{member}: {name}'s checksum is not that of the planned file")
        elif name in published_assets:
            if published_assets[name][1] != url:
                wrong.append(f"{member}: {name} is published at {published_assets[name][1]}, not {url}")
        else:
            wrong.append(f"{member}: {url} has no asset, planned or published")
    return wrong


def check(dist, version, repo, planned, published_assets, catalogues=()):
    """Everything wrong with `dist` as the firmware release of `version`."""
    wrong = []
    listed = names(version)
    if len(listed) != EXPECTED or len(set(listed)) != len(listed):
        wrong.append(f"the list has {len(listed)} names, expected {EXPECTED} without duplicates")
    for name in listed:
        if not (dist / name).is_file():
            wrong.append(f"{name} is not in {dist}")
        if PLUGIN_FILE.match(name):
            wrong.append(f"{name} is a plugin file: those go to the store release")
    for catalogue in [dist / f"paperpaw-{version}-catalogue.zip", *catalogues]:
        if catalogue.is_file():
            wrong += check_catalogue(entry_urls(catalogue), repo, planned, published_assets)
    return wrong


def load_plan(path):
    """Name to (tag, sha256) of the files a plan adds."""
    if not path:
        return {}
    made = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    return {item["name"]: (made["tag"], item["sha256"]) for item in made["files"]}


def load_published(path):
    if not path:
        return {}
    return published.assets(pathlib.Path(path).read_text(encoding="utf-8"))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="what", required=True)
    one = sub.add_parser("names")
    one.add_argument("version")
    cat = sub.add_parser("catalogue")
    cat.add_argument("--repo", required=True)
    cat.add_argument("--plan")
    cat.add_argument("--releases")
    cat.add_argument("zips", type=pathlib.Path, nargs="+")
    two = sub.add_parser("check")
    two.add_argument("--dist", type=pathlib.Path, required=True)
    two.add_argument("--version", required=True)
    two.add_argument("--repo", required=True)
    two.add_argument("--plan")
    two.add_argument("--releases")
    two.add_argument("--catalogue", type=pathlib.Path, nargs="*", default=[])
    args = parser.parse_args(argv)
    if args.what == "names":
        print("\n".join(names(args.version)))
        return
    if args.what == "catalogue":
        planned, found = load_plan(args.plan), load_published(args.releases)
        wrong = []
        for zipped in args.zips:
            wrong += check_catalogue(entry_urls(zipped), args.repo, planned, found)
        for line in wrong:
            print(f"release_assets.py: {line}", file=sys.stderr)
        if wrong:
            sys.exit(1)
        print(f"{len(args.zips)} catalogue ZIPs: every address has an asset")
        return
    planned = {}
    if args.plan:
        made = json.loads(pathlib.Path(args.plan).read_text(encoding="utf-8"))
        planned = {item["name"]: (made["tag"], item["sha256"]) for item in made["files"]}
    found = {}
    if args.releases:
        found = published.assets(pathlib.Path(args.releases).read_text(encoding="utf-8"))
    wrong = check(args.dist, args.version, args.repo, planned, found, args.catalogue)
    for line in wrong:
        print(f"release_assets.py: {line}", file=sys.stderr)
    if wrong:
        sys.exit(1)
    print(f"{len(names(args.version))} firmware assets, {len(planned)} plugin files planned: every address has an asset")


if __name__ == "__main__":
    main()

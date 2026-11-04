#!/usr/bin/env python3
"""The plugin library's entries for PaperPaw's own plugins the store offers
(plugins.txt), from the files build.sh made for every reader of a release:

    python3 tools/plugin-files/catalogue.py <dist> <release> <base-url> [--date YYYY-MM-DD]
        [--only <id>] [--tag <tag>] [--data <dir>] [--releases <releases.json>]

<release> is the PaperPaw release whose firmware the files are built for
(their SDK is its), <tag> the tag they are published at (`v<release>`
where not given; a plugin's update between releases has its own,
`<id>-v<version>`). Each plugin has a version of its own, its crate's
(`plugins/<id>/Cargo.toml`), which its manifest says. <dist> holds
<id>-<version>-<device>.ppn for each reader built. Writes into <dist>:

- <id>-<version>.zip: the plugin's folder (its plugin.ini, and its How it
  works, `how/<lang>.txt`, which the store's page of it and readers'
  store show; the code comes as each reader's file), as readers install
  it;
- catalogue/<id>.toml: its entry in the plugin library, as the website's
  catalogue reads it (`catalogue/<id>.toml`, ADR 0023), with this
  version's builds, each
  file named by its address under <base-url> (the release's downloads)
  and its SHA-256;
- SHA256SUMS of the ZIPs and the plugin files.

Each file is named by the address of the release that holds it: <base-url>
(the store release this run adds to, `store_release.py`) for a file not
published before, and with `--releases` (GitHub's list of the repository's
releases, as `published.py` reads it) the address a file of that name has
there for one that was: a version's files are published once and keep
the release they were first published in.

With `--data <dir>` the data plugins of data.toml come too, their folders
in <dir> as data.py built them (`<dir>/<id>`): each its folder's ZIP,
<id>-<version>.zip, every file of the folder in it, and an entry of kind
`data` with that ZIP alone (no crate, no builds, nothing signed), its
names and summaries from data.toml.

Name and summary come from each plugin's translations
(`<id>.plugin.name`, `<id>.plugin.description`, English and German), the
author's key from the files' signatures (all must have the same), the
fingerprint (`abi`) from each file's header, How it works from the
plugin's `how/<lang>.txt` (the files its manifest names). Nothing is
invented: a plugin without a file, a name, a How it works in English or
a signature fails.
"""

import argparse
import datetime
import hashlib
import json
import pathlib
import re
import struct
import sys
import tomllib
import zipfile

import data
import published

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
DEVICES = ["x4", "x4-pro", "x4-classic"]
REPOSITORY = "https://github.com/hthoene/paperpaw"
SIGNATURE_LEN = 112


def fail(message):
    print(f"catalogue.py: {message}", file=sys.stderr)
    sys.exit(1)


def plugins():
    """(id, group) of plugins.txt."""
    out = []
    for line in (HERE / "plugins.txt").read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].split()
        if line:
            out.append((line[0], line[1]))
    return out


def plugin_version(plugin, plugins=ROOT / "plugins"):
    """The plugin's own version, as its crate says it (never PaperPaw's)."""
    path = plugins / plugin / "Cargo.toml"
    version = tomllib.loads(path.read_text(encoding="utf-8"))["package"].get("version")
    if not isinstance(version, str):
        fail(f"{path}: no version of its own")
    return version


def message(plugin, language, key):
    """A message of the plugin's translations in `language`."""
    for path in sorted((ROOT / "plugins" / plugin / "i18n").glob(f"*{language}.ini")):
        for line in path.read_text(encoding="utf-8").splitlines():
            name, sep, value = line.partition("=")
            if sep and name.strip() == key:
                return value.strip()
    return None


def ppn(path):
    """The fingerprint (16 hex digits) and the author's key (64 hex digits)
    of a signed plugin file."""
    data = path.read_bytes()
    if data[:4] != b"PPNP":
        fail(f"{path}: not a plugin file")
    fingerprint = struct.unpack_from("<Q", data, 8)[0]
    trailer = data[-SIGNATURE_LEN:]
    if len(data) < 80 + SIGNATURE_LEN or trailer[:4] != b"PPNS":
        fail(f"{path}: not signed")
    return f"{fingerprint:016x}", trailer[12:44].hex()


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def toml(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def how_files(plugin, plugins=ROOT / "plugins"):
    """The plugin's How it works as its folder holds it: (`how/<lang>.txt`,
    text) by language, lines ending in LF as a checkout on any system."""
    out = []
    for path in sorted((plugins / plugin / "how").glob("*.txt")):
        if re.fullmatch(r"[a-z]{2,3}(-[A-Z]{2})?", path.stem):
            # Read as text: lines end in LF, as from a checkout anywhere.
            out.append((f"how/{path.name}", path.read_text(encoding="utf-8")))
    return out


def card_files(plugin, plugins=ROOT / "plugins"):
    """The files the plugin reads from its folder on the card: those its
    `Cargo.toml` lists as `card-files` (globs from the plugin's folder, in
    `[package.metadata.paperpaw]`, e.g. `pictures/*.png`) as (name, bytes),
    texts with LF line ends. Only the kinds a package may hold: another
    (an `.html`, a `.jpg`) is a mistake the plugin library would refuse."""
    manifest = tomllib.loads((plugins / plugin / "Cargo.toml").read_text(encoding="utf-8"))
    patterns = manifest["package"].get("metadata", {}).get("paperpaw", {}).get("card-files", [])
    kinds = json.loads((ROOT / "tools/publish/store-rules.json").read_text())["package_kinds"]
    out = []
    for pattern in patterns:
        found = sorted(path for path in (plugins / plugin).glob(pattern) if path.is_file())
        if not found:
            fail(f"{plugin}: card-files `{pattern}` matches no file")
        for path in found:
            name = path.relative_to(plugins / plugin).as_posix()
            kind = path.suffix.lstrip(".").lower()
            if kind not in kinds:
                fail(f"{plugin}: {name} is no kind a package may hold ({', '.join(kinds)})")
            if kind in ("txt", "md", "ini", "css"):
                out.append((name, path.read_text(encoding="utf-8").encode("utf-8")))
            else:
                out.append((name, path.read_bytes()))
    return out


def folder_zip(path, plugin_ini, files=()):
    """The folder's ZIP: `plugin.ini` and `files` (name, text or bytes),
    the same bytes for the same contents."""
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, text in [("plugin.ini", plugin_ini), *sorted(files)]:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            z.writestr(info, text)


def locator(base, published_assets=None, checksum=None):
    """The address of a file by its name: where it was published before,
    else under `base` (the release this run adds it to). Given the file's
    SHA-256, one published before must be the same bytes (`checksum` says the
    published ones'), else the entry would name an address whose file has
    another checksum."""
    found = published_assets or {}

    def url_of(name, digest=None):
        if name not in found:
            return f"{base}/{name}"
        if digest is not None and checksum is not None and checksum(name) != digest:
            fail(
                f"{name} is published in {found[name][0]} with other bytes than this build's: "
                "a version is published once, so raise the plugin's version"
            )
        return found[name][1]

    return url_of


def data_entries(source, dist, release, base, date, only=None, url_of=None):
    """The data plugins of data.toml from their folders in `source`: their
    ZIPs into `dist` and their entries into `dist/catalogue`; the ZIPs'
    (sha256, name) for SHA256SUMS."""
    sums = []
    url_of = url_of or locator(base)
    sdk = ".".join(release.split("-")[0].split("+")[0].split(".")[:2])
    for plugin, fields in data.plugins().items():
        if only not in (None, plugin):
            continue
        folder = source / plugin
        ini_path = folder / "plugin.ini"
        if not ini_path.exists():
            fail(f"{plugin}: no folder in {source} (tools/plugin-files/data.py builds it)")
        version = fields["version"]
        ini = ini_path.read_text(encoding="utf-8")
        if f"version = {version}\n" not in ini:
            fail(f"{plugin}: its plugin.ini is not at data.toml's version {version}")
        summary = fields["description"]
        if len(summary) > 160 or len(fields["de"]["description"]) > 160:
            fail(f"{plugin}: its description is longer than a summary may be (160)")
        files = [
            (path.relative_to(folder).as_posix(), path.read_bytes())
            for path in sorted(folder.rglob("*"))
            if path.is_file() and path.name != "plugin.ini"
        ]
        zipped = dist / f"{plugin}-{version}.zip"
        folder_zip(zipped, ini, files)
        digest = sha256(zipped)
        sums.append((digest, zipped.name))
        lines = [
            f"# PaperPaw's own data plugin, built for PaperPaw {release} (tools/plugin-files/data.toml).",
            f"id = {toml(plugin)}",
            f"name = {toml(fields['name'])}",
            f"summary = {toml(summary)}",
            'kind = "data"',
            'author = "PaperPaw"',
            f"licence = {toml(fields['licence'])}",
            f"source = {toml(REPOSITORY + '/blob/main/' + fields['build'])}",
            f"group = {toml(fields['group'])}",
            "devices = [" + ", ".join(toml(d) for d in fields["devices"]) + "]",
        ]
        if fields.get("off_by_default"):
            lines.append("off_by_default = true")
        lines += [
            "",
            "[i18n.de]",
            f"name = {toml(fields['de']['name'])}",
            f"summary = {toml(fields['de']['description'])}",
            "",
            "[[versions]]",
            f"version = {toml(version)}",
            f"sdk = {toml(sdk)}",
            f"date = {toml(date)}",
            f"url = {toml(url_of(zipped.name, digest))}",
            f"sha256 = {toml(digest)}",
        ]
        (dist / "catalogue" / f"{plugin}.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return sums


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("dist", type=pathlib.Path)
    parser.add_argument("release")
    parser.add_argument("base_url")
    parser.add_argument("--date", default=datetime.date.today().isoformat())
    # One plugin alone: an update of it between releases (store-plugin.yml).
    parser.add_argument("--only")
    parser.add_argument("--tag")
    # The data plugins' folders (data.py), for their entries too.
    parser.add_argument("--data", type=pathlib.Path)
    # GitHub's list of the releases: files published before keep their address.
    parser.add_argument("--releases", type=pathlib.Path)
    args = parser.parse_args()
    dist, release, base = args.dist, args.release, args.base_url.rstrip("/")
    listing = args.releases.read_text(encoding="utf-8") if args.releases else None
    url_of = locator(
        base,
        published.assets(listing) if listing else None,
        published.checksums(listing) if listing else None,
    )
    tag = args.tag or f"v{release}"
    sdk = ".".join(release.split("-")[0].split("+")[0].split(".")[:2])
    (dist / "catalogue").mkdir(parents=True, exist_ok=True)
    keys = set()
    sums = []
    chosen = [(p, g) for p, g in plugins() if args.only in (None, p)]
    only_data = args.only is not None and args.only in data.plugins()
    if not chosen and not only_data:
        fail(f"{args.only} is not in plugins.txt or data.toml")
    for plugin, group in chosen:
        name = message(plugin, "en", f"{plugin}.plugin.name")
        summary = message(plugin, "en", f"{plugin}.plugin.description")
        if not name or not summary:
            fail(f"{plugin}: no {plugin}.plugin.name or .description in English")
        if len(summary) > 160:
            fail(f"{plugin}: its description is longer than a summary may be (160)")
        version = plugin_version(plugin)
        how = how_files(plugin)
        if "how/en.txt" not in dict(how):
            fail(f"{plugin}: no How it works in English (plugins/{plugin}/how/en.txt)")
        builds = []
        for device in DEVICES:
            path = dist / f"{plugin}-{version}-{device}.ppn"
            if path.exists():
                abi, key = ppn(path)
                keys.add(key)
                digest = sha256(path)
                sums.append((digest, path.name))
                builds.append((device, abi, url_of(path.name, digest), digest))
        if not builds:
            fail(f"{plugin}: no file built for any reader")
        plugin_ini = (
            "[plugin]\n"
            f"id = {plugin}\n"
            f"name = {name}\n"
            f"description = {summary}\n"
            f"version = {version}\n"
        )
        folder = dist / f"{plugin}-{version}.zip"
        folder_zip(folder, plugin_ini, [*how, *card_files(plugin)])
        folder_digest = sha256(folder)
        sums.append((folder_digest, folder.name))
        lines = [
            f"# PaperPaw's own plugin, built for PaperPaw {release} (tools/plugin-files).",
            "# Newer versions are added above older ones.",
            f"id = {toml(plugin)}",
            f"name = {toml(name)}",
            f"summary = {toml(summary)}",
            'kind = "code"',
            'author = "PaperPaw"',
            "author_key = AUTHOR_KEY",
            'licence = "Apache-2.0"',
            f"source = {toml(REPOSITORY + '/tree/main/plugins/' + plugin)}",
            # The crate a firmware lists when it has the plugin built in
            # (ADR 0022): updates find it here as a file.
            f"crate = {toml('paperpaw-plugin-' + plugin)}",
            f"group = {toml(group)}",
            "devices = [" + ", ".join(toml(b[0]) for b in builds) + "]",
        ]
        de_name = message(plugin, "de", f"{plugin}.plugin.name")
        de_summary = message(plugin, "de", f"{plugin}.plugin.description")
        if de_name and de_summary and len(de_summary) <= 160:
            lines += ["", "[i18n.de]", f"name = {toml(de_name)}", f"summary = {toml(de_summary)}"]
        lines += [
            "",
            "[[versions]]",
            f"version = {toml(version)}",
            f"sdk = {toml(sdk)}",
            f"date = {toml(args.date)}",
            f"url = {toml(url_of(folder.name, folder_digest))}",
            f"sha256 = {toml(folder_digest)}",
            "",
            "[versions.source]",
            f"repository = {toml(REPOSITORY)}",
            f"revision = {toml(tag)}",
            'path = "tools/plugin-files"',
            f"bin = {toml(plugin)}",
        ]
        for device, abi, url, digest in builds:
            lines += [
                "",
                "[[versions.builds]]",
                f"device = {toml(device)}",
                f"abi = {toml(abi)}",
                f"url = {toml(url)}",
                f"sha256 = {toml(digest)}",
            ]
        (dist / "catalogue" / f"{plugin}.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")
    if chosen and len(keys) != 1:
        fail(f"the files are signed by {len(keys)} keys, not one")
    key = keys.pop() if chosen else None
    for entry in (dist / "catalogue").glob("*.toml"):
        entry.write_text(entry.read_text(encoding="utf-8").replace("AUTHOR_KEY", toml(key or "")), encoding="utf-8")
    if args.data is not None:
        sums += data_entries(args.data, dist, release, base, args.date, args.only, url_of)
    (dist / "SHA256SUMS").write_text("".join(f"{d}  {n}\n" for d, n in sorted(sums, key=lambda s: s[1])), encoding="utf-8")
    print(f"{len(chosen)} plugins, signed by {key}")


if __name__ == "__main__":
    main()

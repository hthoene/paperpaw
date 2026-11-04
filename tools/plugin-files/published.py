#!/usr/bin/env python3
"""A plugin's version is published once: the files of PaperPaw's own
store plugins a release built, against those its releases published
before under the same names (`<id>-<version>-<device>.ppn`, the folder
`<id>-<version>.zip`), as GitHub lists them:

    gh api --paginate --slurp repos/<owner>/<repo>/releases > releases.json
    python3 tools/plugin-files/published.py files <dir> releases.json
    python3 tools/plugin-files/published.py folders <dir> releases.json

`files`, before catalogue.py: a file of a version published before, for
the same chip, firmware fingerprint and compiler, is taken as published
(its bytes, so a release never offers other bytes under a version that
has them); one for another fingerprint (the plugin interface changed) is
a new build of that version and stays. `folders`, after catalogue.py: a
folder of a version published before must be the same bytes, else it
fails (versions.py keeps a plugin that changed from its old version).
"""

import hashlib
import json
import pathlib
import sys
import urllib.request

# Where the header says what a file runs on (paperpaw-native's Header):
# its chip, its fingerprint, its compiler.
RUNS_ON = [(6, 8), (8, 16), (44, 48)]


def fail(message):
    print(f"published.py: {message}", file=sys.stderr)
    sys.exit(1)


def releases(text):
    """The releases of GitHub's list `text` (pages as `--slurp` or
    `--paginate` give them), drafts left out, the oldest first."""
    decoder = json.JSONDecoder()
    pages, at = [], 0
    text = text.strip()
    while at < len(text):
        page, at = decoder.raw_decode(text, at)
        pages.append(page)
        while at < len(text) and text[at].isspace():
            at += 1
    found = []
    for page in pages:
        for item in page:
            found.extend(item if isinstance(item, list) else [item])
    # Oldest first: GitHub lists the newest first.
    found = sorted(found, key=lambda r: r.get("created_at") or "")
    return [release for release in found if not release.get("draft")]


def usable(asset):
    """Whether an asset is uploaded in full: GitHub lists a file whose
    upload broke off with another state (`starter`, `open`) and some size."""
    return asset.get("state", "uploaded") == "uploaded"


def details(text):
    """Name to what GitHub says of every finished asset of the releases in
    `text`, drafts left out; a name in several, the first (oldest)
    release's: {"tag", "url", "size", "digest"} (`digest` is
    `sha256:<hex>`, or None where GitHub gives none)."""
    found = {}
    for release in releases(text):
        for asset in release.get("assets", []):
            if usable(asset):
                found.setdefault(
                    asset["name"],
                    {
                        "tag": release["tag_name"],
                        "url": asset["browser_download_url"],
                        "size": asset.get("size"),
                        "digest": asset.get("digest"),
                    },
                )
    return found


def assets(text):
    """Name to (tag, address) of every finished asset of the releases in
    GitHub's list `text`, drafts left out; a name in several, the first
    (oldest) release's."""
    return {name: (info["tag"], info["url"]) for name, info in details(text).items()}


def partial(text):
    """Name to tag of the assets whose upload is not finished and that no
    finished asset of the same name makes good."""
    done = details(text)
    found = {}
    for release in releases(text):
        for asset in release.get("assets", []):
            if not usable(asset) and asset["name"] not in done:
                found.setdefault(asset["name"], release["tag_name"])
    return found


def sha256_of(info, fetch=None):
    """The SHA-256 (hex) of a published asset: GitHub's `digest` where it
    gives one, else the bytes `fetch` downloads from its address."""
    digest = info.get("digest") or ""
    if digest.startswith("sha256:"):
        return digest[len("sha256:"):].lower()
    return hashlib.sha256((fetch or download)(info["url"])).hexdigest()


def checksums(text, fetch=None):
    """A function: name of a published asset to its SHA-256 (hex), looked up
    once for each name."""
    found, seen = details(text), {}

    def checksum(name):
        if name not in seen:
            seen[name] = sha256_of(found[name], fetch)
        return seen[name]

    return checksum


def runs_on(data):
    return [data[a:b] for a, b in RUNS_ON]


def files(folder, published, fetch):
    """Takes each file of a version published before for the same
    firmware as published; what it did, a line a file."""
    said = []
    for path in sorted(folder.glob("*.ppn")):
        if path.name not in published:
            continue
        tag, url = published[path.name]
        before = fetch(url)
        if runs_on(before) == runs_on(path.read_bytes()):
            path.write_bytes(before)
            said.append(f"{path.name}: as published in {tag}")
        else:
            said.append(f"{path.name}: a new build, for another firmware than {tag}'s")
    return said


def folders(folder, published, fetch):
    """The folders of versions published before that differ from it."""
    return [
        f"{path.name} differs from the one published in {published[path.name][0]}: "
        "a plugin that changed needs a newer version"
        for path in sorted(folder.glob("*.zip"))
        if path.name in published and fetch(published[path.name][1]) != path.read_bytes()
    ]


def download(url):
    with urllib.request.urlopen(url, timeout=60) as reply:
        return reply.read()


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in ("files", "folders"):
        fail("usage: published.py files|folders <dir> <releases.json>")
    what, folder = sys.argv[1], pathlib.Path(sys.argv[2])
    published = assets(pathlib.Path(sys.argv[3]).read_text(encoding="utf-8"))
    if what == "files":
        for line in files(folder, published, download):
            print(line)
    else:
        differ = folders(folder, published, download)
        if differ:
            fail("\n".join(differ))


if __name__ == "__main__":
    main()

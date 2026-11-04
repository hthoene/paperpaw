#!/usr/bin/env python3
"""The store release: where PaperPaw's own plugin files are published, apart
from the firmware's release (#89, #171).

A firmware release (`v<version>`) holds the firmware, the emulators, the
handbook, its catalogue and its checksums: some seventy files. Every plugin
file, whether built for a release or for an update between releases, goes to
a *store release* of the same repository instead, one for each year:

    plugins-2026        a pre-release, never the latest, only ever added to
    plugins-2026-2      the next one, when the first holds 800 assets

so the list of releases does not fill with plugin files and an update does
not wait for a firmware. Nothing in it is replaced or deleted, so addresses
stay what the catalogue says they are. A plugin's version is published
once: a file whose name is published already (in any release) is not
uploaded again, and a name that is published with other bytes stops the run
(the plugin needs a newer version, `versions.py`).

    gh api --paginate --slurp repos/<owner>/<repo>/releases > releases.json

    store_release.py choose --releases releases.json --year 2026 --dist <dir>
        prints the tag to add to: the newest store release of the year, or
        the next one where it would hold more than 800 assets with what
        <dist> brings (`catalogue.py` writes the ZIPs after this, so it
        counts the most they can be)
    store_release.py plan --releases releases.json --tag <tag> --dist <dir>
        [--extra <file>...] --out plan.json
        the files to upload: those of <dist> (and the extras, a catalogue ZIP
        and its sums) not published before; a name published with the same
        SHA-256 is skipped (so a run that broke off can be started again), one
        with other bytes, or a file whose upload was never finished, fails
        ("raise the plugin's version"); also when the release would hold
        more than 900
    store_release.py upload --plan plan.json [--dry-run]
        makes the release if it is not there (a pre-release, not the
        latest), uploads the files in batches without ever replacing one,
        retries each that fails, and checks the release lists them all, each
        in full (state uploaded) with its SHA-256; --dry-run prints what it
        would run and changes nothing
    store_release.py versions --releases releases.json <id>=<version>...
        the version each plugin has in the source (its crate's, or
        data.toml's) against those published: fails for one older than one
        published, or an id like `v2-reader` whose git tag would start
        release.yml; prints `<id> <version>` (one published already is
        left to `plan`, which skips the same bytes and fails other ones)

The workflows (release.yml, store-plugin.yml) run it; `release_assets.py`
checks what they plan.
"""

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import time


HERE = pathlib.Path(__file__).resolve().parent
# Run with `python3 -I` from a checkout that is not this one (store-plugin.yml):
# this folder, and no other, is where its modules come from.
sys.path.insert(0, str(HERE))
import published  # noqa: E402

# Where a store release stops taking files, and where the run stops with an
# error: GitHub's limit is 1000 assets for each release.
SOFT_LIMIT = 800
TRIPWIRE = 900
# Files for each `gh release upload`.
BATCH = 20
RETRIES = 3
STORE_TAG = re.compile(r"^plugins-(\d{4})(?:-([2-9]|[1-9]\d+))?$")
# What a plugin file is called: `<id>-<version>-<device>.ppn`, the folder or
# data ZIP `<id>-<version>.zip`.
KINDS = (".ppn", ".zip")
NOTES = (
    "Files of the plugins in PaperPaw's store (paperpaw.org), added to and never changed: "
    "each version of a plugin is published once, here or in the release of its firmware. "
    "Install plugins from the store, not from this page."
)


class Stop(Exception):
    """A run that must not go on; the message says why."""


def store_tag(year, shard=1):
    return f"plugins-{year}" if shard == 1 else f"plugins-{year}-{shard}"


def shards(releases, year):
    """The store releases of `year` by their number: (tag, assets)."""
    found = {}
    for release in releases:
        match = STORE_TAG.match(release["tag_name"])
        if match and int(match.group(1)) == year:
            found[int(match.group(2) or 1)] = (release["tag_name"], len(release.get("assets", [])))
    return found


def choose(releases, year, incoming, counted=None):
    """The tag to add `incoming` files to, and whether it exists: the newest
    store release of the year, or the next number where it would hold more
    than SOFT_LIMIT. `counted` (tag to the number of assets it holds, every
    page of them) replaces the count of the releases list, which may stop at
    a page."""
    if incoming > SOFT_LIMIT:
        raise Stop(f"{incoming} files are more than a store release holds ({SOFT_LIMIT}): publish fewer at a time")
    found = shards(releases, year)
    if not found:
        return store_tag(year), False
    newest = max(found)
    tag, count = found[newest]
    if counted is not None:
        count = max(count, counted(tag))
    if count + incoming <= SOFT_LIMIT:
        return tag, True
    return store_tag(year, newest + 1), False


def files_of(folder):
    """The plugin files in `folder`, not in a folder below it."""
    return sorted(p for p in pathlib.Path(folder).iterdir() if p.is_file() and p.suffix in KINDS)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def plan(releases, tag, files, extras=(), repo="", fetch=None, counted=None):
    """What to upload to `tag`: the files and extras (paths) whose names are
    not published yet. A name that is published must be the same bytes
    (SHA-256, not the size), else the run stops; so does a name whose upload
    was never finished. `fetch` downloads an asset GitHub gives no digest of;
    `counted` is as for `choose`."""
    text = json.dumps([releases])
    names = published.details(text)
    checksum = published.checksums(text, fetch)
    broken = published.partial(text)
    held = next((len(r.get("assets", [])) for r in releases if r["tag_name"] == tag), 0)
    if counted is not None and any(r["tag_name"] == tag for r in releases):
        held = max(held, counted(tag))
    new, said = [], []
    for path in [*files, *map(pathlib.Path, extras)]:
        name = path.name
        if name in broken:
            raise Stop(
                f"{name} is in {broken[name]} but its upload was never finished: delete it there "
                f"(gh release delete-asset {broken[name]} {name}) and run again"
            )
        mine = digest(path)
        if name in names:
            where = names[name]["tag"]
            theirs = checksum(name)
            if theirs != mine:
                raise Stop(
                    f"{name} is published in {where} with other bytes (sha256 {theirs[:12]}, built {mine[:12]}): "
                    "a version is published once, so raise the plugin's version"
                )
            said.append(f"{name}: published in {where}, the same bytes")
            continue
        new.append({"name": name, "path": str(path), "size": path.stat().st_size, "sha256": mine})
    seen = [item["name"] for item in new]
    if len(seen) != len(set(seen)):
        raise Stop("the same name twice in one run: " + ", ".join(sorted({n for n in seen if seen.count(n) > 1})))
    after = held + len(new)
    if after > TRIPWIRE:
        raise Stop(f"{tag} would hold {after} assets (more than {TRIPWIRE}); GitHub stops at 1000")
    return {
        "repo": repo,
        "tag": tag,
        "create": not any(r["tag_name"] == tag for r in releases),
        "held": held,
        "after": after,
        "files": new,
        "published": said,
    }


def commands(plan_):
    """The `gh` commands of a plan, one argument list each (no file is ever
    replaced: no --clobber)."""
    repo = ["-R", plan_["repo"]] if plan_["repo"] else []
    tag = plan_["tag"]
    out = []
    if plan_["create"]:
        year = STORE_TAG.match(tag).group(1)
        out.append(
            ["gh", "release", "create", tag, *repo, "--prerelease", "--latest=false",
             "--title", f"Store plugins {year}", "--notes", NOTES]
        )
    paths = [item["path"] for item in plan_["files"]]
    for at in range(0, len(paths), BATCH):
        out.append(["gh", "release", "upload", tag, *repo, *paths[at : at + BATCH]])
    return out


def run(argv):
    done = subprocess.run(argv, capture_output=True, text=True, check=False)
    return done.returncode, done.stdout + done.stderr


def listed(tag, repo, runner):
    """Name to {"size", "state", "digest"} of the assets the release has now,
    every page of them; None where the release cannot be read."""
    code, out = runner(["gh", "api", f"repos/{repo}/releases/tags/{tag}", "--jq", ".id"])
    if code != 0 or not out.strip().isdigit():
        return None
    code, out = runner(
        ["gh", "api", "--paginate", f"repos/{repo}/releases/{out.strip()}/assets",
         "--jq", ".[] | {name, size, state, digest}"]
    )
    if code != 0:
        return None
    found = {}
    for line in out.splitlines():
        if line.strip():
            asset = json.loads(line)
            found[asset["name"]] = {
                "size": asset.get("size"),
                "state": asset.get("state") or "uploaded",
                "digest": asset.get("digest"),
            }
    return found


def asset_count(tag, repo, runner=run):
    """How many assets the release has, every page counted; 0 for one that
    is not there."""
    return len(listed(tag, repo, runner) or {})


def status(item, now, tag, repo, fetch):
    """`ok` where the release has the item in full with its bytes, `missing`
    or `partial` where it has not (a partial one has to go before a new
    try); other bytes under the name stop the run."""
    if now is None:
        return "missing"
    if now["state"] != "uploaded":
        return "partial"
    info = {"digest": now.get("digest"), "url": f"https://github.com/{repo}/releases/download/{tag}/{item['name']}"}
    theirs = published.sha256_of(info, fetch)
    if theirs != item["sha256"]:
        raise Stop(
            f"{tag} holds {item['name']} with other bytes (sha256 {theirs[:12]}, built {item['sha256'][:12]}): "
            "a version is published once, so raise the plugin's version"
        )
    return "ok"


def upload(plan_, runner=run, sleep=time.sleep, say=print, fetch=None):
    """Does what `commands` says, and checks the release has every file in
    full with its SHA-256; a batch that fails goes file by file, each tried
    again (an unfinished asset of the failed try is deleted first)."""
    tag, repo = plan_["tag"], plan_["repo"]
    flag = ["-R", repo] if repo else []
    if plan_["create"]:
        code, out = runner(commands({**plan_, "files": []})[0])
        if code != 0 and listed(tag, repo, runner) is None:
            # Another run may have made it meanwhile: it is there, or it is not.
            raise Stop(f"the release {tag} could not be made: {out.strip()}")
    items = plan_["files"]
    for at in range(0, len(items), BATCH):
        batch = items[at : at + BATCH]
        code, out = runner(["gh", "release", "upload", tag, *flag, *[i["path"] for i in batch]])
        say(f"{tag}: {len(batch)} files {'ok' if code == 0 else 'failed: ' + out.strip()}")
        if code == 0:
            continue
        for item in batch:
            for attempt in range(1, RETRIES + 1):
                where = status(item, (listed(tag, repo, runner) or {}).get(item["name"]), tag, repo, fetch)
                if where == "ok":
                    break
                if where == "partial":
                    runner(["gh", "release", "delete-asset", tag, item["name"], *flag, "--yes"])
                code, out = runner(["gh", "release", "upload", tag, *flag, item["path"]])
                if code == 0:
                    break
                say(f"{item['name']}: attempt {attempt} failed: {out.strip()}")
                sleep(5 * attempt)
            else:
                raise Stop(f"{item['name']} could not be uploaded to {tag}")
    present = listed(tag, repo, runner)
    if present is None:
        raise Stop(f"{tag} cannot be listed after the upload")
    missing = [i["name"] for i in items if status(i, present.get(i["name"]), tag, repo, fetch) != "ok"]
    if missing:
        raise Stop(f"{tag} lacks, or has an unfinished upload of: " + ", ".join(missing))
    say(f"{tag}: {len(items)} files added, {len(present)} assets now")


def newest_published(names, plugin):
    """The newest version of `plugin` among published file names, or None."""
    pattern = re.compile(rf"^{re.escape(plugin)}-(\d+(?:\.\d+)*(?:[-+][0-9A-Za-z.-]+)??)(?:-[a-z0-9-]+\.ppn|\.zip)$")
    versions = [m.group(1) for name in names if (m := pattern.match(name))]
    return max(versions, key=version_tuple) if versions else None


def versions(releases, current):
    """`<id> <version>` for each plugin of `current` (id to the version the
    source has now) that is not older than a published one. A version that
    is published already is allowed: `plan` skips its files when they are the
    same bytes (a run that broke off starts again) and stops for other ones."""
    names = published.assets(json.dumps([releases]))
    lines = []
    for plugin, version in current.items():
        if re.match(r"v\d", plugin):
            # Its git tag `<id>-v<version>` would begin like a firmware tag
            # (`v[0-9]*`) and start release.yml.
            raise Stop(f"{plugin}: an id that begins with v and a digit would make a tag that starts a firmware release")
        before = newest_published(names, plugin)
        if before is not None and version_tuple(version) < version_tuple(before):
            raise Stop(f"{plugin} {version} is older than {before}, which is published")
        lines.append(f"{plugin} {version}")
    return lines


def version_tuple(version):
    """Numbers of a version for comparing: 1.2.10 after 1.2.9; a pre-release
    (1.0.0-beta.1) before its release."""
    core, _, pre = version.partition("-")
    numbers = tuple(int(n) for n in re.findall(r"\d+", core))
    return (*numbers, 0 if pre else 1, tuple(int(n) if n.isdigit() else 0 for n in re.findall(r"[0-9A-Za-z]+", pre)))


def load(path):
    return published.releases(pathlib.Path(path).read_text(encoding="utf-8"))


def upper_bound(dist):
    """The most files `dist` brings once catalogue.py has made the folders'
    ZIPs: its plugin files, a ZIP for each plugin of the store and each data
    plugin."""
    import catalogue
    import data

    return len(files_of(dist)) + len(catalogue.plugins()) + len(data.plugins())


def counter(repo):
    """The exact count of a release's assets (`choose`, `plan`), or none
    where there is no repository to ask."""
    return (lambda tag: asset_count(tag, repo)) if repo else None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="what", required=True)
    one = sub.add_parser("choose")
    one.add_argument("--releases", required=True)
    one.add_argument("--year", type=int, required=True)
    one.add_argument("--dist", required=True)
    one.add_argument("--incoming", type=int)
    one.add_argument("--repo", default="")
    two = sub.add_parser("plan")
    two.add_argument("--releases", required=True)
    two.add_argument("--tag", required=True)
    two.add_argument("--dist", required=True)
    two.add_argument("--extra", nargs="*", default=[])
    two.add_argument("--repo", default="")
    two.add_argument("--out", required=True)
    three = sub.add_parser("upload")
    three.add_argument("--plan", required=True)
    three.add_argument("--dry-run", action="store_true")
    four = sub.add_parser("versions")
    four.add_argument("--releases", required=True)
    four.add_argument("current", nargs="+", metavar="ID=VERSION")
    args = parser.parse_args(argv)
    try:
        if args.what == "choose":
            incoming = args.incoming if args.incoming is not None else upper_bound(args.dist)
            tag, _ = choose(load(args.releases), args.year, incoming, counter(args.repo))
            print(tag)
        elif args.what == "plan":
            if not STORE_TAG.match(args.tag):
                raise Stop(f"{args.tag} is no store release (plugins-<year>)")
            made = plan(load(args.releases), args.tag, files_of(args.dist), args.extra, args.repo, counted=counter(args.repo))
            pathlib.Path(args.out).write_text(json.dumps(made, indent=2) + "\n", encoding="utf-8")
            for line in made["published"]:
                print(line)
            print(f"{args.tag}: {len(made['files'])} files to add to {made['held']} ({made['after']} after)")
        elif args.what == "upload":
            made = json.loads(pathlib.Path(args.plan).read_text(encoding="utf-8"))
            if args.dry_run:
                for argv_ in commands(made):
                    print("DRY RUN:", " ".join(argv_) if len(argv_) < 12 else " ".join(argv_[:8]) + " ...")
            else:
                upload(made)
        else:
            pairs = dict(pair.split("=", 1) for pair in args.current)
            for line in versions(load(args.releases), pairs):
                print(line)
    except Stop as stop:
        print(f"store_release.py: {stop}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()

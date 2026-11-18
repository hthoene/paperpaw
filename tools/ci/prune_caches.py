#!/usr/bin/env python3
"""Keeps the newest Rust cache entry of each prefix, deletes the older ones.

GitHub keeps 10 GB of caches for the repository and evicts the least recently
used entry when it is full. Every change of a Cargo.lock leaves the entry the
rust-cache action made before it next to the new one (about 1 GB each for the
firmware builds), and those stale entries push out the small ones that save
minutes (the Xtensa toolchain, the apt packages: 5 minutes per S3 job).

Only `v0-rust-*` entries are touched: their key ends in two 8-digit hashes
(lock files, environment), the rest of it names the job and the platform. Per
branch (`ref`) and prefix the newest entry by creation time stays.

usage: prune_caches.py <owner/repo> [--dry-run]     (GH_TOKEN with actions: write)
"""

import json
import re
import subprocess
import sys

HASHES = re.compile(r"-[0-9a-f]{8}-[0-9a-f]{8}$")


def stale(entries):
    """The ids of the entries to delete, from `{id, key, ref, created_at}` dicts."""
    newest = {}
    for entry in entries:
        key = entry["key"]
        if not key.startswith("v0-rust-") or not HASHES.search(key):
            continue
        group = (entry["ref"], HASHES.sub("", key))
        best = newest.get(group)
        if best is None or entry["created_at"] > best["created_at"]:
            newest[group] = entry
    keep = {entry["id"] for entry in newest.values()}
    return sorted(
        entry["id"]
        for entry in entries
        if entry["key"].startswith("v0-rust-")
        and HASHES.search(entry["key"])
        and entry["id"] not in keep
    )


def gh(*args):
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    repo, dry = argv[1], "--dry-run" in argv[2:]
    entries = []
    page = 1
    while True:
        batch = json.loads(gh("api", f"repos/{repo}/actions/caches?per_page=100&page={page}"))["actions_caches"]
        entries += batch
        if len(batch) < 100:
            break
        page += 1
    drop = stale(entries)
    by_id = {entry["id"]: entry for entry in entries}
    freed = 0
    for cache in drop:
        entry = by_id[cache]
        freed += entry["size_in_bytes"]
        print(("would delete " if dry else "delete ") + f"{entry['key']} ({entry['ref']}, {entry['size_in_bytes'] // 1_000_000} MB)")
        if not dry:
            gh("api", "--method", "DELETE", f"repos/{repo}/actions/caches/{cache}")
    print(f"{len(entries)} caches; {len(drop)} stale, {freed // 1_000_000} MB {'to free' if dry else 'freed'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

#!/usr/bin/env bash
# Before the cache of an Xtensa firmware job is saved (.github/actions/firmware-cache):
# removes from shells/xteink/target what the next run builds again in any case
# (PaperPaw's own crates: cargo takes a fresh checkout's files for newer than
# their artefacts), the linked images and the incremental state, and keeps the
# dependencies and the core libraries of -Zbuild-std.
#
#   tools/ci/prune-firmware-target.sh [<target-dir>]      (default shells/xteink/target)
set -euo pipefail
target=${1:-shells/xteink/target}
[[ -d "$target" ]] || exit 0
shopt -s nullglob
for profile in "$target"/release "$target"/*/release; do
  [[ -d "$profile/deps" ]] || continue
  find "$profile" -maxdepth 1 -type f -delete
  rm -rf "$profile/incremental" "$profile/examples"
  for dir in deps .fingerprint build; do
    [[ -d "$profile/$dir" ]] || continue
    find "$profile/$dir" -maxdepth 1 \( -name 'paperpaw*' -o -name 'libpaperpaw*' \) -exec rm -rf {} +
  done
done

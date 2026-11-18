#!/usr/bin/env bash
# Host checks of the direct update and the rollback between the released betas
# and this tree (#229). No device, no flash, no network, nothing signed.
#
#   tools/qualify-update-paths.sh [-t TAG]... [--work DIR]
#
# 1. forward: every card format fixture of the beta tags reads as before
#    (tools/qualify-card-formats.py), and this tree's kernel starts on the
#    beta card and keeps it (`cargo test -p paperpaw-core --test beta_upgrade`);
#    that test also writes the card as Beta 4 leaves it into DIR/card. The
#    stats plugin moves each tag's own `webdav.ini` password into the secrets
#    (`plugins/stats/tests/webdav_migration.rs`, #323).
# 2. rollback: for each TAG (default v0.6.0-beta.1 .. 3) a worktree of the tag
#    in DIR/<tag> (made once, never removed here) runs
#    tools/rollback/older_beta_reads_beta4_card.rs on that card with THAT tag's code.
#
# Results: docs/testing/update-rollback.md. What only a device shows:
# tools/pps/update-rollback/README.md.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
work=${PAPERPAW_QUALIFY_DIR:-$HOME/qualify-update-paths}
tags=()
while [ $# -gt 0 ]; do
  case $1 in
    -t) tags+=("$2"); shift 2 ;;
    --work) work=$2; shift 2 ;;
    *) echo "usage: $0 [-t TAG]... [--work DIR]" >&2; exit 2 ;;
  esac
done
[ ${#tags[@]} -gt 0 ] || tags=(v0.6.0-beta.1 v0.6.0-beta.2 v0.6.0-beta.3)
export CARGO_INCREMENTAL=${CARGO_INCREMENTAL:-0}
mkdir -p "$work"
card=$work/card
[ ! -e "$card" ] || { echo "$card exists: remove it or pass --work with a new folder" >&2; exit 2; }

cd "$root"
python3 -I tools/qualify-card-formats.py "${tags[@]/#/--tag=}"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$work/target-current} PAPERPAW_ROLLBACK_CARD_OUT=$card \
  cargo test --locked -p paperpaw-core --test beta_upgrade

for tag in "${tags[@]}"; do
  fixture=$work/webdav.ini.$tag
  git show "$tag:plugins/stats/testdata/webdav.ini.v1" > "$fixture"
  echo "== forward: $tag's stats webdav.ini password"
  CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$work/target-current} PAPERPAW_WEBDAV_FIXTURE=$fixture \
    cargo test --locked -p paperpaw-plugin-stats --features linked --test webdav_migration
done

failed=0
for tag in "${tags[@]}"; do
  wt=$work/$tag
  [ -d "$wt" ] || git worktree add --detach "$wt" "$tag"
  cp tools/rollback/older_beta_reads_beta4_card.rs "$wt/crates/paperpaw-core/tests/rollback_card.rs"
  echo "== rollback: $tag's code on the Beta 4 card"
  (cd "$wt" && CARGO_TARGET_DIR=$work/target-$tag PAPERPAW_ROLLBACK_CARD=$card \
    cargo test --locked -p paperpaw-core --test rollback_card -- --nocapture) || failed=1
done
exit $failed

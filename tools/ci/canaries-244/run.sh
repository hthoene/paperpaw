#!/usr/bin/env bash
# The canary runner of #244 (recipe Q3): one-line breakages of the kernel, the tests
# that catch each (the failing names go to <id>.fails), a failing build is an ERROR.
#
#   tools/ci/canaries-244/run.sh [--base REF] [--keep] [--check] [canaries.py options]
#
# Works in a worktree and a target folder of its own (never the tree you are in):
# $CANARY_DIR (default: ../canary-worktree-<pid> beside the repository). Heavy: start
# it with start_job and ~/bin/heavy-slot.sh. See canaries.py --help.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
base="HEAD"; keep=0; mode="run"; pass=()
while [ $# -gt 0 ]; do
  case "$1" in
    --base) base="$2"; shift 2 ;;
    --keep) keep=1; shift ;;
    --check) mode="check"; shift ;;
    --apply-check) mode="apply-check"; shift ;;
    *) pass+=("$1"); shift ;;
  esac
done
tree="${CANARY_DIR:-$repo/../canary-worktree-$$}"
git -C "$repo" worktree add -q --detach "$tree" "$base"
cleanup() { [ "$keep" -eq 1 ] || git -C "$repo" worktree remove --force "$tree" >/dev/null 2>&1 || true; }
trap cleanup EXIT
echo "canary worktree: $tree ($(git -C "$tree" rev-parse HEAD))"
python3 "$here/canaries.py" "$mode" --tree "$tree" --out "$here" --file "$here/canaries.txt" ${pass[@]+"${pass[@]}"}

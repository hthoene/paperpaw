#!/usr/bin/env bash
# CI job update-path: this build's installer, started from the release
# before's image (ci.yml explains what it does and does not prove).
#
#   tools/ci/update-path.sh <images-dir> <work-dir>
#
# <images-dir> holds install-image-<device>/install.bin for each device the
# firmware job built. Needs `target/release/paperpaw-sign` and the release
# list of the repository (`gh`). Fails when the X4's update is not in the
# release before, and when no device was tested at all: a job that tested
# nothing must not be green. The other devices are tested where the release
# before has their update (the X3, X4 Pro and X4 Classic came after the
# first releases) and warned about where it has not.
#
# For tests of this script (tools/test_ci_tools.py) these can be replaced:
#   GH=<command>             instead of gh
#   SIGN=<command>           instead of target/release/paperpaw-sign
#   UPDATE_PATH_TEST=<cmd>   instead of cargo test (run once for each device)
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 <images-dir> <work-dir>" >&2; exit 2; }
images=$1
dir=$2
gh_cmd=${GH:-gh}
sign=${SIGN:-target/release/paperpaw-sign}
run_test=${UPDATE_PATH_TEST:-cargo test -q --release -p paperpaw-x4 --test update_path}
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

mkdir -p "$dir"
version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")"
# The newest release of PaperPaw older than this version.
before="$("$gh_cmd" release list -R "${GITHUB_REPOSITORY:-hthoene/paperpaw}" --exclude-drafts --limit 100 \
  --json tagName --jq '.[].tagName' | python3 -c '
import sys
sys.path.insert(0, sys.argv[2])
from versions import semver
now = semver(sys.argv[1])
tags = [t.strip() for t in sys.stdin if t[:1] == "v" and t[1:2].isdigit()]
older = [t for t in tags if semver(t[1:]) < now]
print(max(older, key=lambda t: semver(t[1:])) if older else "")
' "$version" "$root/tools/plugin-files")"
if [[ -z "$before" ]]; then
  echo "::error::no release of PaperPaw before $version"
  exit 1
fi
"$sign" key "$dir/run.key" > "$dir/run.txt"
key="$(awk '/^public key:/ { print $3 }' "$dir/run.txt")"
tested=0
for device in x4 x3 x4-pro x4-classic; do
  old="paperpaw-$device-${before#v}-update.bin"
  if ! "$gh_cmd" release download "$before" -R "${GITHUB_REPOSITORY:-hthoene/paperpaw}" -D "$dir" --clobber \
      -p "$old" -p "$old.sha256"; then
    if [[ "$device" == x4 ]]; then
      # The device every release has had: a missing file is a changed
      # name or the wrong release, and the gate would stop gating.
      echo "::error::$before has no update for the x4 ($old): the update path is not tested"
      exit 1
    fi
    echo "::warning::$before has no update for the $device: its path is not tested"
    continue
  fi
  (cd "$dir" && sha256sum -c "$old.sha256")
  "$sign" sign "$images/install-image-$device/install.bin" "$dir/$device-new.bin" --key "$dir/run.key"
  echo "::group::$device: $before to this build"
  PAPERPAW_UPDATE_DEVICE="$device" PAPERPAW_UPDATE_OLD="$dir/$old" \
    PAPERPAW_UPDATE_NEW="$dir/$device-new.bin" PAPERPAW_UPDATE_KEY="$key" \
    $run_test
  echo "::endgroup::"
  tested=$((tested + 1))
done
if ((tested == 0)); then
  echo "::error::no device was tested"
  exit 1
fi
echo "update path tested for $tested device(s) from $before"

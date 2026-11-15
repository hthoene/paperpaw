#!/usr/bin/env bash
# Every plugin's library and the UI kit, each built alone: `cargo check -p <crate> --lib`, one
# crate a call, so that nothing else in the build turns the SDK's `kernel` feature on. With it
# off the old plugin API (`Plugin`, `Registrar`, `Screen`, `Env`, `Cx`, `Command`, ..) is not
# public (#244): a library that names it fails here. The workspace's own build proves nothing
# of this, because there the kernel turns the feature on for everyone.
#
# Feature sets, each without `kernel`: a crate's defaults, and one build each with `linked`
# (what the firmware and the app turn on), `native-entry` (the plugin file's build) and every
# further feature of a plugin that one of those builds turns on.
#
#   tools/ci/plugin-libs-alone.sh            # all of them
#   tools/ci/plugin-libs-alone.sh <crate>..  # these only
#
# Two plugins name `kernel` themselves and are listed, not skipped: `fonts-default` (the table
# of fonts the kernel registers) and `store` (it installs plugins).
set -o pipefail
cd "$(dirname "$0")/../.." || exit 2
features="linked native-entry packs outline embedded-quotes embedded-page x4-gate-closed"
sets="$(cargo metadata --format-version 1 --no-deps | FEATURES="$features" ONLY="$*" python3 -I -c '
import json, os, sys
want, only = os.environ["FEATURES"].split(), os.environ["ONLY"].split()
for package in json.load(sys.stdin)["packages"]:
    manifest, name = package["manifest_path"], package["name"]
    if "/plugins/" not in manifest and name != "paperpaw-ui-kit":
        continue
    if "/plugins/test-support/" in manifest or (only and name not in only):
        continue
    if not any("lib" in target["kind"] or "rlib" in target["kind"] for target in package["targets"]):
        continue
    print(name, "-")
    for feature in want:
        if feature in package["features"]:
            print(name, feature)
')" || exit 2
[[ -n "$sets" ]] || { echo "plugin-libs-alone: no crate to check" >&2; exit 2; }
log="$(mktemp)"
checked=0 red=0
while read -r crate feature; do
    checked=$((checked + 1))
    args=()
    [[ "$feature" == - ]] || args=(--features "$feature")
    if ! cargo check -q -p "$crate" --lib "${args[@]}" > "$log" 2>&1; then
        red=$((red + 1))
        echo "FAILED: $crate (${feature/#-/defaults})"
        grep -B2 -A12 -E "^error" "$log" | head -60
    fi
done <<< "$sets"
rm -f "$log"
echo "plugin-libs-alone: $checked builds, $red failed"
[[ $red -eq 0 ]]

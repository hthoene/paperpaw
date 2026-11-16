#!/usr/bin/env bash
# Every plugin's library and the UI kit, each built alone: `cargo check -p <crate> --lib`, one
# crate a call, so that nothing else in the build turns the SDK's `kernel` feature on. With it
# off the old plugin API (`Plugin`, `Registrar`, `Screen`, `Env`, `Cx`, `Command`, ..) is not
# public (#244): a library that names it fails here. The workspace's own build proves nothing
# of this, because there the kernel turns the feature on for everyone.
#
# Feature sets: a crate's defaults, and one build for each feature it declares (`cargo metadata`
# says which: `linked`, what the firmware and the app turn on, `native-entry`, the plugin
# file's build, and whatever a plugin adds).
#
# A build proves something only where the SDK is built without `kernel`, so each one is asked
# first (`cargo tree -e normal,features -i paperpaw-sdk`). The crates below turn `kernel` on
# themselves and are built all the same; any other crate whose build has it on fails, also one
# that only depends on one of them.
#
#   tools/ci/plugin-libs-alone.sh            # all of them
#   tools/ci/plugin-libs-alone.sh <crate>..  # these only
set -o pipefail
cd "$(dirname "$0")/../.." || exit 2
# `fonts-default`: the table of fonts the kernel registers. `store`: it installs plugins.
with_kernel=" paperpaw-plugin-fonts-default paperpaw-plugin-store "
sets="$(cargo metadata --format-version 1 --no-deps | ONLY="$*" python3 -I -c '
import json, os, sys
only = os.environ["ONLY"].split()
for package in json.load(sys.stdin)["packages"]:
    manifest, name = package["manifest_path"], package["name"]
    if "/plugins/" not in manifest and name != "paperpaw-ui-kit":
        continue
    if "/plugins/test-support/" in manifest or (only and name not in only):
        continue
    if not any("lib" in target["kind"] or "rlib" in target["kind"] for target in package["targets"]):
        continue
    print(name, "-")
    for feature in sorted(package["features"]):
        if feature != "default":
            print(name, feature)
')" || exit 2
[[ -n "$sets" ]] || { echo "plugin-libs-alone: no crate to check" >&2; exit 2; }
log="$(mktemp)"
checked=0 red=0
while read -r crate feature; do
    checked=$((checked + 1))
    args=()
    [[ "$feature" == - ]] || args=(--features "$feature")
    if ! cargo tree -q -p "$crate" "${args[@]}" -e normal,features -i paperpaw-sdk > "$log" 2>&1; then
        red=$((red + 1))
        echo "FAILED: $crate (${feature/#-/defaults}): cargo tree could not say what the SDK is built with"
        head -20 "$log"
        continue
    fi
    if grep -q 'paperpaw-sdk feature "kernel"' "$log" && [[ "$with_kernel" != *" $crate "* ]]; then
        red=$((red + 1))
        echo "FAILED: $crate (${feature/#-/defaults}): the SDK is built with \`kernel\` here, so the build proves nothing. What asks for it:"
        grep -A3 'paperpaw-sdk feature "kernel"' "$log" | head -12
        continue
    fi
    if ! cargo check -q -p "$crate" --lib "${args[@]}" > "$log" 2>&1; then
        red=$((red + 1))
        echo "FAILED: $crate (${feature/#-/defaults})"
        grep -B2 -A12 -E "^error" "$log" | head -60
    fi
done <<< "$sets"
rm -f "$log"
echo "plugin-libs-alone: $checked builds, $red failed"
[[ $red -eq 0 ]]

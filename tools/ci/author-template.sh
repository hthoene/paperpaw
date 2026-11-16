#!/usr/bin/env bash
# What `paperpaw-dev new --github <id> --code` writes is a plugin an author
# can build: made against this checkout's SDK, then built for the device
# and packed by `paperpaw-dev build`, as an author does it (#244). Interface
# 7 still changes in place, so the template can stop compiling without this.
# `x4-pro` needs espup's compiler and its linker on the path.
#
#   tools/ci/author-template.sh <empty work folder> [x4|x4-pro]
set -euo pipefail
work="${1:?usage: author-template.sh <work folder> [x4|x4-pro]}"
device="${2:-x4}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
plugin="$work/night-owl"
rm -rf "$plugin"
mkdir -p "$work"
cd "$root"
# The tool's table of the interface is current.
python3 tools/abi-metadata-table.py --check
cargo build -q --release -p paperpaw-sign
cargo build -q -p paperpaw-dev
dev="${CARGO_TARGET_DIR:-$root/target}/debug/paperpaw-dev"
sign="${CARGO_TARGET_DIR:-$root/target}/release/paperpaw-sign"
"$dev" new --github night-owl --code --sdk crates/paperpaw-sdk --dir "$plugin"
# In the plugin's own target folder, with its own configuration (the command
# itself leaves the build's flags out of it).
env -u CARGO_TARGET_DIR "$dev" build "$plugin" --device "$device" --sign-tool "$sign"
test -s "$plugin/dist/$device/night-owl/code.ppn"
test -s "$plugin/dist/$device/night-owl/plugin.ini"

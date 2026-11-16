#!/usr/bin/env bash
# What `paperpaw-dev new --github <id> --code` writes is a plugin an author
# can build: made against this checkout's SDK, then built for the X4 and
# packed by `paperpaw-dev build`, and built for the emulator, as an author
# does it (#244). Interface 7 still changes in place, so the template can
# stop compiling without this.
# The X4 Pro's build needs espup's compiler and is not run here.
#
#   tools/ci/author-template.sh <empty work folder>
set -euo pipefail
work="${1:?usage: author-template.sh <work folder>}"
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
# In the plugin's own target folder, with its own configuration.
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR \
    "$dev" build "$plugin" --sign-tool "$sign"
test -s "$plugin/dist/x4/night-owl/code.ppn"
test -s "$plugin/dist/x4/night-owl/plugin.ini"
# And for the emulator: the same program as the library it loads.
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR \
    "$dev" build "$plugin" --device emulator
test -s "$plugin/dist/emulator/plugins/night-owl/code-host.so"

#!/bin/bash
# Edge light (#104) on the emulator with the X4 Pro's profile (touch, a front light with warmth), from a
# fresh card:
#   tools/pps/edge-light/run-emulator.sh EMU_BINARY PAPERPAW_DEV OUT
# EMU_BINARY: paperpaw-emu; PAPERPAW_DEV: paperpaw-dev (both built from this checkout, `cargo build -p
# paperpaw-emulator -p paperpaw-dev`). The plugin is built for the emulator the way an author builds
# one (`paperpaw-dev new --code`, then `paperpaw-dev build --device emulator`: the library has the heap
# word the loader fills), from a project that only names this checkout's plugin crate.
set -e
EMU=$(realpath "$1"); DEV=$(realpath "$2"); OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
WORK=$(mktemp -d); CARD=$(mktemp -d)
trap 'rm -rf "$WORK" "$CARD"' EXIT
"$DEV" new --github edge-light --code --sdk "$ROOT/crates/paperpaw-sdk" --dir "$WORK/project" > /dev/null
sed -i "s|^paperpaw-sdk = .*|&\npaperpaw-plugin-edge-light = { path = \"$ROOT/plugins/edge-light\", features = [\"native-entry\"] }|" "$WORK/project/Cargo.toml"
printf '#![no_std]\n#![no_main]\nuse paperpaw_plugin_edge_light as _;\n' > "$WORK/project/src/main.rs"
(cd "$WORK/project" && env -u CARGO_TARGET_DIR "$DEV" build . --device emulator > "$WORK/build.log" 2>&1) \
  || { cat "$WORK/build.log"; exit 2; }
mkdir -p "$CARD/plugins/edge-light" "$CARD/books" "$CARD/.paperpaw/plugins/setup"
# The first-run setup guards the device (no filter runs under it): its done mark is there.
: > "$CARD/.paperpaw/plugins/setup/done"
cp "$WORK/project/dist/emulator/plugins/edge-light/code-host.so" "$CARD/plugins/edge-light/code-host.so"
cp -r "$ROOT/plugins/edge-light/i18n" "$ROOT/plugins/edge-light/how" "$CARD/plugins/edge-light/"
cat > "$CARD/plugins/edge-light/plugin.ini" <<INI
[plugin]
id = edge-light
name = Edge light
description = Swipe along the screen edge to set the front light.
version = 1.0.0
INI
python3 -I "$ROOT/tools/x4-remote.py" --emulator "$EMU" --library "$CARD" --device-profile xteink-x4-pro \
  --emu-arg=--trust-code --script-file "$HERE/../edge-light.pps" --out "$OUT"

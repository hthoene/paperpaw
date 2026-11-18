#!/bin/bash
# File sync (#134) on the emulator, from a fresh card, with a fake WebDAV server (account demo / secret).
#   tools/pps/file-sync/run-emulator.sh EMU_BINARY PAPERPAW_DEV OUT
# EMU_BINARY: paperpaw-emu; PAPERPAW_DEV: paperpaw-dev (both built from this checkout, `cargo build -p
# paperpaw-emulator -p paperpaw-dev`). The plugin is built for the emulator the way an author builds
# one (`paperpaw-dev new --code`, then `paperpaw-dev build --device emulator`: the library has the heap
# word the loader fills), from a project that only names this checkout's plugin crate.
# The card gets the plugin (`--trust-code` installs it) and a folder /Notes with two notes; the server
# has the folder /dav/notes/ with one note of its own. Both folders are checked after the script.
set -e
EMU=$(realpath "$1"); DEV=$(realpath "$2"); OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
WORK=$(mktemp -d); CARD=$(mktemp -d); SERVER=$(mktemp -d); LOG=$(mktemp)
trap 'kill $PID 2>/dev/null; rm -rf "$WORK" "$CARD" "$SERVER" "$LOG"' EXIT
"$DEV" new --github file-sync --code --sdk "$ROOT/crates/paperpaw-sdk" --dir "$WORK/project" > /dev/null
sed -i "s|^paperpaw-sdk = .*|&\npaperpaw-plugin-file-sync = { path = \"$ROOT/plugins/file-sync\", features = [\"native-entry\"] }|" "$WORK/project/Cargo.toml"
printf '#![no_std]\n#![no_main]\nuse paperpaw_plugin_file_sync as _;\n' > "$WORK/project/src/main.rs"
(cd "$WORK/project" && env -u CARGO_TARGET_DIR "$DEV" build . --device emulator > "$WORK/build.log" 2>&1) \
  || { cat "$WORK/build.log"; exit 2; }
mkdir -p "$CARD/plugins/file-sync" "$CARD/Notes/projects" "$CARD/books" "$SERVER/projects"
cp "$WORK/project/dist/emulator/plugins/file-sync/code-host.so" "$CARD/plugins/file-sync/code-host.so"
cp -r "$ROOT/plugins/file-sync/i18n" "$ROOT/plugins/file-sync/how" "$CARD/plugins/file-sync/"
cat > "$CARD/plugins/file-sync/plugin.ini" <<INI
[plugin]
id = file-sync
name = File sync
description = Keeps folders of the card in sync with your own server.
version = 1.0.0
INI
printf 'card one\n' > "$CARD/Notes/one.md"
printf 'card two\n' > "$CARD/Notes/two.md"
printf 'card project\n' > "$CARD/Notes/projects/plan.md"
printf 'server only\n' > "$SERVER/three.md"
python3 -I "$ROOT/plugins/file-sync/testdata/fake_webdav.py" "$SERVER" --user demo --password secret > "$LOG" 2>&1 &
PID=$!
for _ in $(seq 50); do grep -q '^PORT' "$LOG" && break; sleep 0.1; done
PORT=$(awk '/^PORT/ {print $2}' "$LOG")
rc=0
python3 -I "$ROOT/tools/x4-remote.py" --emulator "$EMU" --library "$CARD" \
  --emu-arg=--trust-code --emu-arg=--wifi --emu-arg=online --var port="$PORT" \
  --script-file "$HERE/../file-sync.pps" --out "$OUT" || rc=$?
cp "$LOG" "$OUT/fake-webdav.log" 2>/dev/null || true
# What the run left: the card's folder and the server's.
(cd "$CARD/Notes" && find . -type f | sort) > "$OUT/card-files.txt"
(cd "$SERVER" && find . -type f | sort) > "$OUT/server-files.txt"
exit $rc

#!/bin/bash
# Picture frame (#138) on the emulator, from a fresh card, with a fake WebDAV photo server.
#   tools/pps/picture-frame/run-emulator.sh EMU_BINARY PAPERPAW_DEV OUT
# EMU_BINARY: paperpaw-emu; PAPERPAW_DEV: paperpaw-dev (both built from this checkout, `cargo build -p
# paperpaw-emulator -p paperpaw-dev`). The plugin is built for the emulator the way an author builds
# one (`paperpaw-dev new --code`, then `paperpaw-dev build --device emulator`: the library has the heap
# word the loader fills), from a project that only names this checkout's plugin crate.
# The card gets the plugin (plugins/picture-frame, `--trust-code` installs it), three photos in /photos and
# the same three on the fake server (the server's account is demo / secret).
set -e
EMU=$(realpath "$1"); DEV=$(realpath "$2"); OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
WORK=$(mktemp -d); CARD=$(mktemp -d); SERVER=$(mktemp -d); LOG=$(mktemp)
trap 'kill $PID 2>/dev/null; rm -rf "$WORK" "$CARD" "$SERVER" "$LOG"' EXIT
"$DEV" new --github picture-frame --code --sdk "$ROOT/crates/paperpaw-sdk" --dir "$WORK/project" > /dev/null
sed -i "s|^paperpaw-sdk = .*|&\npaperpaw-plugin-picture-frame = { path = \"$ROOT/plugins/picture-frame\", features = [\"native-entry\"] }|" "$WORK/project/Cargo.toml"
printf '#![no_std]\n#![no_main]\nuse paperpaw_plugin_picture_frame as _;\n' > "$WORK/project/src/main.rs"
(cd "$WORK/project" && env -u CARGO_TARGET_DIR "$DEV" build . --device emulator > "$WORK/build.log" 2>&1) \
  || { cat "$WORK/build.log"; exit 2; }
mkdir -p "$CARD/plugins/picture-frame" "$CARD/photos" "$CARD/books"
cp "$WORK/project/dist/emulator/plugins/picture-frame/code-host.so" "$CARD/plugins/picture-frame/code-host.so"
cp -r "$ROOT/plugins/picture-frame/i18n" "$ROOT/plugins/picture-frame/how" "$CARD/plugins/picture-frame/"
cat > "$CARD/plugins/picture-frame/plugin.ini" <<INI
[plugin]
id = picture-frame
name = Picture frame
description = Your own photos as the sleep screen, as a slideshow and as the photo of the day.
version = 1.0.0
INI
cp "$ROOT/shells/desktop/tests/fixtures/cover.jpg" "$CARD/photos/a-cover.jpg"
cp "$ROOT/plugins/picture-home/tests/data/wide.jpg" "$CARD/photos/b-wide.jpg"
cp "$ROOT/crates/paperpaw-image/tests/data/kafka-title.jpg" "$CARD/photos/c-kafka.jpg"
cp "$ROOT/crates/paperpaw-image/tests/data/alice-cover.jpg" "$SERVER/s1-alice.jpg"
cp "$ROOT/plugins/picture-home/tests/data/wide.jpg" "$SERVER/s2-wide.jpg"
cp "$ROOT/shells/desktop/tests/fixtures/cover.jpg" "$SERVER/s3-cover.jpg"
python3 -I "$ROOT/plugins/picture-frame/testdata/fake_webdav.py" "$SERVER" --user demo --password secret > "$LOG" 2>&1 &
PID=$!
for _ in $(seq 50); do grep -q '^PORT' "$LOG" && break; sleep 0.1; done
PORT=$(awk '/^PORT/ {print $2}' "$LOG")
rc=0
python3 -I "$ROOT/tools/x4-remote.py" --emulator "$EMU" --library "$CARD" \
  --emu-arg=--trust-code --emu-arg=--wifi --emu-arg=online --var port="$PORT" \
  --script-file "$HERE/picture-frame.pps" --out "$OUT" || rc=$?
cp "$LOG" "$OUT/fake-webdav.log" 2>/dev/null || true
exit $rc

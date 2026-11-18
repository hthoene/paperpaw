#!/bin/bash
# Smart home (#135) on the emulator, from a fresh card, with a fake Home Assistant server.
#   tools/pps/smart-home/run-emulator.sh EMU_BINARY OUT
# EMU_BINARY: paperpaw-emu built from this checkout (`cargo build -p paperpaw-emulator --bin paperpaw-emu`),
# where the plugin is linked and ships off (the script switches it on). The fake server
# (plugins/smart-home/testdata/fake_ha.py) has the entities of a small home and 600 filler sensors, so the
# list of states is long; its token is test-token-0123456789.
set -e
EMU=$(realpath "$1"); OUT=$2
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
CARD=$(mktemp -d); LOG=$(mktemp)
trap 'kill $PID 2>/dev/null; rm -rf "$CARD" "$LOG"' EXIT
mkdir -p "$CARD/books"
python3 -I "$ROOT/plugins/smart-home/testdata/fake_ha.py" --token test-token-0123456789 --filler 600 > "$LOG" 2>&1 &
PID=$!
for _ in $(seq 50); do grep -q '^PORT' "$LOG" && break; sleep 0.1; done
PORT=$(awk '/^PORT/ {print $2}' "$LOG")
rc=0
"${PYTHON:-python3}" -I "$ROOT/tools/x4-remote.py" --emulator "$EMU" --library "$CARD" \
  --emu-arg=--wifi --emu-arg=online --var port="$PORT" \
  --script-file "$HERE/smart-home.pps" --out "$OUT" || rc=$?
cp "$LOG" "$OUT/fake-ha.log" 2>/dev/null || true
exit $rc

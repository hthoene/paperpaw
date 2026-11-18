#!/bin/bash
# The Beta 4 suite on the emulator, from a fresh copy of the card.
#   tools/pps/beta4/run-emulator.sh EMU_BINARY CARD_TEMPLATE OUT [more x4-remote.py arguments, e.g. --baseline results.json]
# CARD_TEMPLATE: a library folder (books/, paperpaw.ini ...); the run works on a copy, the template is not changed.
# The radio starts online and a fake AnkiConnect (plugins/anki/testdata/fake_anki_connect.py) listens on a free port of
# this computer for suite 14 (--var ankiport=PORT; --var ankidead=1 is a port nobody listens on).
set -e
EMU=$1; TEMPLATE=$2; OUT=$3; shift 3
HERE=$(cd "$(dirname "$0")" && pwd)
PYTHON=${PYTHON:-python3}
CARD=$(mktemp -d)
cp -a "$TEMPLATE/." "$CARD/"
PORT=$("$PYTHON" -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
"$PYTHON" -I "$HERE/../../../plugins/anki/testdata/fake_anki_connect.py" --port "$PORT" --bind 127.0.0.1 >/dev/null &
FAKE=$!
trap 'kill $FAKE 2>/dev/null; rm -rf "$CARD"' EXIT
set +e
"$PYTHON" -I "$HERE/../../x4-remote.py" --emulator "$EMU" --library "$CARD" --emu-arg=--wifi --emu-arg=online \
  --var ankiport="$PORT" --var ankidead=1 --script-file "$HERE/beta4.pps" --out "$OUT" "$@"
exit $?

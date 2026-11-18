#!/bin/bash
# The Beta 4 suite on the emulator, from a fresh copy of the card.
#   tools/pps/beta4/run-emulator.sh EMU_BINARY CARD_TEMPLATE OUT [more x4-remote.py arguments, e.g. --baseline results.json]
# CARD_TEMPLATE: a library folder (books/, paperpaw.ini ...); the run works on a copy, the template is not changed.
set -e
EMU=$1; TEMPLATE=$2; OUT=$3; shift 3
HERE=$(cd "$(dirname "$0")" && pwd)
CARD=$(mktemp -d)
cp -a "$TEMPLATE/." "$CARD/"
"${PYTHON:-python3}" -I "$HERE/../../x4-remote.py" --emulator "$EMU" --library "$CARD" \
  --script-file "$HERE/beta4.pps" --out "$OUT" "$@"
rc=$?
rm -rf "$CARD"
exit $rc

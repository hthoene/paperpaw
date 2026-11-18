#!/bin/bash
# Mail (#81) on the emulator, from a fresh card, against a fake IMAP + SMTP server over TLS with a
# self-signed certificate that the account pins (the plugin trusts nothing else).
#   tools/pps/mail/run-emulator.sh EMU_BINARY OUT [--starttls] [more x4-remote.py arguments]
# EMU_BINARY: paperpaw-emu from this checkout (`cargo build -p paperpaw-emulator --bin paperpaw-emu`), where the
# plugin is linked and ships off (the script switches it on). The fake server (fake_mail.py) holds
# user@example.org / secret-pass-123 with 120 mails; the sent mails are kept in OUT/server/sent.
set -e
EMU=$(realpath "$1"); OUT=$2; shift 2
SECURITY=implicit
if [ "$1" = "--starttls" ]; then SECURITY=starttls; STARTTLS=--starttls; shift; fi
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
mkdir -p "$OUT"
CARD=$(mktemp -d); LOG=$OUT/fake-mail.log
trap 'kill $PID 2>/dev/null; rm -rf "$CARD"' EXIT
mkdir -p "$CARD/books"
# The password is a secret setting: a value in paperpaw.ini moves into the secrets at load (docs/abi/secrets.md 4).
printf '[mail]\n1.password = secret-pass-123\n' > "$CARD/paperpaw.ini"
python3 -I "$HERE/fake_mail.py" --out "$OUT/server" $STARTTLS > "$LOG" 2>&1 &
PID=$!
for _ in $(seq 100); do grep -q '^READY' "$LOG" && break; sleep 0.1; done
rc=0
"${PYTHON:-python3}" -I "$ROOT/tools/x4-remote.py" --emulator "$EMU" --library "$CARD" \
  --emu-arg=--wifi --emu-arg=online --emu-arg=--trust-code \
  --var imap="$(awk '/^IMAP/ {print $2}' "$LOG")" --var smtp="$(awk '/^SMTP/ {print $2}' "$LOG")" \
  --var pin="$(awk '/^PIN/ {print $2}' "$LOG")" --var security="$SECURITY" --var card="$CARD" \
  --script-file "$HERE/mail.pps" --out "$OUT" "$@" || rc=$?
exit $rc

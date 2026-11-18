#!/bin/bash
# The soak runner (#230). Source of truth: tools/pps/soak/; ~/dt-jobs/soak/run.sh calls this file in a checkout.
#   run.sh start  NAME --port 'rfc2217://esp-host:4001?ign_set_control' --hours 72 [--books-always 'books/cjk.epub|books/pics.epub'] ...
#   run.sh start  NAME --emulator BIN --library CARDCOPY --minutes 20      (label emulator)
#   run.sh resume NAME [--hours 72]    run.sh stop NAME    run.sh status NAME    run.sh finalize NAME [--validator PATH]
# Long runs: start it with start_job (it keeps running; `stop` ends it cleanly after the round in progress).
PY=${PYTHON:-$HOME/esptool-venv/bin/python}
[ -x "$PY" ] || PY=python3
export PYTHON="$PY"
exec "$PY" -I "$(dirname "$(readlink -f "$0")")/soak.py" "$@"

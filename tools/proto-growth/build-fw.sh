#!/usr/bin/env bash
# build-fw.sh <label> <features> : release firmware build for the ESP32-C3 as CI builds it
. ~/.cargo/env
set -uo pipefail
label=$1; feats=$2
cd /workspaces/paperpaw/firmware-engineer/paperpaw/shells/xteink
( time cargo build --release --target riscv32imc-unknown-none-elf --no-default-features --features "$feats" ) > ~/fw-jobs/proto/logs/$label.log 2>&1 || { echo "$label FAILED"; tail -20 ~/fw-jobs/proto/logs/$label.log; exit 1; }
cp target/riscv32imc-unknown-none-elf/release/paperpaw-xteink ~/fw-jobs/proto/elf/$label.elf
size -A ~/fw-jobs/proto/elf/$label.elf > ~/fw-jobs/proto/elf/$label.size
echo "$label done"

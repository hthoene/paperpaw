#!/usr/bin/env bash
# Real SDK macro regression: build riscv32, pack format 5, then check_file5.
set -euo pipefail
cd "$(dirname "$0")"
sample=$PWD
root=$(cd ../.. && pwd)
packer_root=${PAPERPAW_PACKER_ROOT:-$root}
: "${CARGO_TARGET_DIR:?Set an own CARGO_TARGET_DIR}"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
python3 "$root/tools/abi-metadata.py" --schema "$root/abi/abi.def" --input metadata.json \
    --bytes "$scratch/metadata.bin" --json "$scratch/metadata.json"
cmp "$scratch/metadata.bin" metadata.bin
cmp paperpaw-plugin7.x "$packer_root/crates/paperpaw-native/paperpaw-plugin7.x"
env -u RUSTFLAGS cargo x4 -j4
elf=$CARGO_TARGET_DIR/riscv32imc-unknown-none-elf/release/abi7_sdk_entry
(cd "$packer_root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- pack "$elf" "$scratch/plugin.ppn")
# The plugin command calls format-5 check_file5, including relocated entry checks.
(cd "$packer_root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- plugin "$scratch/plugin.ppn")
# Alignment option (b): the packer itself refuses a misplaced entry. Keep its
# size at 20 bytes; a Rust align(8) wrapper would pad the symbol to 24 bytes.
python3 "$sample/misalign_entry.py" "$elf" "$scratch/misaligned.elf"
if (cd "$packer_root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- \
    pack "$scratch/misaligned.elf" "$scratch/misaligned.ppn") >"$scratch/refusal.txt" 2>&1; then
    echo 'packer accepted an entry deliberately misaligned by four bytes' >&2
    exit 1
fi
rg 'not 8-aligned' "$scratch/refusal.txt"

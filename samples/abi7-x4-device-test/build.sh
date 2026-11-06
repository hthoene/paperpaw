#!/usr/bin/env bash
# Builds the device-test plugin for the X4 (riscv32imc), three variants, packs
# each as format 5 and checks it with the packer's own `plugin` command.
# Output: $OUT (default ./dist): ok.ppn, crash-make.ppn, crash-event.ppn and
# SHA256SUMS. Nothing is signed, flashed or run.
set -euo pipefail
cd "$(dirname "$0")"
sample=$PWD
root=$(cd ../.. && pwd)
: "${CARGO_TARGET_DIR:?Set an own CARGO_TARGET_DIR}"
out=${OUT:-$sample/dist}
mkdir -p "$out"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
python3 "$root/tools/abi-metadata.py" --schema "$root/abi/abi.def" --input metadata.json \
    --bytes "$scratch/metadata.bin" --json "$scratch/metadata.json"
cmp "$scratch/metadata.bin" metadata.bin
cmp paperpaw-plugin7.x "$root/crates/paperpaw-native/paperpaw-plugin7.x"
elf=$CARGO_TARGET_DIR/riscv32imc-unknown-none-elf/release/abi7_x4_device_test
for variant in ok crash-make crash-event; do
    features=()
    [ "$variant" = ok ] || features=(--features "$variant")
    env -u RUSTFLAGS cargo x4 -j4 "${features[@]}"
    (cd "$root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- pack "$elf" "$out/$variant.ppn")
    (cd "$root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- plugin "$out/$variant.ppn")
done
(cd "$out" && sha256sum ok.ppn crash-make.ppn crash-event.ppn | tee SHA256SUMS)

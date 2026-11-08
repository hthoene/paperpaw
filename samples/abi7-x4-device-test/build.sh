#!/usr/bin/env bash
# Builds the device-test plugin for the X4 (riscv32imc), packs it as format 5
# and checks it with the packer's own `plugin` command.
#
#   build.sh                      ok.ppn only (the default)
#   build.sh crash-make           the variant that faults while it is made
#   build.sh ok crash-event ...   any of ok, crash-make, crash-event
#
# A crash variant is built only when it is named: it resets the device.
# Output: $OUT (default ./dist): the .ppn files built and SHA256SUMS.
# Nothing is signed, flashed or run.
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
[ $# -gt 0 ] || set -- ok
for variant in "$@"; do
    case $variant in ok | crash-make | crash-event) ;; *)
        echo "unknown variant '$variant' (ok, crash-make, crash-event)" >&2
        exit 2
        ;;
    esac
done
for variant in "$@"; do
    features=()
    [ "$variant" = ok ] || features=(--features "$variant")
    # `${features[@]+...}`: an empty array is an unset variable under `set -u`
    # on bash before 4.4.
    env -u RUSTFLAGS cargo x4 -j4 ${features[@]+"${features[@]}"}
    (cd "$root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- pack "$elf" "$out/$variant.ppn")
    (cd "$root" && env -u RUSTFLAGS cargo run -q -j4 -p paperpaw-sign -- plugin "$out/$variant.ppn")
done
(cd "$out" && sha256sum "${@/%/.ppn}" | tee SHA256SUMS)

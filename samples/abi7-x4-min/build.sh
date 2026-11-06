#!/usr/bin/env bash
# Builds the smallest interface-7 plugin for the X4, packs it with the real
# packer (`paperpaw-sign pack`, format 5) and keeps or checks the two files
# the tests read:
#
#   crates/paperpaw-native/tests/data/abi7-x4-min-linked.bin   the linked ELF
#   crates/paperpaw-native/tests/data/abi7-x4-min.ppn          the plugin file
#
#   ./build.sh --check     builds again and fails if the file differs (default)
#   ./build.sh --update    writes both files
#
# Needs the Rust of rust-toolchain.toml (with the riscv32imc target) and
# python3, and an own CARGO_TARGET_DIR. Never set RUSTFLAGS: it would replace
# .cargo/config.toml's flags, which link the plugin. The file holds no path
# and no name, so it is the same wherever the sample is built; the ELF is
# kept as the packer's input and is not compared (its symbol names carry a
# hash of the build's directory).
set -euo pipefail
cd "$(dirname "$0")"
sample=$PWD
root=$(cd ../.. && pwd)
data=$root/crates/paperpaw-native/tests/data
mode=${1:---check}
case "$mode" in --check | --update) ;; *) echo "usage: ./build.sh [--check|--update]" >&2; exit 2 ;; esac
: "${CARGO_TARGET_DIR:?Set an own CARGO_TARGET_DIR}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The metadata record is the generator's, not hand-written.
python3 "$root/tools/abi-metadata.py" --schema "$root/abi/abi.def" --input metadata.json \
    --bytes "$tmp/metadata.bin" --json "$tmp/metadata.json"
cmp "$tmp/metadata.bin" metadata.bin || { echo "metadata.bin is not what the generator writes" >&2; exit 1; }
cmp paperpaw-plugin7.x "$root/crates/paperpaw-native/paperpaw-plugin7.x" \
    || { echo "paperpaw-plugin7.x differs from the crate's" >&2; exit 1; }

env -u RUSTFLAGS cargo x4
elf=$CARGO_TARGET_DIR/riscv32imc-unknown-none-elf/release/abi7_x4_min
(cd "$root" && env -u RUSTFLAGS cargo run -q --release -p paperpaw-sign -- pack "$elf" "$tmp/abi7-x4-min.ppn")
if [ "$mode" = --update ]; then
    cp "$elf" "$data/abi7-x4-min-linked.bin"
    cp "$tmp/abi7-x4-min.ppn" "$data/abi7-x4-min.ppn"
    echo "written: $data/abi7-x4-min.ppn"
else
    cmp "$tmp/abi7-x4-min.ppn" "$data/abi7-x4-min.ppn" \
        || { echo "the packed file differs from the committed one: run ./build.sh --update and review" >&2; exit 1; }
    echo "the committed file is what the real packer makes of the sample"
fi

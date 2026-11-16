#!/usr/bin/env bash
# What `paperpaw-dev new --github <id> --code` writes is a plugin an author
# can build: made against this checkout's SDK, built for the X4, packed as a
# format-5 file, and passed by the packer's own check (#244). Interface 7 still
# changes in place, so the template can stop compiling without this.
#
#   tools/ci/author-template.sh <empty work folder>
set -euo pipefail
work="${1:?usage: author-template.sh <work folder>}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
plugin="$work/night-owl"
rm -rf "$plugin"
mkdir -p "$work"
cd "$root"
cargo build -q --release -p paperpaw-sign
cargo run -q -p paperpaw-dev -- new --github night-owl --code \
    --sdk crates/paperpaw-sdk --dir "$plugin"
# The plugin's own configuration builds it (its compiler, its linker script).
(cd "$plugin" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR="$plugin/target" cargo x4)
built="$plugin/target/riscv32imc-unknown-none-elf/release/night-owl"
target/release/paperpaw-sign pack "$built" "$work/night-owl.ppn"
target/release/paperpaw-sign plugin "$work/night-owl.ppn"

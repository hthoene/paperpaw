#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
: "${CARGO_TARGET_DIR:?Set an isolated CARGO_TARGET_DIR for this sample}"
cargo test --lib -j4
cargo clippy --all-targets -j4 -- -D warnings
# With the crate type given on the command line cargo does not list the
# library among its artifacts; it writes release/deps/ under a hashed name.
# Remove old ones, build, and take the one it wrote.
deps="$CARGO_TARGET_DIR/release/deps"
rm -f "$deps"/libpaperpaw_sample_abi7_tick_draw-*.so
cargo rustc --release --lib -j4 -- --crate-type=cdylib -Cpanic=abort -Clink-arg=-nostartfiles
shopt -s nullglob
built=("$deps"/libpaperpaw_sample_abi7_tick_draw-*.so)
if [ "${#built[@]}" -ne 1 ]; then
    echo "expected one cdylib in $deps, found ${#built[@]}" >&2
    exit 1
fi
so="${built[0]}"
test -f "$so" || { echo "missing artifact: $so" >&2; exit 1; }
# A real dlopen needs no symbol from the process: the host table is the only link.
undefined="$(nm -D --undefined-only "$so")"
if [ -n "$undefined" ]; then
    echo "undefined symbols in $so:" >&2
    echo "$undefined" >&2
    exit 1
fi
python3 -I check_artifact.py "$so"
echo "artifact: $so"

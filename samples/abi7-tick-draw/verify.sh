#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
: "${CARGO_TARGET_DIR:?Set an isolated CARGO_TARGET_DIR for this sample}"
cargo test --lib -j4
cargo clippy --all-targets -j4 -- -D warnings
cargo rustc --release --lib -j4 -- --crate-type=cdylib -Cpanic=abort -Clink-arg=-nostartfiles

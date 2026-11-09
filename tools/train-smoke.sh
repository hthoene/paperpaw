#!/bin/bash
# The checks CI runs that went red on a merge train, run locally before the
# train's pull request is opened, so that a train does not go red online.
#
#   tools/train-smoke.sh [<worktree>] [--skip-test]
#
# Per step one line `-> <name> rc=<n>`, the output in <worktree>/target/train-smoke.log;
# the exit code is 1 if any step failed. It waits while the machine's load is
# 21 or more (the team's limit for heavy builds), builds in <worktree>/target
# with the sccache wrapper, and does not touch the network except for crates.
# Not covered here (they need the Xtensa/NDK toolchains): the S3 frame-budget
# scan (tools/check-s3-budget.sh, if the tree has it) and the Android lint.
set -u
wt=$(cd "${1:-.}" && pwd) || exit 2
skip_test=0; [ "${2:-}" = --skip-test ] && skip_test=1
cd "$wt" || exit 2
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export CARGO_TARGET_DIR="$wt/target"
[ -x tools/sccache-wrap.sh ] && export RUSTC_WRAPPER="$wt/tools/sccache-wrap.sh"
mkdir -p target; log=target/train-smoke.log; : > "$log"
for _ in $(seq 1 90); do l=$(cut -d' ' -f1 /proc/loadavg | cut -d. -f1); [ "$l" -lt 21 ] && break; sleep 60; done
fail=0
run() {
  local name=$1; shift
  echo "== $name" >> "$log"
  "$@" >> "$log" 2>&1; local rc=$?
  echo "-> $name rc=$rc"
  [ "$rc" -eq 0 ] || fail=1
}
run fmt cargo fmt --all -- --check
run fmt-shells bash -c 'for f in $(git diff --name-only origin/abi7/integration...HEAD -- "shells/xteink/*.rs" "fuzz/*.rs"); do rustfmt --edition 2024 --check "$f" || exit 1; done'
run clippy cargo clippy -j4 --workspace --all-targets -- -D warnings
run rustdoc env RUSTDOCFLAGS="-D warnings" cargo doc -j4 --workspace --no-deps
run clippy-wasm cargo clippy -j4 --target wasm32-unknown-unknown -p paperpaw-web -- -D warnings
run deny cargo deny check
run tag-tests python3 -I tools/tests/test_abi_tags.py
run stage-codec python3 tools/stage-codec.py plugins/about/stages plugins/about/src/stages.rs --check
[ -x tools/check-s3-budget.sh ] && run s3-budget tools/check-s3-budget.sh "$wt" both
[ "$skip_test" -eq 1 ] || run test-workspace cargo test -j4 --workspace --no-fail-fast
echo "SMOKE_EXIT=$fail"
exit "$fail"

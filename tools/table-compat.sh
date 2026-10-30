#!/usr/bin/env bash
# Old plugins against this tree's firmware bodies (the shared function
# table, #44; the CI job `table-compat`):
#
#   tools/table-compat.sh <baseline-ref>
#
# <baseline-ref> is the commit of the last interface raise (the tag
# `interface-<n>` the raise leaves). A plugin built then calls the
# firmware by index, and an index must keep meaning the same function with
# the same behaviour. From the baseline this builds the test plugin
# (crates/paperpaw-core/tests/native-plugin: the text calls of its
# probe.rs) twice, and loads each into this tree's kernel
# (crates/paperpaw-core/tests/native_plugin.rs):
#
#  - A, with its own copies (as an unshared plugin): its calls must come
#    out as this tree's own bodies give them. A body changed since the
#    baseline fails here: the table entry's family needs a new revision, or
#    the interface a raise (ADR 0026).
#  - B, in host table mode (`--cfg paperpaw_table_host`: the shared
#    functions are stubs that call this tree's table by index, as on a
#    chip): the index, the type and the calling convention of every call
#    must still be the function it was.
#
# Before both, the baseline's list (`shared.list`, every family's lines)
# must be a prefix of this one: entries are only added, at the end (the
# interface test of the SDK checks the same against the blessed copy), and
# every entry added since carries a revision above its family's in the
# baseline. At the same revision, firmware that has the family but not the
# entry would accept a plugin built with it, and call past its table's end
# (tools/shared-table.py --appended-to).
#
# The baseline needs the host table mode itself (the commit that brought
# it, or a later one). Needs rustc, python3 and git; builds in
# $TABLE_COMPAT_DIR (default: a temporary directory).
set -euo pipefail

[[ $# -eq 1 ]] || { echo "usage: $0 <baseline-ref>" >&2; exit 2; }
root="$(git rev-parse --show-toplevel)"
cd "$root"
base="$(git rev-parse --verify "$1^{commit}")"
work="${TABLE_COMPAT_DIR:-$(mktemp -d)}"
tree="$work/baseline"
manifest=crates/paperpaw-core/tests/native-plugin/Cargo.toml
lib="release/libpaperpaw_test_native_plugin.so"

git worktree add --detach "$tree" "$base" > /dev/null
trap 'git worktree remove --force "$tree" > /dev/null' EXIT

list() { sed -e 's/#.*//' "$1" | awk 'NF'; }
echo "baseline ${base:0:12}: the shared list is a prefix of this tree's"
list "$tree/crates/paperpaw-table/shared.list" > "$work/baseline.list"
list crates/paperpaw-table/shared.list > "$work/now.list"
if ! cmp -s "$work/baseline.list" <(head -n "$(wc -l < "$work/baseline.list")" "$work/now.list"); then
    echo "shared.list: the baseline's lines are not the first lines of this tree's" >&2
    diff "$work/baseline.list" "$work/now.list" | head -20 >&2 || true
    exit 1
fi

python3 tools/shared-table.py --appended-to "$tree/crates/paperpaw-table/shared.list"

# The plugin is a library of its own, built apart from this tree; a
# build's -D warnings would stop at the stubs' unused code.
echo "baseline plugin A: its own copies"
(cd "$tree" && env -u RUSTFLAGS CARGO_TARGET_DIR="$work/target-a" \
    cargo build --release --manifest-path "$manifest" -q)
echo "baseline plugin B: host table mode"
(cd "$tree" && CARGO_TARGET_DIR="$work/target-b" \
    RUSTFLAGS="--cfg paperpaw_plugin --cfg paperpaw_table_host" \
    cargo build --release --manifest-path "$manifest" -q)

for mode in a b; do
    echo "this tree's kernel with plugin $mode"
    # The test checks the library is in the mode it is run as.
    tables=0; [[ $mode == b ]] && tables=1
    PAPERPAW_TEST_TABLES=$tables PAPERPAW_TEST_PLUGIN="$work/target-$mode/$lib" \
        cargo test -q -p paperpaw-core --test native_plugin -- --ignored
done
echo "old plugins and this tree's firmware agree"

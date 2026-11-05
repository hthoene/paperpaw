#!/usr/bin/env bash
# Old plugins, new PaperPaw, end to end (ADR 0026, #228; the CI job
# `table-compat` runs it after `tools/table-compat.sh`):
#
#   tools/compat-e2e.sh [baseline-ref]
#
# Plugins built at the last interface raise (`baseline-ref`: the commit
# `crates/paperpaw-table/table-baseline.txt` names, unless one is given)
# must keep running on every PaperPaw of the same interface number, and
# must be refused cleanly, before any of their code runs, on one of
# another. This builds the test plugin (crates/paperpaw-core/tests/
# native-plugin) against the baseline's crates and loads it into copies of
# this tree that were changed on purpose (tools/compat/*.patch):
#
#  - none:             this tree as it is. Runs.
#  - additions:        a field appended to the firmware's table and a new
#                      item in the SDK (additions-1), then another of each
#                      (additions-2: the second after the first). Runs, in
#                      the emulator's loader and in a kernel. With
#                      COMPAT_GATE=1 the interface test must also call
#                      each change only new ("write the files anew",
#                      never "raise"), and pass after the bless, twice.
#  - refuse-layout:    a field inserted into the middle of the firmware's
#                      table. Refused by the emulator's loader as another
#                      build (and the interface test, with COMPAT_GATE=1,
#                      calls for a raise).
#  - refuse-interface: native::INTERFACE raised. Refused likewise.
#
# Needs git, python3, cargo; COMPAT_GATE=1 also the nightly and
# cargo-public-api of tools/interface.py. Builds in $COMPAT_DIR (default:
# a temporary directory).
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"
ref="${1:-}"
if [[ -z "$ref" ]]; then
    ref="$(sed -e 's/#.*//' crates/paperpaw-table/table-baseline.txt | awk 'NF { print $1; exit }')"
fi
if [[ -z "$ref" || "$ref" == none ]]; then
    echo "no baseline: nothing to compare" >&2
    exit 0
fi
base="$(git rev-parse --verify "$ref^{commit}")"
work="${COMPAT_DIR:-$(mktemp -d)}"
gate="${COMPAT_GATE:-0}"
manifest=crates/paperpaw-core/tests/native-plugin/Cargo.toml
case "$(uname -s)" in
    Darwin) lib="release/libpaperpaw_test_native_plugin.dylib" ;;
    MINGW* | MSYS* | CYGWIN*) lib="release/paperpaw_test_native_plugin.dll" ;;
    *) lib="release/libpaperpaw_test_native_plugin.so" ;;
esac
trees=("$work/baseline")
cleanup() {
    for t in "${trees[@]}"; do git worktree remove --force "$t" > /dev/null 2>&1 || true; done
}
trap cleanup EXIT

step() { echo; echo "== $*"; }

# The baseline's plugin, with the test plugin of this tree (its probes call
# only what the baseline has), built by the baseline's compiler.
step "baseline ${base:0:12}: building the plugin"
git worktree add --detach "$work/baseline" "$base" > /dev/null
plugin="$(dirname "$manifest")"
rm -rf "${work:?}/baseline/$plugin"
mkdir -p "$work/baseline/$plugin"
tar -C "$plugin" --exclude=./target -cf - . | tar -C "$work/baseline/$plugin" -xf -
(cd "$work/baseline" && env -u RUSTFLAGS CARGO_TARGET_DIR="$work/target-a" \
    cargo build --release --manifest-path "$manifest" -q)
(cd "$work/baseline" && CARGO_TARGET_DIR="$work/target-b" \
    RUSTFLAGS="--cfg paperpaw_plugin --cfg paperpaw_table_host" \
    cargo build --release --manifest-path "$manifest" -q)
lib_a="$work/target-a/$lib"
lib_b="$work/target-b/$lib"
[[ -f "$lib_a" && -f "$lib_b" ]] || { echo "the baseline's plugin was not built" >&2; exit 1; }
# The emulator loads only libraries of its own compiler (ADR 0026): if the
# baseline's differs, the plugins are refused for that, and nothing here
# would test the interface.
here_rustc="$(rustc -vV | awk '/^release:/ { print $2 }')"
base_rustc="$(cd "$work/baseline" && rustc -vV | awk '/^release:/ { print $2 }')"
if [[ "$here_rustc" != "$base_rustc" ]]; then
    echo "the baseline builds with rustc $base_rustc and this tree with $here_rustc: a new compiler raises the interface (ADR 0026); nothing to compare" >&2
    exit 1
fi

# A copy of this tree (its changes and new files too), to change on purpose.
new_tree() {
    local t="$work/tree-$1"
    git worktree add --detach "$t" HEAD > /dev/null
    trees+=("$t")
    git diff HEAD --binary | git -C "$t" apply --allow-empty -
    git ls-files -o --exclude-standard -z | xargs -0 -r cp --parents -t "$t"
}

target="$work/target-trees"
# cargo test of the tree in $PWD
in_tree() { (cd "$1" && shift && CARGO_TARGET_DIR="$target" "$@"); }

# The interface test of the tree: its output, and whether it passed.
gate_test() {
    in_tree "$1" cargo test -q -p paperpaw-sdk --test interface 2>&1 || return 1
}
regenerate() { in_tree "$1" python3 tools/interface.py > /dev/null; }

# The emulator's loader and a kernel with the plugin built before.
loads() { # tree, expect (runs|refused)
    step "$1: the emulator's loader, expecting: $2"
    in_tree "$1" env PAPERPAW_TEST_PLUGIN="$lib_a" PAPERPAW_EXPECT="$2" \
        cargo test -q -p paperpaw-emulator --no-default-features --lib -- --ignored \
        a_plugin_built_before_runs_or_is_refused_cleanly
    if [[ "$2" == runs ]]; then
        step "$1: a kernel, its own copies and table mode"
        in_tree "$1" env PAPERPAW_TEST_TABLES=0 PAPERPAW_TEST_PLUGIN="$lib_a" \
            cargo test -q -p paperpaw-core --test native_plugin -- --ignored
        in_tree "$1" env PAPERPAW_TEST_TABLES=1 PAPERPAW_TEST_PLUGIN="$lib_b" \
            cargo test -q -p paperpaw-core --test native_plugin -- --ignored
    fi
}

# What the gate says about a tree, when COMPAT_GATE=1.
expect_gate() { # tree, text the failing test must say, text it must not
    local out
    if out="$(gate_test "$1")"; then
        echo "the interface test passed on a tree that was changed: $out" >&2
        exit 1
    fi
    grep -qF -- "$2" <<< "$out" || { echo "the interface test should say: $2" >&2; echo "$out" >&2; exit 1; }
    if [[ -n "${3:-}" ]] && grep -qF -- "$3" <<< "$out"; then
        echo "the interface test should not say: $3" >&2; echo "$out" >&2; exit 1
    fi
}

step "none: this tree"
loads "$root" runs

step "additions: two appended fields and two new SDK items"
new_tree additions
t="$work/tree-additions"
for n in 1 2; do
    git -C "$t" apply "$root/tools/compat/additions-$n.patch"
    if [[ "$gate" == 1 ]]; then
        regenerate "$t"
        # Only new: INTERFACE stays, the files are written anew.
        expect_gate "$t" "only new items" "raise native::INTERFACE"
        in_tree "$t" env PAPERPAW_BLESS=1 cargo test -q -p paperpaw-sdk --test interface
        gate_test "$t" > /dev/null
    fi
done
loads "$t" runs

step "refuse-layout: a field in the middle of the firmware's table"
new_tree refuse-layout
t="$work/tree-refuse-layout"
git -C "$t" apply "$root/tools/compat/refuse-layout.patch"
if [[ "$gate" == 1 ]]; then
    regenerate "$t"
    expect_gate "$t" "raise native::INTERFACE"
fi
loads "$t" refused

step "refuse-interface: native::INTERFACE raised"
new_tree refuse-interface
t="$work/tree-refuse-interface"
git -C "$t" apply "$root/tools/compat/refuse-interface.patch"
if [[ "$gate" == 1 ]]; then
    expect_gate "$t" "native::INTERFACE is 7, interface.txt says 6"
fi
loads "$t" refused

echo
echo "old plugins keep running after additions, and are refused cleanly after changes"

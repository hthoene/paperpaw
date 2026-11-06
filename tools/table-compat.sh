#!/usr/bin/env bash
# Old plugins against this tree's firmware bodies (the shared function
# table, #44; the CI job `table-compat`):
#
#   tools/table-compat.sh [<baseline-ref>]
#
# <baseline-ref> is the commit of the last interface raise (the tag
# `interface-<n>` the raise leaves); without it, the one
# crates/paperpaw-table/table-baseline.txt names, as CI and
# tools/ci-local.sh run it. A plugin built then calls the
# firmware by index, and an index must keep meaning the same function with
# the same behaviour. Against the baseline's crates this builds the test
# plugin (crates/paperpaw-core/tests/native-plugin) twice, and loads each
# into this tree's kernel (crates/paperpaw-core/tests/native_plugin.rs).
# The plugin's code is this tree's (its probes: probe.rs, the text, list,
# data and graphics calls; ui_probe.rs, every ui entry, mirrored, and text
# drawn right to left), the SDK's code it carries or calls the baseline's,
# as in a plugin built then: so a probe added later checks old plugins
# too, and calls only what the baseline already has.
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
# A baseline of an older plugin interface, or of an older draft of this
# one (`interface` and `draft` of crates/paperpaw-sdk/interface.txt, no
# draft: 0), built plugins this tree's firmware must refuse, not run: a
# raise and every draft step change what plugins see, so its bodies need
# not agree. Then the list's checks above run as ever, and the plugin is
# built once and must be refused at load (PAPERPAW_TEST_EXPECT=refused).
# Only a baseline of this interface and draft is run, as above. A
# baseline of a later interface or draft than this tree's is an error.
# While this interface is a draft, CI and ci-local therefore run the script
# twice: against table-baseline.txt (the refusal), and against HEAD (the
# plugin built both ways must run and agree).
#
# The baseline needs the host table mode itself (the commit that brought
# it, or a later one). Needs rustc, python3 and git; builds in
# $TABLE_COMPAT_DIR (default: a temporary directory).
set -euo pipefail

[[ $# -le 1 ]] || { echo "usage: $0 [<baseline-ref>]" >&2; exit 2; }
root="$(git rev-parse --show-toplevel)"
cd "$root"
ref="${1:-}"
if [[ -z "$ref" ]]; then
    ref="$(sed -e 's/#.*//' crates/paperpaw-table/table-baseline.txt | awk 'NF { print $1; exit }')"
    if [[ -z "$ref" || "$ref" == none ]]; then
        # The raise commit sets the baseline: once an interface with
        # tables is tagged, `none` would let every check pass unused.
        if git tag -l 'interface-*' | awk -F- '$2 + 0 >= 5 { found = 1 } END { exit !found }'; then
            echo "crates/paperpaw-table/table-baseline.txt: interface 5 or later is tagged, but the baseline is still none: set it to the raise's commit" >&2
            exit 1
        fi
        echo "no interface raise with tables yet: nothing to compare"
        exit 0
    fi
fi
base="$(git rev-parse --verify "$ref^{commit}")"
work="${TABLE_COMPAT_DIR:-$(mktemp -d)}"
tree="$work/baseline"
manifest=crates/paperpaw-core/tests/native-plugin/Cargo.toml
lib="release/libpaperpaw_test_native_plugin.so"

git worktree add --detach "$tree" "$base" > /dev/null
trap 'git worktree remove --force "$tree" > /dev/null' EXIT
plugin="$(dirname "$manifest")"
rm -rf "${tree:?}/$plugin"
mkdir -p "$tree/$plugin"
tar -C "$plugin" --exclude=./target -cf - . | tar -C "$tree/$plugin" -xf -

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

# Runs the one test that loads the plugin, by its exact name, and fails
# unless exactly that one ran: a renamed test would otherwise pass with none.
plugin_test() {
    local log
    log="$(mktemp)"
    cargo test -q -p paperpaw-core --test native_plugin -- --ignored --exact \
        a_plugin_built_apart_runs_in_the_kernel 2>&1 | tee "$log"
    if ! grep -q '^test result: ok\. 1 passed; 0 failed' "$log"; then
        rm -f "$log"
        echo "native_plugin: a_plugin_built_apart_runs_in_the_kernel did not run exactly once (renamed?)" >&2
        return 1
    fi
    rm -f "$log"
}

# `interface` and `draft` (0 without one) of interface.txt in tree $1.
level() {
    awk -F' *= *' '
        $1 == "interface" { i = $2 } $1 == "draft" { d = $2 }
        END { if (i !~ /^[0-9]+$/ || d !~ /^[0-9]*$/) exit 1; printf "%d %d\n", i, d }
    ' "$1/crates/paperpaw-sdk/interface.txt"
}
read -r then_interface then_draft < <(level "$tree") || {
    echo "baseline ${base:0:12}: no interface in its interface.txt" >&2; exit 1; }
read -r now_interface now_draft < <(level .) || {
    echo "this tree: no interface in its interface.txt" >&2; exit 1; }

# The plugin is a library of its own, built apart from this tree; a
# build's -D warnings would stop at the stubs' unused code.
echo "baseline plugin A: its own copies"
(cd "$tree" && env -u RUSTFLAGS CARGO_TARGET_DIR="$work/target-a" \
    cargo build --release --manifest-path "$manifest" -q)

if (( then_interface > now_interface || (then_interface == now_interface && then_draft > now_draft) )); then
    echo "baseline ${base:0:12} is interface $then_interface draft $then_draft, later than this tree's $now_interface draft $now_draft" >&2
    exit 1
fi
if (( then_interface != now_interface || then_draft != now_draft )); then
    echo "baseline ${base:0:12} is interface $then_interface draft $then_draft, this tree $now_interface draft $now_draft: its plugin must be refused"
    PAPERPAW_TEST_EXPECT=refused PAPERPAW_TEST_BUILT_FOR="$then_interface" \
        PAPERPAW_TEST_PLUGIN="$work/target-a/$lib" \
        plugin_test
    echo "old plugins are refused by this tree's firmware, cleanly"
    exit 0
fi
echo "baseline plugin B: host table mode"
(cd "$tree" && CARGO_TARGET_DIR="$work/target-b" \
    RUSTFLAGS="--cfg paperpaw_plugin --cfg paperpaw_table_host" \
    cargo build --release --manifest-path "$manifest" -q)

for mode in a b; do
    echo "this tree's kernel with plugin $mode"
    # The test checks the library is in the mode it is run as.
    tables=0; [[ $mode == b ]] && tables=1
    PAPERPAW_TEST_EXPECT=runs PAPERPAW_TEST_TABLES=$tables PAPERPAW_TEST_PLUGIN="$work/target-$mode/$lib" \
        plugin_test
done
echo "old plugins and this tree's firmware agree"

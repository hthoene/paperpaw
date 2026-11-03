#!/bin/sh
# Prints PaperPaw's changes to hadris-fat and hadris-fs (patches/hadris-*)
# as a unified diff against the release they were taken from, fetched from
# crates.io. To carry them to a newer release: run this, unpack the new
# release into patches/hadris-*, apply the diff with `patch -p1` and check
# each hunk against HADRIS-PATCHES.md.
#
#   patches/hadris-diff.sh [version] > hadris.patch   (default 3.0.0-rc.2)
set -eu
version=${1:-3.0.0-rc.2}
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for crate in hadris-fat hadris-fs; do
    curl -sfL "https://static.crates.io/crates/$crate/$crate-$version.crate" | tar xz -C "$work"
    mkdir -p "$work/a" "$work/b"
    cp -r "$work/$crate-$version" "$work/a/$crate"
    cp -r "$here/$crate" "$work/b/$crate"
done
cd "$work"
# What crates.io adds to a package, and the tests, examples and benches
# that are not vendored, are no change of ours.
diff -ru -x .cargo_vcs_info.json -x Cargo.toml.orig -x Cargo.lock \
    -x tests -x examples -x benches a b || true

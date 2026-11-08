#!/usr/bin/env bash
# Builds PaperPaw's own native interface-7 plugins (every folder of src/bin:
# a plugin crate with a native entry) as plugin files of format 5
# (abi/ppn5.md), packs each (signed with --key), and checks it as the X4's
# loader reads it:
#
#   tools/plugin-files7/build.sh <device> <out-dir> [--key <file>] [--only <id>]
#
# <device> is x4 (the ESP32-C3; the X4 Pro and the X4 Classic have no format-5
# profile yet, target 9). The files are <out-dir>/<id>-<version>-<device>.ppn,
# <version> the plugin's own (its crate's). With --only, one plugin alone.
# Run from anywhere; paperpaw-sign is built from this repository. A plugin that
# does not build or pack is named on standard error, after the others were
# tried. Exit status: 0 all built; 1 the packer refused some (a plugin that is
# not ready to be a file: writable data, a frame of 2,048 B or more; the others
# are built); 3 a plugin or the signer did not BUILD, or a file is not for the
# X4 (a broken build, never to be waved through); 2 usage.
# `complete.txt` lists the plugins whose port is complete (one id a line); each
# must have a file, and its PORTING-7.md must not say "Status: **not complete".
# The flash gate counts a file as interface 7 only for those.
#
# Needs: the X4's Rust from rust-toolchain.toml. Never set RUSTFLAGS: it would
# replace .cargo/config.toml's flags, which link the plugins.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

usage() { echo "usage: $0 x4 <out-dir> [--key <file>] [--only <id>]" >&2; exit 2; }
[[ $# -ge 2 ]] || usage
device=$1
out=$2
shift 2
key=()
only=
while [[ $# -gt 0 ]]; do
    case "$1" in
        --key) [[ $# -ge 2 ]] || usage; key=(--key "$2"); shift 2 ;;
        --only) [[ $# -ge 2 ]] || usage; only=$2; shift 2 ;;
        *) usage ;;
    esac
done
[[ $device = x4 ]] || { echo "only x4 has format-5 files yet (the X4 Pro's target 9 has no profile)" >&2; exit 2; }
target=riscv32imc-unknown-none-elf

ids="$(cd "$here/src/bin" && ls *.rs | sed 's/\.rs$//')"
if [[ -n "$only" ]]; then
    grep -qx "$only" <<< "$ids" || { echo "$only has no native entry here (src/bin)" >&2; exit 1; }
    ids=$only
fi
version_of() {
    sed -n '/^\[package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/plugins/$1/Cargo.toml"
}
for id in $ids; do
    [[ -n "$(version_of "$id")" ]] || { echo "plugins/$id/Cargo.toml: no version of its own" >&2; exit 1; }
done
cmp -s "$here/paperpaw-plugin7.x" "$root/crates/paperpaw-native/paperpaw-plugin7.x" ||
    { echo "paperpaw-plugin7.x differs from the crate's" >&2; exit 1; }

# The paths of the build are remapped, as tools/plugin-files/build.sh does
# (a variable split at spaces: with a space in a path the remap is left out).
target_dir="${CARGO_TARGET_DIR:-$here/target}"
remappable=1
for dir in "$root" "$target_dir" "${CARGO_HOME:-$HOME/.cargo}"; do
    case "$dir" in
        *[[:space:]]*) remappable=0; echo "warning: $dir has a space or tab in it; the paths are not remapped" >&2 ;;
    esac
done
if [[ $remappable = 1 ]]; then
    flags_var="CARGO_TARGET_$(tr 'a-z-' 'A-Z_' <<< "$target")_RUSTFLAGS"
    remap="--remap-path-prefix=$root=/build --remap-path-prefix=$target_dir=/target --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
    case " ${!flags_var:-} " in
        *" --remap-path-prefix=$root=/build "*) ;; # the caller has done it
        *) export "$flags_var=${!flags_var:+${!flags_var} }$remap" ;;
    esac
fi
# The signer is built first: a build failure of it is a broken build (3), not a refusal.
(cd "$root" && cargo build -q --release -p paperpaw-sign) || { echo "paperpaw-sign did not build" >&2; exit 3; }
sign() { (cd "$root" && cargo run -q --release -p paperpaw-sign -- "$@"); }

mkdir -p "$out"
out="$(cd "$out" && pwd)"
refused=() broken=()
for id in $ids; do
    file="$out/$id-$(version_of "$id")-$device.ppn"
    if ! (cd "$here" && env -u RUSTFLAGS cargo x4 --locked -q --bin "$id"); then
        broken+=("$id")
        echo "BUILD FAILED $id" >&2
        continue
    fi
    if ! sign pack "$target_dir/$target/release/$id" "$file" "${key[@]}"; then
        refused+=("$id")
        echo "REFUSED $id (the packer)" >&2
        continue
    fi
    shown="$(sign plugin "$file")" || { broken+=("$id"); echo "BUILD FAILED $id (the file does not check)" >&2; continue; }
    if ! grep -Eq "^chip: .*\(5\)\$" <<< "$shown"; then
        broken+=("$id")
        echo "BUILD FAILED $id: not for chip 5" >&2
        continue
    fi
    echo "$id: $(wc -c < "$file") bytes"
done
# The plugins whose port is complete (complete.txt) must all be files, and say so.
if [[ -z "$only" ]]; then
    for id in $(sed -e 's/#.*//' "$here/complete.txt" | awk 'NF { print $1 }'); do
        if grep -qE '^Status: \*\*not complete' "$root/plugins/$id/PORTING-7.md" 2>/dev/null; then
            echo "$id is in complete.txt but its PORTING-7.md says not complete" >&2
            broken+=("$id")
        elif [[ ! -e "$out/$id-$(version_of "$id")-$device.ppn" ]]; then
            echo "$id is in complete.txt but has no file" >&2
            broken+=("$id")
        fi
    done
fi
if ((${#broken[@]})); then
    echo "broken: ${broken[*]}" >&2
    exit 3
fi
if ((${#refused[@]})); then
    echo "not packed (refused): ${refused[*]}" >&2
    exit 1
fi

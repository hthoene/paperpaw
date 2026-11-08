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
# does not build or pack is named on standard error and the exit status is 1,
# after the others were tried.
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
sign() { (cd "$root" && cargo run -q --release -p paperpaw-sign -- "$@"); }
sign --help >/dev/null 2>&1 || true

mkdir -p "$out"
out="$(cd "$out" && pwd)"
failed=()
for id in $ids; do
    file="$out/$id-$(version_of "$id")-$device.ppn"
    if ! (cd "$here" && env -u RUSTFLAGS cargo x4 --locked -q --bin "$id") ||
        ! sign pack "$target_dir/$target/release/$id" "$file" "${key[@]}"; then
        failed+=("$id")
        echo "FAILED $id" >&2
        continue
    fi
    shown="$(sign plugin "$file")" || { failed+=("$id"); echo "FAILED $id (check)" >&2; continue; }
    if ! grep -Eq "^chip: .*\(5\)\$" <<< "$shown"; then
        failed+=("$id")
        echo "FAILED $id: not for chip 5" >&2
        continue
    fi
    echo "$id: $(wc -c < "$file") bytes"
done
if ((${#failed[@]})); then
    echo "not built: ${failed[*]}" >&2
    exit 1
fi

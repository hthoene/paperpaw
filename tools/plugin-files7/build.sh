#!/usr/bin/env bash
# Builds PaperPaw's own native interface-7 plugins (every folder of src/bin:
# a plugin crate with a native entry) as plugin files of format 5
# (abi/ppn5.md), packs each (signed with --key), and checks it as the X4's
# loader reads it:
#
#   tools/plugin-files7/build.sh <device> <out-dir> [--key <file>] [--only <id>]
#
# <device> is x4 (the ESP32-C3), x4-pro or x4-classic (the ESP32-S3, Xtensa).
# For the S3 devices the plugins are only BUILT (linked, ELF checked) so far,
# not packed: format 5 has no target-9 profile yet (decision 142; the profile
# is Kernel's, `abi/ppn5.md`), so no file is made and <out-dir> stays empty.
# Until every plugin has the S3's generated ABI metadata
# (plugins/<id>/native/$S3_METADATA, from the target-9 profile; without it a
# plugin's native_entry.rs refuses to compile), the S3 build says so and exits
# 4 ("pending"): CI shows that as a notice, never as a pass. Some but not all
# present is a broken tree (3).
# The files are <out-dir>/<id>-<version>-<device>.ppn,
# <version> the plugin's own (its crate's). With --only, one plugin alone.
# Run from anywhere; paperpaw-sign is built from this repository. A plugin that
# does not build or pack is named on standard error, after the others were
# tried. Exit status: 0 all built; 1 the packer refused some (a plugin that is
# not ready to be a file: writable data, a frame of 2,048 B or more; the others
# are built); 4 the S3's metadata profile is not there yet (pending, see above); 3 anything else that is wrong: a pre-flight failure (no
# version, no complete.txt, a linker script that differs), a plugin or the signer that
# did not BUILD, a file not for the X4, a complete.txt entry that fails; never to be
# waved through; 2 usage.
# `complete.txt` (it must exist) lists the plugins whose port is complete (one id a
# line); each must have a file and a PORTING-7.md that does not say "Status: **not
# complete". A plugin without a PORTING-7.md is accepted only with an explicit
# `status=complete` after its id in complete.txt.
# The flash gate counts a file as interface 7 only for those.
#
# Needs: the X4's Rust from rust-toolchain.toml. Never set RUSTFLAGS: it would
# replace .cargo/config.toml's flags, which link the plugins.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

usage() { echo "usage: $0 <x4|x4-pro|x4-classic> <out-dir> [--key <file>] [--only <id>]" >&2; exit 2; }
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
case "$device" in
    x4) target=riscv32imc-unknown-none-elf; build=(cargo x4 --locked -q); pack=1 ;;
    # espup's `esp` toolchain with the release's core libraries in it (tools/xtensa-std).
    x4-pro | x4-classic) target=xtensa-esp32s3-none-elf; build=(cargo +esp x4pro --locked -q); pack=0 ;;
    *) usage ;;
esac

ids="$(cd "$here/src/bin" && ls *.rs | sed 's/\.rs$//')"
if [[ -n "$only" ]]; then
    grep -qx "$only" <<< "$ids" || { echo "$only has no native entry here (src/bin)" >&2; exit 3; }
    ids=$only
fi
version_of() {
    sed -n '/^\[package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/plugins/$1/Cargo.toml"
}
for id in $ids; do
    [[ -n "$(version_of "$id")" ]] || { echo "plugins/$id/Cargo.toml: no version of its own" >&2; exit 3; }
done
# The S3's generated metadata is named by the target-9 profile (Kernel's, decision 142).
S3_METADATA=metadata.s3.bin
if [[ $pack = 0 ]]; then
    have=0 lack=()
    for id in $ids; do
        if [[ -e "$root/plugins/$id/native/$S3_METADATA" ]]; then have=$((have + 1)); else lack+=("$id"); fi
    done
    if ((have == 0)); then
        echo "S3 build PENDING: no plugin has native/$S3_METADATA yet (the target-9 profile of abi/ppn5.md); nothing was built" >&2
        exit 4
    elif ((${#lack[@]})); then
        echo "native/$S3_METADATA is missing for: ${lack[*]}" >&2
        exit 3
    fi
fi
cmp -s "$here/paperpaw-plugin7.x" "$root/crates/paperpaw-native/paperpaw-plugin7.x" ||
    { echo "paperpaw-plugin7.x differs from the crate's" >&2; exit 3; }
[[ -f "$here/complete.txt" ]] ||
    { echo "tools/plugin-files7/complete.txt is missing: the gate could not tell which ports are complete" >&2; exit 3; }

# The paths of the build are remapped, as tools/plugin-files/build.sh does
# (a variable split at spaces): with a space in a path the remap cannot be made
# and the files would depend on where they were built, so the build refuses,
# unless PAPERPAW_ALLOW_UNREMAPPED=1 (a trial build; ignored with --key: a
# signed build is never one).
target_dir="${CARGO_TARGET_DIR:-$here/target}"
remappable=1
spaced=
for dir in "$root" "$target_dir" "${CARGO_HOME:-$HOME/.cargo}"; do
    case "$dir" in
        *[[:space:]]*) remappable=0; spaced=$dir ;;
    esac
done
if [[ $remappable = 0 ]]; then
    if [[ ${PAPERPAW_ALLOW_UNREMAPPED:-} = 1 && ${#key[@]} = 0 ]]; then
        echo "warning: $spaced has a space or tab in it; the paths are not remapped, so the plugin files depend on where they are built (PAPERPAW_ALLOW_UNREMAPPED=1)" >&2
    else
        echo "error: $spaced has a space or tab in it, so the paths cannot be remapped and the plugin files would depend on where they are built. Use a path without one. (PAPERPAW_ALLOW_UNREMAPPED=1 allows a trial build, but never one with --key.)" >&2
        exit 3
    fi
fi
if [[ $remappable = 1 ]]; then
    flags_var="CARGO_TARGET_$(tr 'a-z-' 'A-Z_' <<< "$target")_RUSTFLAGS"
    remap="--remap-path-prefix=$root=/build --remap-path-prefix=$target_dir=/target --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
    case " ${!flags_var:-} " in
        *" --remap-path-prefix=$root=/build "*) ;; # the caller has done it
        *) export "$flags_var=${!flags_var:+${!flags_var} }$remap" ;;
    esac
fi
# The signer is built first: a build failure of it is a broken build (3), not a refusal.
if [[ $pack = 1 ]]; then
    (cd "$root" && cargo build -q --release -p paperpaw-sign) || { echo "paperpaw-sign did not build" >&2; exit 3; }
fi
sign() { (cd "$root" && cargo run -q --release -p paperpaw-sign -- "$@"); }

mkdir -p "$out"
out="$(cd "$out" && pwd)"
refused=() broken=()
for id in $ids; do
    file="$out/$id-$(version_of "$id")-$device.ppn"
    if ! (cd "$here" && env -u RUSTFLAGS "${build[@]}" --bin "$id"); then
        broken+=("$id")
        echo "BUILD FAILED $id" >&2
        continue
    fi
    if [[ $pack = 0 ]]; then
        # No target-9 profile yet: built and linked, not packed. The ELF must
        # exist and be Xtensa (machine 94) so that a silent fallback to
        # another target cannot pass.
        elf="$target_dir/$target/release/$id"
        if [[ ! -s "$elf" ]] || [[ "$(od -An -tu2 -j18 -N2 "$elf" | tr -d ' ')" != 94 ]]; then
            broken+=("$id")
            echo "BUILD FAILED $id: no Xtensa ELF at $elf" >&2
            continue
        fi
        echo "$id: built for $device ($(wc -c < "$elf") bytes of ELF), not packed (no target-9 profile yet)"
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
if [[ -z "$only" && $pack = 1 ]]; then
    while read -r id mark _; do
        notes="$root/plugins/$id/PORTING-7.md"
        if [[ -e "$notes" ]]; then
            if grep -qE '^Status: \*\*not complete' "$notes"; then
                echo "$id is in complete.txt but its PORTING-7.md says not complete" >&2
                broken+=("$id")
            fi
        elif [[ $mark != status=complete ]]; then
            echo "$id is in complete.txt but has no PORTING-7.md and no status=complete mark there" >&2
            broken+=("$id")
        fi
        if [[ ! -e "$out/$id-$(version_of "$id")-$device.ppn" ]]; then
            echo "$id is in complete.txt but has no file" >&2
            broken+=("$id")
        fi
    done < <(sed -e 's/#.*//' "$here/complete.txt" | awk 'NF { print $1, $2 }')
fi
if ((${#broken[@]})); then
    echo "broken: ${broken[*]}" >&2
    exit 3
fi
if ((${#refused[@]})); then
    echo "not packed (refused): ${refused[*]}" >&2
    exit 1
fi

#!/usr/bin/env bash
# The X4 flash gate (owner decision 136, checklist G1): the finished firmware
# may be at most 60,000 bytes larger than the Beta 3 firmware, same profile and
# settings. Builds both in ONE worktree path (the path is in the panic
# messages, so sizes stay comparable), measures what the chip's flash holds,
# and prints the difference against the gate. Build only: no device.
#
#   tools/flash-gate.sh [--head REV] [--base TAG] [--base-features LIST]
#                       [--head-features LIST] [--label TEXT] [--limit BYTES]
#                       [--jobs N] [--work DIR] [--keep]
#
# Defaults: head HEAD with features `x4,ppn5` (as the finished firmware
# ships); base `v0.6.0-beta.3` with `x4`. Each tree builds with its own
# `rust-toolchain.toml` and `.cargo/config.toml`, release profile, no default
# features (no `remote`, no `waveform-lab`), as the release image is built.
# Until interface 6 is removed the result is an ESTIMATE: say so with
# `--label 'estimate, interface 6 still linked'`; the label is printed with
# the result. Exit 0: within the limit. Exit 1: ONLY when the delta is over it.
# Exit 2: every other failure (bad argument, no toolchain, failed build).
set -Eeuo pipefail
# Any failure that is not a measurement is exit 2 ("over the gate" is 1).
trap 'exit 2' ERR
die() { echo "flash-gate: $*" >&2; exit 2; }

head=HEAD base=v0.6.0-beta.3 base_features=x4 head_features=x4,ppn5
label="" limit=60000 jobs=4 keep=0
work="${TMPDIR:-/tmp}"
target=riscv32imc-unknown-none-elf

while [ $# -gt 0 ]; do
    case "$1" in
        --head) head=$2; shift 2 ;;
        --base) base=$2; shift 2 ;;
        --base-features) base_features=$2; shift 2 ;;
        --head-features) head_features=$2; shift 2 ;;
        --label) label=$2; shift 2 ;;
        --limit) limit=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        --work) work=$2; shift 2 ;;
        --keep) keep=1; shift ;;
        -h|--help) sed -n 2,19p "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
head_sha="$(git rev-parse --verify "$head^{commit}")"
base_sha="$(git rev-parse --verify "$base^{commit}")"

# Everything goes into a directory of its own inside --work, made here; only
# that directory is ever removed, never --work itself.
mkdir -p "$work" || die "cannot make $work"
ours="$(mktemp -d "$work/flash-gate.XXXXXX")" || die "cannot make a directory in $work"
cleanup() {
    git -C "$root" worktree remove --force "$ours/tree" >/dev/null 2>&1 || true
    if [ "$keep" = 0 ]; then rm -rf "$ours"; else echo "kept: $ours" >&2; fi
}
trap cleanup EXIT
git worktree add -q --detach "$ours/tree" "$base_sha"

# What the chip's flash holds: code, read-only data and the initial values of
# data (the sections the ELF loads into flash).
flash_bytes() {
    size -A "$1" | awk '$1 ~ /^(\.trap|\.rwtext.*|\.data.*|\.flash\.appdesc|\.rodata.*|\.text)$/ && $1 != ".rwdata_dummy" {s += $2} END {print s}'
}

build() { # name rev sha features
    local name=$1 sha=$2 features=$3
    git -C "$ours/tree" checkout -q --detach "$sha"
    local tool
    tool="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$ours/tree/shells/xteink/rust-toolchain.toml")"
    [ -n "$tool" ] || die "$name: no toolchain channel in shells/xteink/rust-toolchain.toml"
    echo "== $name: $sha, features $features, toolchain $tool" >&2
    # shellcheck disable=SC2086
    (cd "$ours/tree/shells/xteink" &&
        env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR="$ours/target-$name" \
            cargo +"$tool" build --locked -j"$jobs" --release --target "$target" \
            --no-default-features --features "$features" >&2) || { echo "build failed: $name" >&2; exit 2; }
    cp "$ours/target-$name/$target/release/paperpaw-xteink" "$ours/$name.elf"
    echo "$tool" > "$ours/$name.tool"
}

build base "$base_sha" "$base_features"
build head "$head_sha" "$head_features"

b=$(flash_bytes "$ours/base.elf")
h=$(flash_bytes "$ours/head.elf")
delta=$((h - b))
echo
echo "base  $base ($base_sha) features $base_features toolchain $(cat "$ours/base.tool"): $b B"
echo "head  $head ($head_sha) features $head_features toolchain $(cat "$ours/head.tool"): $h B"
if [ "$(cat "$ours/base.tool")" != "$(cat "$ours/head.tool")" ]; then
    echo "note: the two trees pin different toolchains; the delta includes the compiler's difference"
fi
# The images the chip is flashed with, for the record (not the gate).
for n in base head; do
    if command -v espflash >/dev/null 2>&1 &&
        espflash save-image --chip esp32c3 "$ours/$n.elf" "$ours/$n.bin" >/dev/null 2>&1; then
        echo "application.bin $n: $(wc -c < "$ours/$n.bin") B"
    else
        echo "application.bin $n: not made (espflash missing or failed)"
    fi
done
echo "delta $delta B over the gate of $limit B (flash-resident: .text, .rodata, .data load, .rwtext, .trap)"
[ -n "$label" ] && echo "RESULT (${label}): $delta B"
if [ "$delta" -le "$limit" ]; then
    echo "GATE: within ($((limit - delta)) B to spare)"
else
    echo "GATE: OVER by $((delta - limit)) B"
    exit 1
fi

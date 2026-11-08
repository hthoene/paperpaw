#!/usr/bin/env bash
# The X4 flash gate (owner decision 136, checklist G1): the finished firmware
# may be at most 60,000 bytes larger than the Beta 3 firmware, same profile and
# settings. Builds both in ONE worktree path (the path is in the panic
# messages, so sizes stay comparable), measures what the chip's flash holds,
# and prints the difference against the gate. Build only: no device.
#
#   tools/flash-gate.sh [--head REV] [--base TAG] [--base-features LIST]
#                       [--head-features LIST] [--label TEXT] [--limit BYTES]
#                       [--jobs N] [--work DIR] [--keep] [--image-only]
#                       [--plugin-elf FILE]...
#
# Like for like (Manager ruling on decision 136): the gate counts the image
# AND the plugin files (`tools/plugin-files/build.sh x4`, the `.ppn` the store
# offers) of both trees, for the same features. A plugin that Beta 3 built into
# the image and that is a file now (ported) counts in the head's files; one
# not ported yet stays in the image (listed, flagged NOT PORTED); one that did
# not exist in Beta 3 is listed (NEW) and, as a file, left out of the gate; a new
# plugin built into the image is listed too, but its bytes stay in the image
# delta (they cannot be separated from it). `--image-only`
# measures the image alone, as before.
#
# `--plugin-elf FILE` (any number; the ELF a `.ppn` is packed from) also runs
# `tools/plugin-symbols.sh` on it: a plugin that links the whole records codec or
# the text-escaping tables fails the gate (exit 1) whatever the delta.
#
# Both builds remap the work directory, the cargo home
# (`--remap-path-prefix`, through `--config`, which adds to the flags of
# `.cargo/config.toml`), so that the strings of the paths are the same. That is
# not enough for equal sizes: the compiler's crate hashes (and so the layout)
# still depend on the path. The same --work gives the same result to within
# about 10 bytes (measured twice, #4382); another --work, even with the same
# remaps, moved the total by up to 12 KB. Compare only runs with the same --work.
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
label="" limit=60000 jobs=4 keep=0 image_only=0 plugin_elfs=()
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
        --plugin-elf) plugin_elfs+=("$2"); shift 2 ;;
        --image-only) image_only=1; shift ;;
        -h|--help) sed -n 2,40p "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
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
# A fixed name: the path is part of what the compiler sees (the crates' hashes
# and so the layout), so a random name would move the sizes by a little.
ours="$work/flash-gate"
[ ! -e "$ours" ] || die "$ours exists (a kept run?): remove it, or use another --work"
mkdir "$ours" || die "cannot make $ours"
cleanup() {
    git -C "$root" worktree remove --force "$ours/tree" >/dev/null 2>&1 || true
    if [ "$keep" = 0 ]; then rm -rf "$ours"; else echo "kept: $ours" >&2; fi
}
trap cleanup EXIT
# A worktree record left by an earlier run that was killed would refuse the path.
git worktree prune
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
    local cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    local remap="target.$target.rustflags=[\"--remap-path-prefix=$ours/tree=/build\",\"--remap-path-prefix=$ours/target-$name=/target\",\"--remap-path-prefix=$cargo_home=/cargo\"]"
    (cd "$ours/tree/shells/xteink" &&
        env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR="$ours/target-$name" \
            cargo +"$tool" build --locked -j"$jobs" --release --target "$target" --config "$remap" \
            --no-default-features --features "$features" >&2) || { echo "build failed: $name" >&2; exit 2; }
    cp "$ours/target-$name/$target/release/paperpaw-xteink" "$ours/$name.elf"
    cp "$ours/tree/shells/xteink/partitions.csv" "$ours/$name.csv"
    echo "$tool" > "$ours/$name.tool"
    if [ "$image_only" = 0 ]; then
        echo "== $name: the plugin files (tools/plugin-files/build.sh x4)" >&2
        [ -x "$ours/tree/tools/plugin-files/build.sh" ] || die "$name: no tools/plugin-files/build.sh"
        # The remap is set here too: Beta 3's build.sh does not do it itself.
        env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR="$ours/ppn-target-$name" \
            CARGO_TARGET_RISCV32IMC_UNKNOWN_NONE_ELF_RUSTFLAGS="--remap-path-prefix=$ours/tree=/build --remap-path-prefix=$ours/ppn-target-$name=/target --remap-path-prefix=$cargo_home=/cargo" \
            "$ours/tree/tools/plugin-files/build.sh" x4 "$ours/ppn-$name" >&2 ||
            { echo "plugin files failed: $name" >&2; exit 2; }
        rm -rf "$ours/ppn-target-$name"
    fi
}

build base "$base_sha" "$base_features"
build head "$head_sha" "$head_features"

b=$(flash_bytes "$ours/base.elf")
h=$(flash_bytes "$ours/head.elf")
image_delta=$((h - b))
delta=$image_delta
pb=0 ph=0
if [ "$image_only" = 0 ]; then
    # The table of plugins and the two sums (python3: parsing, nm, sums).
    python3 -I - "$ours" "$target" <<'PY' || die "the plugin table failed"
import os, re, subprocess, sys
ours = sys.argv[1]
def files(name):
    out = {}
    d = os.path.join(ours, "ppn-" + name)
    for f in sorted(os.listdir(d)):
        # <id>-<version>-x4.ppn; a pre-release may contain '-' (1.0.0-beta-2).
        m = re.match(r"(.+?)-\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.+-]*)?-x4\.ppn$", f)
        if m: out[m.group(1)] = os.path.getsize(os.path.join(d, f))
    return out
def builtin(name):
    nm = subprocess.run(["nm", "-C", "--defined-only", os.path.join(ours, name + ".elf")],
                        capture_output=True, text=True, check=True).stdout
    if not nm.strip(): sys.exit(f"nm found no symbols in {name}.elf (stripped?)")
    return {m.replace("_", "-") for m in set(re.findall(r"\bpaperpaw_plugin_(\w+?)::", nm))}
bf, hf, bb, hb = files("base"), files("head"), builtin("base"), builtin("head")
if not bb: sys.exit("no built-in plugin symbols in base.elf: Beta 3 has some, so the symbols are missing")
ids = sorted(set(bf) | set(hf) | bb | hb)
pb = sum(bf.values())
# A plugin with a Beta 3 file but no head file is not shipped as a file on head
# yet (waiting for its interface-7 build). It is counted at its Beta 3 size, as
# the rule for plugins that are not ported says; dropping it would flatter the head.
pending = {k: v for k, v in bf.items() if k not in hf and k not in hb}
ph = sum(v for k, v in hf.items() if k in bf or k in bb) + sum(pending.values())
rows = []
for i in ids:
    if i in bf and i in hf: st = "file in both"
    elif i in bb and i in hf: st = "ported (image -> file)"
    elif i in bb and i in hb and i not in hf: st = "NOT PORTED (in the image, counted there)"
    elif i in hf and i not in bf and i not in bb: st = "NEW since Beta 3 (left out of the gate)"
    elif i in hb and i not in hf and i not in bf and i not in bb:
        st = "NEW since Beta 3, built into the image (its bytes are in the image delta)"
    elif i in bf and i not in hf: st = ("no head file yet (counted at its Beta 3 size)" if i in pending
                                         else "file in Beta 3 only (built into the image now)")
    else: st = "other"
    rows.append((i, bf.get(i), hf.get(i), st))
print()
print(f"{'plugin':28} {'Beta 3 .ppn':>12} {'head .ppn':>12}  status")
for i, b, h, st in rows:
    print(f"{i:28} {b if b is not None else '-':>12} {h if h is not None else '-':>12}  {st}")
print(f"{'sum of files':28} {pb:>12} {sum(hf.values()):>12}  (head counted in the gate: {ph}, of which {sum(pending.values())} pending at Beta 3 size)")
open(os.path.join(ours, "ppn-sums"), "w").write(f"{pb} {ph}\n")
PY
    read -r pb ph < "$ours/ppn-sums"
    delta=$(( (h + ph) - (b + pb) ))
fi
echo
echo "base  $base ($base_sha) features $base_features toolchain $(cat "$ours/base.tool"): $b B"
echo "head  $head ($head_sha) features $head_features toolchain $(cat "$ours/head.tool"): $h B"
if [ "$(cat "$ours/base.tool")" != "$(cat "$ours/head.tool")" ]; then
    echo "note: the two trees pin different toolchains; the delta includes the compiler's difference"
fi
# The images the chip is flashed with, for the record (not the gate).
for n in base head; do
    if command -v espflash >/dev/null 2>&1 &&
        espflash save-image --chip esp32c3 --partition-table "$ours/$n.csv" --flash-size 16mb "$ours/$n.elf" "$ours/$n.bin" >/dev/null 2>&1; then
        echo "application.bin $n: $(wc -c < "$ours/$n.bin") B"
    else
        echo "application.bin $n: not made (espflash missing or failed)"
    fi
done
if [ "$image_only" = 0 ]; then
    echo "image delta $image_delta B; plugin files: Beta 3 $pb B, head $ph B (same feature set)"
    echo "total Beta 3 $((b + pb)) B, head $((h + ph)) B"
fi
echo "delta $delta B against the gate of $limit B (image: .text, .rodata, .data load, .rwtext, .trap$( [ "$image_only" = 0 ] && echo '; plus the plugin files' ))"
[ -n "$label" ] && echo "RESULT (${label}): $delta B"
if [ ${#plugin_elfs[@]} -gt 0 ]; then
    # Exit 2 (no symbols, no nm) ends the gate through the ERR trap; 1 is a finding.
    if "$root/tools/plugin-symbols.sh" "${plugin_elfs[@]}"; then :; else
        [ $? -eq 1 ] || exit 2
        echo "GATE: PLUGIN CODEC (see above)"
        exit 1
    fi
fi
if [ "$delta" -le "$limit" ]; then
    echo "GATE: within ($((limit - delta)) B to spare)"
else
    echo "GATE: OVER by $((delta - limit)) B"
    exit 1
fi

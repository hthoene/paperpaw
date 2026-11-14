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
# The head's files are the interface-7 (format 5) builds of
# `tools/plugin-files7/build.sh` for every plugin that has a native entry and
# ships as a file (a Beta 3 file or a current head file; not a plugin linked
# into the image); a plugin without one counts with its current file, listed.
# A Beta 3 built-in that is nowhere on the head (not in the image, no file,
# not counted at its Beta 3 size) fails the gate with exit 2.
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
# Defaults: head HEAD with features `x4` (native plugins on, as the firmware
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

head=HEAD base=v0.6.0-beta.3 base_features=x4 head_features=x4
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
# The plugin builds take their path remaps from a variable split at spaces.
case "$(cd "$work" && pwd)" in
    *[[:space:]]*) die "--work has a space or tab in its path: use another" ;;
esac
# The cargo home's remap goes into the same split variable.
case "${CARGO_HOME:-$HOME/.cargo}" in
    *[[:space:]]*) die "CARGO_HOME (${CARGO_HOME:-$HOME/.cargo}) has a space or tab in its path: the build's path remaps would be split; use another" ;;
esac
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
# `-f`: a record of this very path left by an earlier run that was killed
# (its folder gone) is taken over. Not `git worktree prune`, which would clean
# the records of everybody else's worktrees in this repository too.
# The records of earlier runs of this script that were killed, whose folder is
# gone: named by their path (`.../flash-gate/tree`) and removed one by one,
# which leaves everybody else's records alone.
git -C "$root" worktree list --porcelain |
    awk '/^worktree /{p = substr($0, 10)} /^prunable/ {if (p ~ /\/flash-gate\/tree$/) print p}' |
    while IFS= read -r stale; do
        git -C "$root" worktree remove -f -f "$stale" >/dev/null 2>&1 || true
    done
git worktree add -q -f --detach "$ours/tree" "$base_sha"

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
        # As many files as the tree's list has plugins: a plugin left out
        # (or one the build did not make) would lower the sum.
        local listed made
        listed="$(sed -e 's/#.*//' "$ours/tree/tools/plugin-files/plugins.txt" | awk 'NF { n++ } END { print n + 0 }')"
        made="$(find "$ours/ppn-$name" -maxdepth 1 -name '*.ppn' | wc -l)"
        [ "$listed" -gt 0 ] && [ "$made" = "$listed" ] ||
            { echo "$name: $made plugin files for $listed plugins in plugins.txt" >&2; exit 2; }
        # The interface-7 (format 5) files of the plugins that have a native
        # entry (`tools/plugin-files7/build.sh`), where the tree has the
        # pipeline: the head counts them in place of the current builds. A
        # plugin the packer refuses is named on stderr (exit 1) and stays on
        # its current file; any other failure ends the gate.
        mkdir -p "$ours/ppn7-$name"
        if [ -x "$ours/tree/tools/plugin-files7/build.sh" ]; then
            echo "== $name: the interface-7 plugin files (tools/plugin-files7/build.sh x4)" >&2
            rc=0
            env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR="$ours/ppn7-target-$name" \
                CARGO_TARGET_RISCV32IMC_UNKNOWN_NONE_ELF_RUSTFLAGS="--remap-path-prefix=$ours/tree=/build --remap-path-prefix=$ours/ppn7-target-$name=/target --remap-path-prefix=$cargo_home=/cargo" \
                "$ours/tree/tools/plugin-files7/build.sh" x4 "$ours/ppn7-$name" >&2 || rc=$?
            # 1 is the packer refusing a plugin (named on stderr; it stays on its
            # current file); 3 is a broken build and anything else a failure: the gate ends.
            [ "$rc" -le 1 ] || { echo "interface-7 plugin files failed ($rc): $name" >&2; exit 2; }
            rm -rf "$ours/ppn7-target-$name"
            [ -f "$ours/tree/tools/plugin-files7/complete.txt" ] || { echo "tools/plugin-files7/complete.txt is missing: $name" >&2; exit 2; }
            cp "$ours/tree/tools/plugin-files7/complete.txt" "$ours/complete-$name.txt"
        fi
    fi
}

build base "$base_sha" "$base_features"
build head "$head_sha" "$head_features"

b=$(flash_bytes "$ours/base.elf")
h=$(flash_bytes "$ours/head.elf")
image_delta=$((h - b))
delta=$image_delta
pb=0 ph=0 ph_all=0
if [ "$image_only" = 0 ]; then
    # The table of plugins and the two sums (python3: parsing, nm, sums).
    python3 -I - "$ours" "$target" <<'PY' || die "the plugin table failed"
import os, re, subprocess, sys
ours = sys.argv[1]
def files(name, prefix="ppn-"):
    out = {}
    d = os.path.join(ours, prefix + name)
    if not os.path.isdir(d): return out
    for f in sorted(os.listdir(d)):
        # <id>-<version>-x4.ppn; a pre-release may contain '-' (1.0.0-beta-2).
        # A file that is not named so cannot be counted, and a gate must not
        # undercount: stop (the caller exits 2).
        if not f.endswith(".ppn"): continue
        m = re.match(r"(.+?)-\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.+-]*)?-x4\.ppn$", f)
        if not m: sys.exit(f"{name}: the plugin file {f} does not match <id>-<version>-x4.ppn")
        if m.group(1) in out: sys.exit(f"{name}: two files for the plugin {m.group(1)}")
        out[m.group(1)] = os.path.getsize(os.path.join(d, f))
    return out
def builtin(name):
    nm = subprocess.run(["nm", "-C", "--defined-only", os.path.join(ours, name + ".elf")],
                        capture_output=True, text=True, check=True).stdout
    if not nm.strip(): sys.exit(f"nm found no symbols in {name}.elf (stripped?)")
    return {m.replace("_", "-") for m in set(re.findall(r"\bpaperpaw_plugin_(\w+?)::", nm))}
bf, hf, bb, hb = files("base"), files("head"), builtin("base"), builtin("head")
# The interface-7 files of the head (format 5). One counts only for a plugin that
# ships as a file today (a Beta 3 file or a current head file) and is not linked
# into the head's image: a linked built-in has a file here too, which is not shipped.
# Only a plugin whose port is complete (tools/plugin-files7/complete.txt) counts as
# interface 7: a partial port's file is a native stub beside the interface-6 plugin, not
# the plugin, and must not be set against a full Beta 3 file. The partial files are
# counted separately (the informational figure).
complete = {l.split("#")[0].split()[0] for l in open(os.path.join(ours, "complete-head.txt")) if l.split("#")[0].split()}
all7 = {k: v for k, v in files("head", "ppn7-").items() if (k in bf or k in hf) and k not in hb}
hf7 = {k: v for k, v in all7.items() if k in complete}
nofile = sorted(complete - set(hf7))
if nofile:
    print("MISSING: complete.txt lists " + ", ".join(nofile) + ", which have no interface-7 file", file=sys.stderr)
    sys.exit(1)
eff = {**hf, **hf7}
eff_all = {**hf, **all7}
if not bb: sys.exit("no built-in plugin symbols in base.elf: Beta 3 has some, so the symbols are missing")
ids = sorted(set(bf) | set(hf) | bb | hb)
pb = sum(bf.values())
# A plugin with a Beta 3 file but no head file is not shipped as a file on head
# yet (waiting for its interface-7 build). It is counted at its Beta 3 size, as
# the rule for plugins that are not ported says; dropping it would flatter the head.
pending = {k: v for k, v in bf.items() if k not in eff and k not in hb}
ph = sum(v for k, v in eff.items() if k in bf or k in bb) + sum(pending.values())
rows = []
for i in ids:
    if i in bf and i in hf7: st = "file in both (interface 7)"
    elif i in bf and i in hf and i in all7: st = "file in both (current build; interface-7 file is a partial port, not counted)"
    elif i in bf and i in hf: st = "file in both (current build, not interface 7)"
    elif i in bb and i in hf: st = "ported (image -> file)"
    elif i in bb and i in hb and i not in hf: st = "NOT PORTED (in the image, counted there)"
    elif i in hf and i not in bf and i not in bb: st = "NEW since Beta 3 (left out of the gate)"
    elif i in hb and i not in hf and i not in bf and i not in bb:
        st = "NEW since Beta 3, built into the image (its bytes are in the image delta)"
    elif i in bf and i not in hf: st = ("no head file yet (counted at its Beta 3 size)" if i in pending
                                         else "file in Beta 3 only (built into the image now)")
    else: st = "other"
    rows.append((i, bf.get(i), eff.get(i), st))
print()
print(f"{'plugin':28} {'Beta 3 .ppn':>12} {'head .ppn':>12}  status")
for i, b, h, st in rows:
    print(f"{i:28} {b if b is not None else '-':>12} {h if h is not None else '-':>12}  {st}")
print(f"{'sum of files':28} {pb:>12} {sum(eff.values()):>12}  (head counted in the gate: {ph}, of which {sum(pending.values())} pending at Beta 3 size)")
# A Beta 3 built-in must still be somewhere on the head: in the image (linked or
# not yet ported), as a file, or counted at its Beta 3 size. One that is nowhere
# would make the head look smaller than it is.
missing = sorted(i for i in bb if i not in hb and i not in eff and i not in pending)
if missing:
    print("MISSING: built into Beta 3 and on the head nowhere (not in the image, "
          "no file, not pending): " + ", ".join(missing), file=sys.stderr)
    sys.exit(1)
cur = sorted(i for i in eff if i in bf and i not in hf7)
ph_all = sum(v for k, v in eff_all.items() if k in bf or k in bb) + sum({k: v for k, v in bf.items() if k not in eff_all and k not in hb}.values())
print(f"head files built for interface 7 (format 5): {len(hf7)}"
      + (f"; still on the current build (counted as that file): {', '.join(cur)}" if cur else ""))
if pending:
    print("not built yet, counted at the Beta 3 file size: " + ", ".join(sorted(pending)))
open(os.path.join(ours, "ppn-sums"), "w").write(f"{pb} {ph} {ph_all}\n")
PY
    read -r pb ph ph_all < "$ours/ppn-sums"
    delta=$(( (h + ph) - (b + pb) ))
fi
echo
echo "work dir $work: sizes (the Beta 3 total too) move by up to 12 KB with the path, so compare only runs with the same --work"
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
    # Informational: every interface-7 file counted, partial ports' stubs too.
    echo "INFO (not the gate): with the partial ports' interface-7 files counted too: delta $(( (h + ph_all) - (b + pb) )) B"
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

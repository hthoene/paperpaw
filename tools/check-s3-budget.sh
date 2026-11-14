#!/usr/bin/env bash
# CI's S3 frame-budget scan, run locally (job "Firmware (x4-pro)" and
# "(x4-classic)", step "The S3's host frames and calls fit under the band"):
#
#   tools/check-s3-budget.sh <worktree> [x4-pro|x4-classic|both]
#
# LOCAL ONLY: this is for a machine's smoke runs, CI does its own build and
# does not call it.
#
# Builds the S3 firmware of the worktree as CI does for that step (the `esp`
# toolchain, `--features <device>,ppn5`, legacy symbol mangling so that the
# budget's names are plain paths) and runs
#
#   paperpaw-sign frames-s3 <elf> --budget shells/xteink/frame-budget-s3[-classic].txt
#
# on it. Exit 0: the budget holds for every device asked (default: both; see
# the note on stale `allow` lines below).
# Exit 1: a build or a scan failed (the scan's own words are printed; each
# device's full output is in <worktree>/target-budget/<device>.scan.txt).
# Exit 2: bad usage or no toolchain.
#
# CHECK_S3_BUDGET_LINKED=1 in the environment scans with the linked plugins' own
# code rooted as well (`shells/xteink/frame-budget-s3-linked.txt`, todo 624; the
# budgets do not include it until the frames it finds are cut, todo 625/626).
#
# CHECK_S3_BUDGET_KEEP=<dir> in the environment copies each device's ELF to
# <dir>/<device>.elf (both devices build into one directory, the second
# overwrites the first; this is how to have both, e.g. to regenerate a budget).
#
# Time, measured on this machine (16 cores, the build with -j4): both devices,
# nothing built before, 6 minutes (4 for one device; the first one builds core for
# the S3 with `-Zbuild-std`); both again with nothing changed, 4 seconds. After a
# change in some crates: those and what depends on them, relinked and scanned,
# for each device (not measured).
#
# The budget depends on the build's layout (LLVM's inlining and the order of
# code units follow the paths of the sources), so the build must see the CI's
# checkout path, `/home/runner/work/paperpaw/paperpaw`. Mapping paths with
# `--remap-path-prefix` is NOT enough (it gave a different image: a `memset`
# call and two stale lines the real path did not have), and a mount namespace
# is not allowed in the agents' container. So the script syncs the worktree's
# files (tracked and untracked, not ignored) to that path, under a lock (only
# one run at a time on a machine), and builds there into `shells/xteink/target`,
# as CI does (the length of that path is in the image too). The sync copies
# only files whose content differs and removes the ones the worktree no longer
# has; a file it copies gets the time of the copy (cargo compares times, so a
# changed file that kept an old time would look unchanged), one it leaves alone
# keeps its time, and so do the directories (a build script that watches a
# directory would otherwise rebuild everything): a run after a change rebuilds
# only what the change touched. It REPLACES everything in that
# directory except `.git` and `shells/xteink/target`: do not keep anything else
# there. There is ONE such copy and ONE target directory on the machine (every
# run reuses them, never one per worktree); the script prints the size of the
# target directory at the end (delete it when the disk is tight, the next run
# rebuilds). Both devices build into the same directory, side by side.
#
# A stale `allow` line (one that "matches nothing any more") is an error in CI, so it is
# one here. Train 3 (`9d278c315`) once gave one extra followed call here (5826 against
# 5825; cause not found), but on train 40 (`0b0b143a3`) this scan and CI's agreed on
# everything: the same counts of functions, roots, followed and unfollowed calls on both S3
# devices, and the two lines this scan called stale (`icons::char`, classic's
# `SettingIdentity::secret`) were the two CI reported (todo 642). So remove what the scan
# calls stale. `CHECK_S3_BUDGET_TOLERATE_STALE=1` brings the old behaviour back (the
# script prints the lines and exits 0 when they are all that is wrong).
#
# The toolchain and registry paths of CI's runner (`/home/runner/...`) still
# differ from this machine's, so a line may differ from CI's even so; a pass is
# a pass, a failure naming a function nothing of the change touches is worth a
# look at CI's run before the budget is edited. Regenerating a budget is done as
# `shells/xteink/frame-budget-s3.txt` says.
set -uo pipefail
ci_path=/home/runner/work/paperpaw/paperpaw

if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo "usage: $0 <worktree> [x4-pro|x4-classic|both]" >&2
    exit 2
fi
worktree="$(cd "$1" 2> /dev/null && pwd)" || { echo "check-s3-budget: no such directory: $1" >&2; exit 2; }
which_device="${2:-both}"
case "$which_device" in
    x4-pro) devices=(x4-pro) ;;
    x4-classic) devices=(x4-classic) ;;
    both) devices=(x4-pro x4-classic) ;;
    *) echo "usage: $0 <worktree> [x4-pro|x4-classic|both]" >&2; exit 2 ;;
esac
[[ -f "$worktree/shells/xteink/frame-budget-s3.txt" ]] \
    || { echo "check-s3-budget: $worktree is no PaperPaw worktree (no shells/xteink/frame-budget-s3.txt)" >&2; exit 2; }

# The Xtensa GCC and the `esp` toolchain, as `espup` leaves them.
# shellcheck disable=SC1090
[[ -f "$HOME/export-esp.sh" ]] && source "$HOME/export-esp.sh"
# shellcheck disable=SC1091
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
if ! cargo +esp --version > /dev/null 2>&1; then
    echo "check-s3-budget: no \`esp\` toolchain (espup install, then source ~/export-esp.sh)" >&2
    exit 2
fi

out="$worktree/target-budget"
mkdir -p "$out" || exit 2
rustflags='["-Z","unstable-options","-C","symbol-mangling-version=legacy"]'

echo "check-s3-budget: building paperpaw-sign"
(cd "$worktree" && env -u RUSTFLAGS CARGO_TARGET_DIR="$out/sign" cargo build --locked -q --release -p paperpaw-sign) \
    || { echo "check-s3-budget: paperpaw-sign did not build" >&2; exit 1; }
sign="$out/sign/release/paperpaw-sign"

# One run at a time; then the copy at the CI's path.
mkdir -p "$(dirname "$ci_path")" "$ci_path" || exit 2
exec 9> "$(dirname "$ci_path")/.check-s3-budget.lock"
flock 9
echo "check-s3-budget: syncing the worktree to $ci_path"
(cd "$worktree" && git ls-files -z --cached --others --exclude-standard) \
    | python3 -I -c '
import os, shutil, sys
src, dst = sys.argv[1], sys.argv[2]
keep = {".git", "shells/xteink/target"}
want = set()
for name in sys.stdin.buffer.read().split(b"\0"):
    name = os.fsdecode(name)
    if name and os.path.isfile(os.path.join(src, name)):
        want.add(name)
for name in sorted(want):
    s, d = os.path.join(src, name), os.path.join(dst, name)
    if os.path.islink(s):
        continue
    try:
        same = os.path.getsize(s) == os.path.getsize(d) and open(s, "rb").read() == open(d, "rb").read()
    except OSError:
        same = False
    if not same:
        os.makedirs(os.path.dirname(d), exist_ok=True)
        shutil.copyfile(s, d)
        shutil.copymode(s, d)
for root, dirs, files in os.walk(dst, topdown=True):
    rel = os.path.relpath(root, dst)
    dirs[:] = [x for x in dirs if os.path.normpath(os.path.join(rel, x)) not in keep]
    for f in files:
        r = os.path.normpath(os.path.join(rel, f))
        if r not in want:
            os.remove(os.path.join(root, f))
for root, dirs, files in os.walk(dst, topdown=False):
    rel = os.path.relpath(root, dst)
    if rel != "." and not os.listdir(root) and rel not in keep:
        os.rmdir(root)
' "$worktree" "$ci_path" \
    || { echo "check-s3-budget: cannot sync the worktree" >&2; exit 2; }
build_dir="$ci_path/shells/xteink/target"

status=0
for device in "${devices[@]}"; do
    budget=shells/xteink/frame-budget-s3.txt
    [[ "$device" == x4-classic ]] && budget=shells/xteink/frame-budget-s3-classic.txt
    if [[ -n "${CHECK_S3_BUDGET_LINKED:-}" ]]; then
        # With the linked plugins' own code rooted too (todo 624): the budget and
        # `include frame-budget-s3-linked.txt`, in a file beside them.
        { cat "$ci_path/$budget"; echo "include frame-budget-s3-linked.txt"; } > "$ci_path/shells/xteink/.scan-linked.txt"
        budget=shells/xteink/.scan-linked.txt
    fi
        echo "check-s3-budget: building the $device firmware ($(date +%H:%M:%S))"
    if ! (cd "$ci_path/shells/xteink" && env -u RUSTFLAGS CARGO_TARGET_DIR="$build_dir" \
        cargo +esp build --locked -j4 --release --target xtensa-esp32s3-none-elf --no-default-features \
        -Zbuild-std=core,alloc --features "$device,ppn5" \
        --config "target.xtensa-esp32s3-none-elf.rustflags=$rustflags"); then
        echo "check-s3-budget: $device: the firmware did not build" >&2
        status=1
        continue
    fi
    elf="$build_dir/xtensa-esp32s3-none-elf/release/paperpaw-xteink"
    if [[ -n "${CHECK_S3_BUDGET_KEEP:-}" ]]; then
        mkdir -p "$CHECK_S3_BUDGET_KEEP" && cp "$elf" "$CHECK_S3_BUDGET_KEEP/$device.elf" \
            || { echo "check-s3-budget: cannot keep the ELF in $CHECK_S3_BUDGET_KEEP" >&2; exit 2; }
    fi
    echo "check-s3-budget: scanning $device ($(date +%H:%M:%S))"
    (cd "$ci_path" && "$sign" frames-s3 "$elf" --budget "$budget") > "$out/$device.scan.txt" 2>&1
    code=$?
    head -c 3000 "$out/$device.scan.txt"
    if [[ $code -eq 0 ]]; then
        echo "check-s3-budget: $device: the budget holds"
    elif [[ -n "${CHECK_S3_BUDGET_TOLERATE_STALE:-}" ]] \
        && [[ -z "$(grep -E '^  ' "$out/$device.scan.txt" | grep -v 'matches nothing any more')" ]] \
        && grep -q 'matches nothing any more' "$out/$device.scan.txt"; then
        echo "check-s3-budget: $device: the budget holds, but with stale \`allow\` lines (tolerated: remove them)"
    else
        echo "check-s3-budget: $device: FAILED ($code); the full scan is $out/$device.scan.txt" >&2
        status=1
    fi
done
echo "check-s3-budget: the shared build directory $build_dir is $(du -sh "$build_dir" 2> /dev/null | cut -f1) (delete it when the disk is tight)"
exit $status

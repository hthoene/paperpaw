#!/usr/bin/env bash
# The checks to run before pushing a change to some crates (decision 130:
# GitHub's CI is the full check; this is the quick, local part).
#
#   tools/check-local.sh [options] [crate ...]
#
# Crates are Cargo package names (paperpaw-plugin-keys, paperpaw-sdk, ...);
# none given: the packages with files changed against the merge base with
# origin/abi7/integration (--base REV for another base).
#
# Steps, in this order: fmt, clippy (-D warnings), test, doc (-D warnings),
# changelog, lockfiles, licences, checks (paperpaw-checks), abi-metadata.
# Every step runs whatever the one before did; each one's own exit code is
# what counts. The summary at the end lists them, and the script exits 1
# if any step failed (or could not run), 0 only if all passed. Nothing is
# piped through grep: read the full log of a failed step under the log
# directory it prints.
#
# Options:
#   --features LIST   also clippy and test with --features LIST (e.g. linked)
#   --only a,b,c      run only these steps
#   --skip a,b,c      skip these steps
#   --online          let lockfiles use the network (default: --offline)
#   --base REV        base for finding the changed packages
#   --jobs N          cargo -j (default 4)
#
# Run it from a worktree root, with CARGO_TARGET_DIR of that worktree.
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 2

features=""
only=""
skip=""
lock_args="--offline"
base="origin/abi7/integration"
jobs=4
crates=()
while (($#)); do
    case "$1" in
        --features) features="${2:?--features needs a list}"; shift 2 ;;
        --only) only=",${2:?--only needs a list},"; shift 2 ;;
        --skip) skip=",${2:?--skip needs a list},"; shift 2 ;;
        --online) lock_args=""; shift ;;
        --base) base="${2:?--base needs a revision}"; shift 2 ;;
        --jobs) jobs="${2:?--jobs needs a number}"; shift 2 ;;
        -h|--help) sed -n '2,/^set -uo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "check-local: unknown option $1" >&2; exit 2 ;;
        *) crates+=("$1"); shift ;;
    esac
done

[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
if [[ -z "${RUSTC_WRAPPER:-}" && -x tools/sccache-wrap.sh ]] && command -v sccache > /dev/null; then
    export RUSTC_WRAPPER="$root/tools/sccache-wrap.sh"
fi

# The packages changed against the base: the nearest Cargo.toml above each file.
changed_packages() {
    local merge_base
    merge_base="$(git merge-base "$base" HEAD 2> /dev/null)" || return 0
    { git diff --name-only "$merge_base"; git ls-files --others --exclude-standard; } | python3 -I -c '
import re, sys
from pathlib import Path
names = []
for line in sys.stdin:
    for parent in Path(line.strip()).parents:
        manifest = Path(parent, "Cargo.toml")
        if not manifest.is_file():
            continue
        text = manifest.read_text(encoding="utf-8")
        name = re.search(r"^\[package\][^\[]*?^name\s*=\s*\"([^\"]+)\"", text, re.M | re.S)
        # A manifest with a [workspace] of its own (shells/xteink, samples) is no
        # member of the root workspace: -p cannot name it.
        if name and not re.search(r"^\[workspace\]", text, re.M) and name.group(1) not in names:
            names.append(name.group(1))
        break
print(" ".join(names))
'
}

if ((${#crates[@]} == 0)); then
    read -r -a crates <<< "$(changed_packages)"
fi
pkgs=()
for crate in "${crates[@]}"; do pkgs+=(-p "$crate"); done
echo "check-local: packages: ${crates[*]:-(none: workspace-wide steps only)}"

logs="$(mktemp -d "${TMPDIR:-/tmp}/check-local.XXXXXX")"
echo "check-local: logs in $logs"

names=()
results=()
seconds=()
failed=0

wanted() {
    [[ -z "$only" || "$only" == *",$1,"* ]] && [[ "$skip" != *",$1,"* ]]
}

# step NAME COMMAND [ARGS...]: runs it, shows its output and keeps it in a log,
# records the command's own exit code (pipefail and PIPESTATUS: tee's does not count).
step() {
    local name="$1"
    shift
    wanted "$name" || return 0
    local log="$logs/$name.log" started=$SECONDS code
    echo "== $name: $*"
    "$@" 2>&1 | tee "$log"
    code=${PIPESTATUS[0]}
    names+=("$name")
    seconds+=("$((SECONDS - started))")
    if ((code == 0)); then
        results+=("ok")
    else
        results+=("FAILED (exit $code)")
        failed=1
    fi
}

has_packages() { ((${#pkgs[@]} > 0)); }

if has_packages; then
    step fmt cargo fmt "${pkgs[@]}" -- --check
    step clippy cargo clippy -j"$jobs" "${pkgs[@]}" --all-targets -- -D warnings
    [[ -n "$features" ]] \
        && step clippy-features cargo clippy -j"$jobs" "${pkgs[@]}" --features "$features" --all-targets -- -D warnings
    step test cargo test -j"$jobs" "${pkgs[@]}"
    [[ -n "$features" ]] \
        && step test-features cargo test -j"$jobs" "${pkgs[@]}" --features "$features"
    step doc env RUSTDOCFLAGS="-D warnings" cargo doc -j"$jobs" --no-deps "${pkgs[@]}"
fi
step changelog python3 tools/changelog.py check
# shellcheck disable=SC2086 # lock_args is one option or nothing
step lockfiles python3 tools/check-lockfiles.py $lock_args
step licences python3 tools/licences/build.py --check
# paperpaw-checks bakes the repository path: no compiler cache for it.
touch tools/checks/src/lib.rs 2> /dev/null || true
step checks env SCCACHE_DISABLE=1 cargo test -j"$jobs" -p paperpaw-checks
[[ -f tools/tests/test_abi_plugin_metadata.py ]] \
    && step abi-metadata python3 tools/tests/test_abi_plugin_metadata.py

echo
echo "check-local summary:"
for i in "${!names[@]}"; do
    printf '  %-16s %-18s %4ss\n' "${names[$i]}" "${results[$i]}" "${seconds[$i]}"
done
if ((${#names[@]} == 0)); then
    echo "check-local: no step ran (--only/--skip left nothing)" >&2
    exit 2
fi
if ((failed)); then
    echo "check-local: FAILED; the logs are in $logs" >&2
    exit 1
fi
echo "check-local: all ${#names[@]} steps passed"

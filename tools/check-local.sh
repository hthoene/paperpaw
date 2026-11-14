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
# Steps, in this order: fmt (and, for changed .rs files there, fmt-shells
# for shells/xteink and fmt-fuzz for fuzz: rustfmt only), clippy (-D warnings), test,
# test-linked (the tests of each package that has a `linked` feature, run with
# it: the hosted tests there are built nowhere else), test-native (the app's
# flows, sleep_parity and sleep_cycles with the linked reader and formats, when the core, the
# app, the native layer or a plugin changed, or with --all-linked), doc (-D warnings),
# changelog, lockfiles, licences, checks (paperpaw-checks), abi-metadata.
# Every step runs whatever the one before did; each one's own exit code is
# what counts. The summary at the end lists them, and the script exits 1
# if any step failed (or could not run), 0 only if all passed. Nothing is
# piped through grep: read the full log of a failed step under the log
# directory it prints. What was NOT checked is listed in the summary too:
# steps left out by --only/--skip, package steps when no package changed, and
# changed files in workspaces of their own (shells/xteink, fuzz, patches,
# samples ...), which this script does not build. Exit 2: bad usage, an
# unknown step name, or a base revision that is not there.
#
# Options:
#   --features LIST   also clippy and test with --features LIST (e.g. linked)
#   --all-linked      test-linked for every package with a `linked` feature,
#                     not only the changed ones (the train smoke)
#   --only a,b,c      run only these steps
#   --skip a,b,c      skip these steps
#   --online          let lockfiles use the network (default: --offline)
#   --base REV        base for finding the changed packages
#   --jobs N          cargo -j (default 4)
#
# Run the tools/check-local.sh of the worktree to check, from inside it, with
# CARGO_TARGET_DIR of that worktree. A copy elsewhere is refused (exit 2).
set -uo pipefail

# Only the script of the tree it checks counts: a copy elsewhere (or an empty
# one) can be stale or "pass" without checking anything. Exit 2 if this file is
# not tools/check-local.sh of the worktree it is run in.
root="$(git rev-parse --show-toplevel 2> /dev/null)" \
    || { echo "check-local: not inside a git worktree" >&2; exit 2; }
root="$(cd "$root" && pwd -P)" || exit 2
self="$(readlink -f "${BASH_SOURCE[0]}")"
if [[ "$self" != "$root/tools/check-local.sh" ]]; then
    echo "check-local: this is '$self', not tools/check-local.sh of the worktree '$root'." >&2
    echo "check-local: merge origin/abi7/integration and run the in-tree script; delete copies." >&2
    exit 2
fi
cd "$root" || exit 2

features=""
only=""
skip=""
lock_args="--offline"
base="origin/abi7/integration"
jobs=4
crates=()
all_linked=0
known=" fmt fmt-shells fmt-fuzz clippy clippy-features test test-features test-linked test-native doc changelog lockfiles licences checks abi-metadata "

check_names() { # LIST OPTION: every name must be a known step
    local name
    for name in $(tr ',' ' ' <<< "$1"); do
        [[ "$known" == *" $name "* ]] || { echo "check-local: unknown step '$name' for $2 (known:$known)" >&2; exit 2; }
    done
}

while (($#)); do
    case "$1" in
        --features) features="${2:?--features needs a list}"; shift 2 ;;
        --only) check_names "${2:?--only needs a list}" --only; only=",$2,"; shift 2 ;;
        --skip) check_names "${2:?--skip needs a list}" --skip; skip=",$2,"; shift 2 ;;
        --all-linked) all_linked=1; shift ;;
        --online) lock_args=""; shift ;;
        --base) base="${2:?--base needs a revision}"; shift 2 ;;
        --jobs) jobs="${2:?--jobs needs a number}"; shift 2 ;;
        -h|--help) sed -n '2,/^set -uo/p' "$self" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "check-local: unknown option $1" >&2; exit 2 ;;
        *) crates+=("$1"); shift ;;
    esac
done

[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
if [[ -z "${RUSTC_WRAPPER:-}" && -x tools/sccache-wrap.sh ]] && command -v sccache > /dev/null; then
    export RUSTC_WRAPPER="$root/tools/sccache-wrap.sh"
fi

logs="$(mktemp -d "${TMPDIR:-/tmp}/check-local.XXXXXX")" || exit 2
not_checked=()
warnings=()
detected=()
merge_base=""
explicit=0
((${#crates[@]} > 0)) && explicit=1
if ! git rev-parse --verify -q "$base^{commit}" > /dev/null; then
    if ((${#crates[@]} == 0)); then
        echo "check-local: base '$base' not found, so the changed packages are unknown (git fetch, or --base REV, or name the crates)" >&2
        exit 2
    fi
    warnings+=("base '$base' not found: no changelog fragment check")
elif ! merge_base="$(git merge-base "$base" HEAD 2> /dev/null)"; then
    echo "check-local: no merge base of '$base' and HEAD" >&2
    exit 2
fi

# The changed files against the merge base, and the packages and workspaces
# of their own among them. A file belongs to the nearest Cargo.toml above it;
# the manifests of `cargo metadata --no-deps` are the root workspace's members.
if [[ -n "$merge_base" ]]; then
    # Through files, not arguments: the metadata of this repository is far
    # beyond the 128 KiB a single argument may have. Any failure here is exit 2:
    # an unknown package set is not an empty one.
    { git diff --name-only "$merge_base"; git ls-files --others --exclude-standard; } | sort -u > "$logs/changed.txt" \
        || { echo "check-local: git could not list the changed files" >&2; exit 2; }
    cargo metadata --no-deps --format-version 1 > "$logs/metadata.json" 2> "$logs/metadata.err" \
        || { echo "check-local: cargo metadata failed (see $logs/metadata.err)" >&2; exit 2; }
    python3 -I - "$logs/metadata.json" "$logs/changed.txt" > "$logs/classified.txt" 2> "$logs/classified.err" <<'PY' \
        || { echo "check-local: could not classify the changed files (see $logs/classified.err)" >&2; exit 2; }
import json, sys
from pathlib import Path
with open(sys.argv[1], encoding="utf-8") as stream:
    metadata = json.load(stream)
members = {}
for package in metadata["packages"]:
    members[str(Path(package["manifest_path"]).parent)] = package["name"]
root = Path.cwd()
packages, separate = [], []
with open(sys.argv[2], encoding="utf-8") as stream:
    for line in stream:
        if not line.strip():
            continue
        path = root / line.strip()
        for parent in path.parents:
            if parent == root.parent:
                break
            if (parent / "Cargo.toml").is_file():
                if str(parent) in members:
                    if members[str(parent)] not in packages:
                        packages.append(members[str(parent)])
                elif parent != root:
                    where = str(parent.relative_to(root))
                    if where not in separate:
                        separate.append(where)
                break
for name in packages:
    print("PKG", name)
for where in separate:
    print("SEPARATE", where)
PY
    while read -r kind value; do
        case "$kind" in
            PKG) detected+=("$value") ;;
            SEPARATE) not_checked+=("NOT CHECKED: $value/ (separate workspace; build it there)") ;;
        esac
    done < "$logs/classified.txt"
fi
if ((explicit == 0)) && ((${#detected[@]} > 0)); then
    crates=("${detected[@]}")
fi
pkgs=()
for crate in "${crates[@]}"; do pkgs+=(-p "$crate"); done
echo "check-local: packages: ${crates[*]:-(none: workspace-wide steps only)}"
for line in "${not_checked[@]}"; do echo "check-local: $line"; done

# A changed crate wants a changelog fragment (or a No-changelog trailer).
if [[ -n "$merge_base" ]] && ((${#crates[@]} > 0)); then
    fragments="$({ git diff --name-only --diff-filter=A "$merge_base" -- changelog.d; git ls-files --others --exclude-standard -- changelog.d; } | grep -v 'README.md' || true)"
    [[ -n "$fragments" ]] || warnings+=("crates changed but no new changelog.d fragment (add one, or a No-changelog: <reason> trailer on the head commit)")
fi

echo "check-local: logs in $logs"

names=()
results=()
seconds=()
skipped=()
failed=0

wanted() {
    [[ -z "$only" || "$only" == *",$1,"* ]] && [[ "$skip" != *",$1,"* ]]
}

# step NAME COMMAND [ARGS...]: runs it, shows its output and keeps it in a log,
# records the command's own exit code (pipefail and PIPESTATUS: tee's does not count).
step() {
    local name="$1"
    shift
    if ! wanted "$name"; then
        skipped+=("$name (left out by --only/--skip)")
        return 0
    fi
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

# pkg_step NAME COMMAND...: a step on the packages; none to check is a skip, said so.
pkg_step() {
    local name="$1"
    if ((${#pkgs[@]} == 0)); then
        if wanted "$name"; then
            skipped+=("$name (no package changed or named)")
        else
            skipped+=("$name (left out by --only/--skip)")
        fi
        return 0
    fi
    step "$@"
}

pkg_step fmt cargo fmt "${pkgs[@]}" -- --check
# The firmware and the fuzz targets are workspaces of their own, which
# `cargo fmt` here does not reach; CI runs rustfmt on the firmware's crate root
# (it follows the modules), so the same here when a file of it changed.
shell_changed=""
fuzz_files=()
if [[ -f "$logs/changed.txt" ]]; then
    grep -q '^shells/xteink/.*\.rs$' "$logs/changed.txt" && shell_changed=1
    while IFS= read -r changed_file; do
        [[ -f "$changed_file" ]] && fuzz_files+=("$changed_file")
    done < <(grep '^fuzz/.*\.rs$' "$logs/changed.txt" || true)
fi
if [[ -n "$shell_changed" ]]; then
    step fmt-shells rustfmt --edition 2024 --check shells/xteink/src/main.rs
else
    skipped+=("fmt-shells (no changed .rs file under shells/xteink, or no base to compare)")
fi
if ((${#fuzz_files[@]} > 0)); then
    step fmt-fuzz rustfmt --edition 2024 --check "${fuzz_files[@]}"
else
    skipped+=("fmt-fuzz (no changed .rs file under fuzz, or no base to compare)")
fi
pkg_step clippy cargo clippy -j"$jobs" "${pkgs[@]}" --all-targets -- -D warnings
if [[ -n "$features" ]]; then
    pkg_step clippy-features cargo clippy -j"$jobs" "${pkgs[@]}" --features "$features" --all-targets -- -D warnings
fi
pkg_step test cargo test -j"$jobs" "${pkgs[@]}"
if [[ -n "$features" ]]; then
    pkg_step test-features cargo test -j"$jobs" "${pkgs[@]}" --features "$features"
fi
# The hosted tests of the plugins (tests/hosted_*.rs, test-support/hosted.rs)
# are built only with --features linked, and CI does not build them: run every
# selected package's tests with it where the feature exists (one cargo call).
# Already covered when --features names linked.
linked_pkgs=()
if [[ -f "$logs/metadata.json" && ",$features," != *",linked,"* ]]; then
    while read -r name; do linked_pkgs+=(-p "$name"); done < <(
        python3 -I - "$logs/metadata.json" "$all_linked" "${crates[@]}" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as stream:
    metadata = json.load(stream)
every = sys.argv[2] == "1"
chosen = set(sys.argv[3:])
for package in metadata["packages"]:
    if "linked" in package["features"] and (every or package["name"] in chosen):
        print(package["name"])
PY
    )
fi
if ((${#linked_pkgs[@]} > 0)); then
    step test-linked cargo test -j"$jobs" "${linked_pkgs[@]}" --features linked
elif ! wanted test-linked; then
    skipped+=("test-linked (left out by --only/--skip)")
else
    skipped+=("test-linked (no selected package has a linked feature)")
fi
# The app's flows and sleep tests, linked_catalogs and linked_settings_tables
# with the linked native plugins (#248): the workspace test run leaves
# `native-linked` off, and they need it; linked_catalogs also with what the
# X4 links (#258: no store plugins).
native_wanted="$all_linked"
for crate in "${crates[@]}"; do
    case "$crate" in
        paperpaw-core | paperpaw-app | paperpaw-native | paperpaw-plugin-*) native_wanted=1 ;;
    esac
done
if [[ "$native_wanted" == 1 ]]; then
    step test-native bash -c 'set -e
        cargo test -j"$1" -p paperpaw-app --features native-linked,native-linked-reading --test flows --test sleep_parity --test sleep_cycles --test linked_catalogs
        cargo test -j"$1" -p paperpaw-app --no-default-features --features native-linked-reading,packs --test linked_catalogs
        cargo test -j"$1" -p paperpaw-app --features native-linked-reading --test linked_settings_tables
        cargo test -j"$1" -p paperpaw-app --features native-linked,native-linked-store --test linked_settings_tables' _ "$jobs"
elif ! wanted test-native; then
    skipped+=("test-native (left out by --only/--skip)")
else
    skipped+=("test-native (no core, app, native or plugin package selected)")
fi
pkg_step doc env RUSTDOCFLAGS="-D warnings" cargo doc -j"$jobs" --no-deps "${pkgs[@]}"
step changelog python3 tools/changelog.py check
# shellcheck disable=SC2086 # lock_args is one option or nothing
step lockfiles python3 tools/check-lockfiles.py $lock_args
step licences python3 tools/licences/build.py --check
# paperpaw-checks bakes the repository path: no compiler cache for it.
touch tools/checks/src/lib.rs 2> /dev/null || true
step checks env SCCACHE_DISABLE=1 cargo test -j"$jobs" -p paperpaw-checks
if [[ -f tools/tests/test_abi_plugin_metadata.py ]]; then
    step abi-metadata python3 tools/tests/test_abi_plugin_metadata.py
fi

echo
echo "check-local summary:"
for i in "${!names[@]}"; do
    printf '  %-16s %-18s %4ss\n' "${names[$i]}" "${results[$i]}" "${seconds[$i]}"
done
for line in "${skipped[@]}"; do echo "  SKIPPED: $line"; done
for line in "${not_checked[@]}"; do echo "  $line"; done
for line in "${warnings[@]}"; do echo "  WARNING: $line"; done
if ((${#names[@]} == 0)); then
    echo "check-local: no step ran (--only/--skip left nothing)" >&2
    exit 2
fi
if ((failed)); then
    echo "check-local: FAILED; the logs are in $logs" >&2
    exit 1
fi
echo "check-local: all ${#names[@]} steps passed"

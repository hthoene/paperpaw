#!/usr/bin/env bash
# The quick check of a branch before it is handed over (#328): the format
# check every branch needs, plus the few targeted checks that would have caught
# the causes of the red bases of Nov 18, each picked by the files the branch
# changes against origin/abi7/integration. It is not the full CI (the firmware
# builds, the S3 band and the fuzzers stay there) and not tools/check-local.sh
# (which checks whole packages and their dependents): it runs what a change of
# these files can break in a few minutes on a warm build directory.
#
#   tools/quick-check.sh [--base REV] [--all] [--list] [--skip a,b] [--only a,b]
#
# Steps (the files that trigger each one in brackets):
#   fmt       always: cargo fmt --all --check and shells/xteink's rustfmt check
#   clippy    the changed packages, -D warnings           [any .rs/Cargo.toml of a package]
#   doc       the workspace's API docs, -D warnings (CI's)   [a package with a library]
#   interface the SDK's interface test: what plugins see changes only with the
#             interface (14 red runs)                     [the crates of crates.txt, abi/, the linker script]
#   checks    paperpaw-checks' tests: icons, text, sources [plugins/, tools/checks, ui-kit, fonts]
#   libs      the plugin libraries build alone, every feature set (native-entry) [changed plugins, ui-kit]
#   desktop   the emulator's tests as CI builds them (the workspace's feature
#             set with paperpaw-core/testing: it adds ~2.1 KiB to every peak):
#             help_fits (a setting's description in its row), the update notice
#             and the admission refusal                   [plugins/, i18n, ui-kit, app, core, sdk, shells/desktop]
#   memory    the memory ceilings (large PNG, Thai, Hindi) and the refused-allocation tests
#                                                         [text, image, layout, document, epub-family, hyphen, zip, xml, core, shells/desktop, the reading plugins]
#   app       the app's goldens: registry_golden, linked_catalogs, bands (CI's native-linked feature set)
#                                                         [plugins/, app, core, sdk, ui-kit]
#   frames    the changed plugins (at most 4; `store` stands in when a library under them changed) built for the X4's
#             C3 as CI builds them and packed: a frame of 2,048 B or more is refused  [plugins/, ui-kit, sdk, core native, text, layout]
# Every step runs whatever the one before did; the summary lists them with their
# times, and the script exits 1 if any failed, 0 if all passed. Logs: the
# directory it prints. NOT checked here: the S3 band and `allow indirect`
# (a full Xtensa firmware build), the C3/S3 firmware size, the store fit, the
# screens job, the other shells, the fuzzers.
#
# --all runs every step (for a measurement or after a base merge); --list says
# what would run and why; --only/--skip take step names. Use the build directory
# of the worktree (CARGO_TARGET_DIR) so the second run is warm. Exit 2: usage.
set -uo pipefail

root="$(git rev-parse --show-toplevel 2> /dev/null)" || { echo "quick-check: not inside a git worktree" >&2; exit 2; }
cd "$root" || exit 2
case ":$PATH:" in *":$HOME/.cargo/bin:"*) ;; *) PATH="$HOME/.cargo/bin:$PATH" ;; esac

base=origin/abi7/integration
all=0 list=0 only="" skip=""
steps=" fmt clippy doc interface checks libs desktop memory app frames "
while (($#)); do
    case "$1" in
        --base) base="${2:?--base needs a revision}"; shift 2 ;;
        --all) all=1; shift ;;
        --list) list=1; shift ;;
        --only) only="${2:?--only needs a list}"; shift 2 ;;
        --skip) skip="${2:?--skip needs a list}"; shift 2 ;;
        -h | --help) sed -n '2,/^set -uo/p' "$0" | sed '$d'; exit 0 ;;
        *) echo "quick-check: unknown option '$1'" >&2; exit 2 ;;
    esac
done
for name in $(tr ',' ' ' <<< "$only,$skip"); do
    [[ "$steps" == *" $name "* ]] || { echo "quick-check: unknown step '$name' (known:$steps)" >&2; exit 2; }
done
mb="$(git merge-base HEAD "$base" 2> /dev/null)" || { echo "quick-check: no base '$base' (git fetch?)" >&2; exit 2; }

# The files this branch changes: committed, uncommitted and new.
files="$( { git diff --name-only "$mb"; git ls-files -o --exclude-standard; } | sort -u)"
[[ -n "$files" || $all = 1 ]] || { echo "quick-check: no changed file against $base; nothing to check"; exit 0; }

# The packages of the changed files (the longest manifest directory that holds
# the file), and which of them are plugins.
meta="$(cargo metadata --format-version 1 --no-deps --offline 2> /dev/null)" || { echo "quick-check: cargo metadata failed" >&2; exit 2; }
picked="$(FILES="$files" ROOT="$root" python3 -I -c '
import json, os, sys
root = os.environ["ROOT"] + "/"
pkgs = []
for p in json.load(sys.stdin)["packages"]:
    d = p["manifest_path"][len(root):].rsplit("/", 1)[0] if p["manifest_path"].startswith(root) else None
    if d is not None:
        pkgs.append((d, p["name"], any(t["kind"] and ("lib" in t["kind"] or "rlib" in t["kind"]) for t in p["targets"])))
pkgs.sort(key=lambda x: -len(x[0]))
seen = {}
for f in os.environ["FILES"].split("\n"):
    for d, n, lib in pkgs:
        if f.startswith(d + "/"):
            seen[n] = (d, lib)
            break
for n, (d, lib) in sorted(seen.items()):
    print(n, d, int(lib))
' <<< "$meta")"
pkg_names=() pkg_libs=() plugin_ids=()
while read -r n d lib; do
    [[ -n "$n" ]] || continue
    pkg_names+=("$n")
    ((lib)) && pkg_libs+=("$n")
    [[ "$d" == plugins/* && -f "tools/plugin-files7/src/bin/${d#plugins/}.rs" ]] && plugin_ids+=("${d#plugins/}")
done <<< "$picked"

if ((all && ${#pkg_names[@]} == 0)); then pkg_names=(paperpaw-sdk paperpaw-emulator); pkg_libs=(paperpaw-sdk); fi

touches() { grep -Eq "$1" <<< "$files"; }
rs_pkg_change() { touches '(\.rs|Cargo\.toml)$'; }
# Which steps run, and why.
declare -A why
if [[ $all = 1 ]]; then for s in $steps; do why[$s]="--all"; done; else
    why[fmt]="always"
    ((${#pkg_names[@]})) && rs_pkg_change && why[clippy]="changed: ${pkg_names[*]}"
    ((${#pkg_libs[@]})) && rs_pkg_change && why[doc]="changed: ${pkg_libs[*]}"
    touches '^crates/(paperpaw-(sdk|platform|graphics|i18n|config|text|document|layout|xml|zip|hyphen|image|update|digest|json|qr|native|lang|sort))/|^tools/plugin-files7/paperpaw-plugin7\.x$|^abi/|^tools/interface' && why[interface]="an interface crate or file changed"
    touches '^(plugins/|tools/checks/|crates/paperpaw-ui-kit/|tools/fonts|crates/paperpaw-plugin-fonts)' && why[checks]="plugins, ui-kit or the checks changed"
    touches '^(plugins/|crates/paperpaw-ui-kit/)' && why[libs]="plugin libraries or ui-kit changed"
    touches '^(plugins/|crates/paperpaw-(i18n|ui-kit|app|core|sdk|native|config)/|shells/desktop/)' && why[desktop]="plugins, i18n, ui-kit, app, core, SDK or the emulator changed"
    touches '^(crates/paperpaw-(text|image|layout|document|epub-family|hyphen|zip|xml|core|native|json|digest)/|shells/desktop/|plugins/(reading|format-|picture|home|library|store))' && why[memory]="a crate on the reading path or the emulator changed"
    touches '^(plugins/|crates/paperpaw-(app|core|sdk|ui-kit|native)/)' && why[app]="plugins, app, core, SDK or ui-kit changed"
    touches '^(plugins/|crates/paperpaw-(ui-kit|sdk|core|native|text|layout|i18n)/|tools/plugin-files7/)' && why[frames]="plugin code or what it builds with changed"
fi
run_step() { # NAME: should it run?
    local s="$1"
    [[ -n "${why[$s]:-}" ]] || return 1
    [[ -z "$only" || ",$only," == *",$s,"* ]] || return 1
    [[ ",$skip," != *",$s,"* ]] || return 1
}
if ((list)); then
    for s in $steps; do run_step "$s" && echo "$s: ${why[$s]}"; done
    exit 0
fi

logs="$(mktemp -d "${TMPDIR:-/tmp}/quick-check.XXXXXX")"
echo "quick-check: base $base ($(wc -l <<< "$files") changed files); logs in $logs"
jobs_n="${QUICK_CHECK_JOBS:-8}"
results=() failed=0
step() { # NAME COMMAND...
    local name="$1"; shift
    local t0=$SECONDS rc
    echo "--- $name: ${why[$name]}"
    "$@" > "$logs/$name.log" 2>&1
    rc=$?
    local dt=$((SECONDS - t0))
    if ((rc)); then
        failed=1
        results+=("FAIL  $name (${dt}s): $logs/$name.log")
        echo "FAILED $name:"; tail -n 40 "$logs/$name.log"
    else
        results+=("ok    $name (${dt}s)")
    fi
}
pk() { local a=(); for n in "$@"; do a+=(-p "$n"); done; printf '%s\n' "${a[@]}"; }

fmt_step() { cargo fmt --all -- --check && rustfmt --edition 2024 --check shells/xteink/src/main.rs; }
clippy_step() {
    # The emulator's tests name `paperpaw_core::testing`: CI's feature on core.
    local rc=0 plain=() n
    for n in "${pkg_names[@]}"; do [[ "$n" == paperpaw-emulator ]] || plain+=("$n"); done
    ((${#plain[@]})) && { mapfile -t a < <(pk "${plain[@]}"); cargo clippy -j"$jobs_n" "${a[@]}" --all-targets -- -D warnings || rc=1; }
    [[ " ${pkg_names[*]} " == *" paperpaw-emulator "* ]] \
        && { cargo clippy -j"$jobs_n" -p paperpaw-emulator --features paperpaw-core/testing --all-targets -- -D warnings || rc=1; }
    return $rc
}
# CI's `cargo doc --workspace --no-deps`, not the changed packages alone: with the workspace's
# features unified (the kernel turns the SDK's `kernel` feature on; text's `outline` is on)
# links resolve that a single package's docs cannot. Warm, only what changed is documented.
doc_step() { RUSTDOCFLAGS="-D warnings" cargo doc -j"$jobs_n" --workspace --no-deps; }
interface_step() { cargo test -j"$jobs_n" -p paperpaw-sdk --test interface; }
checks_step() { cargo test -j"$jobs_n" --no-fail-fast -p paperpaw-checks; }
libs_step() {
    local crates=() n
    for n in "${pkg_names[@]}"; do [[ "$n" == paperpaw-plugin-* || "$n" == paperpaw-ui-kit ]] && crates+=("$n"); done
    ((all)) && crates=()
    tools/ci/plugin-libs-alone.sh "${crates[@]}"
}
# CI's workspace selection and features (the job "Tests"): `testing` is on
# paperpaw-core there, so every peak carries what it costs.
ci_test() { cargo test -j"$jobs_n" --no-fail-fast --workspace --exclude paperpaw-app --features paperpaw-core/testing "$@"; }
desktop_step() {
    local rc=0
    ci_test --test help_fits --test update_notice_fits || rc=1
    cargo test -j"$jobs_n" --no-fail-fast -p paperpaw-emulator --features paperpaw-core/testing --lib admit || rc=1
    return $rc
}
memory_step() {
    local rc=0
    ci_test --test memory || rc=1
    cargo test -j"$jobs_n" --no-fail-fast -p paperpaw-epub-family --test allocation || rc=1
    return $rc
}
app_step() {
    cargo test -j"$jobs_n" --no-fail-fast -p paperpaw-app --features native-linked,native-linked-reading,native-linked-store \
        --test registry_golden --test linked_catalogs --test bands
}
frames_step() {
    local ids=("${plugin_ids[@]}") out
    ((${#ids[@]} == 0 || all)) && ids=(store)
    ((${#ids[@]} > 4)) && { echo "more than 4 changed plugins: the first 4 only"; ids=("${ids[@]:0:4}"); }
    out="$(mktemp -d "$logs/plugins.XXXXXX")"
    local id rc=0
    for id in "${ids[@]}"; do
        echo "== $id"
        tools/plugin-files7/build.sh x4 "$out" --only "$id" || rc=1
    done
    return $rc
}

for s in $steps; do
    run_step "$s" || continue
    step "$s" "${s}_step"
done

echo
echo "quick-check summary:"
printf '  %s\n' "${results[@]}"
((failed)) && { echo "quick-check: FAILED"; exit 1; }
echo "quick-check: passed"

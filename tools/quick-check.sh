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
#   fmt       always: cargo fmt --all --check and shells/xteink's rustfmt check; no tracked path starting with `-`
#   generated the generated files and tables are current, CI's `--check` runs of a few seconds: the author tool's
#             interface table (tools/abi-metadata-table.py: red on Nov 19, "Firmware author template"), abi-gen, the
#             metadata fixtures, NOTICE/licences, the lock files, the plugins' stage codecs   [always]
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
#   snapshots the emulator's screen snapshots (screens_look_as_before, screens_say_what_they_show, the
#             header, transfer and quick-panel layout tests) as CI builds them, and the app's own screens:
#             kernel_screens and screens (golden pictures and layout faults)
#                                                         [plugins/, app, ui-kit, layout, text, graphics, fonts, i18n, core, sdk, shells/desktop]
#   publish   the publisher's tests: its exempt list of plugins that name the SDK `kernel` feature must be
#             the plugins' own manifests (red twice on Nov 18)   [plugins/*/Cargo.toml, tools/publish]
#   wasm32    CI's "no_std targets (wasm32-unknown-unknown)" build, release, of the kernel with every first-party
#             plugin: for the changed packages that build for it, or the lock file [a crate or plugin under the app, Cargo.lock]
#   webemu   CI's "Lint the web emulator" (clippy -D warnings of paperpaw-web for wasm32: a function only the
#             native host reads is dead there)           [shells/desktop, shells/web, paperpaw-x4/core/sdk/emulator]
#   x4tests   the X4 crate's update tests (update_path, updates): the boards, partition tables and the constants
#             of shells/xteink/src/ota.rs they check      [crates/paperpaw-x4, shells/xteink, .github/actions/install-image]
#   frames    the changed plugins (at most 4; `store` stands in when a library under them changed) built for the X4's
#             C3 as CI builds them and packed: a frame of 2,048 B or more is refused  [plugins/, ui-kit, sdk, core native, text, layout]
#   frames-s3 the same plugins built for the X4 Pro / Classic (ESP32-S3, espup's `esp` toolchain,
#             `tools/plugin-files7/build.sh x4-pro`) and packed for target 9: a frame over the limit
#             that only the S3's code shows. Its own build directory under ~/.cache (QUICK_CHECK_S3_TARGET
#             overrides it) keeps the second run warm; the first one is cold (about 3 min for `store`)
# Every step runs whatever the one before did; the summary lists them with their
# times, and the script exits 1 if any failed, 0 if all passed. Logs: the
# directory it prints.
#
# It has to fit PlainBoard's 10 minutes, so (#328):
#  - six lanes run side by side, each with one cargo at a time (CARGO_BUILD_JOBS=8 each) and a build
#    directory of its own, so nothing waits on cargo's directory lock: the host lane (fmt, clippy,
#    interface, checks, libs), the tests lane (desktop, memory, app, snapshots), the doc lane (doc alone: a cold
#    `cargo doc --workspace` is 300-400 s on a busy machine, rustdoc is not cached by sccache, and behind
#    it wasm32 and webemu were cut for the budget on Nov 19), the wasm lane (wasm32, webemu, publish, x4tests), and one
#    lane each for the plugin frames, `frames` (C3) and `frames-s3` (Xtensa). On a cold directory
#    `frames-s3` alone takes ~2 min (115 s for `store`, #3864): behind doc and wasm32 it was the step the
#    budget cut, so the first run of a branch failed and the second, warm, passed;
#  - rustc goes through sccache (tools/sccache-wrap.sh, as the other agents use it; RUSTC_WRAPPER of the
#    environment wins) with incremental builds off: another worktree's build of the same crate, also of
#    the workspace's own, is a hit, so a fresh worktree is not a cold build. Build directories are not
#    shared between worktrees: cargo trusts file times, and two trees with other sources would reuse
#    each other's artifacts;
#  - a library change (ui-kit, SDK, core ...) builds the frames of `store` alone, the largest plugin
#    and the one at the limit; changed plugins are built themselves (at most 4);
#  - every step has a time limit (QUICK_CHECK_STEP_LIMIT, 440 s) and the whole run a budget
#    (QUICK_CHECK_BUDGET, 470 s: with the ~25 s of the python checks in front of it, the whole quick-check
#    command ends within 8.5 minutes, PlainBoard's limit being 10). A step that is cut, or not started for the budget, FAILS the run with
#    a message (exit 1): it checked nothing, and the build directories are warm for the next run. NOT checked here: the S3 `allow indirect` band
# (tools/check-s3-budget.sh: it needs a full Xtensa firmware build of the whole
# kernel, far beyond a quick check; CI and the landing check keep it), the
# C3/S3 firmware size, the store fit, the `paperpaw-dev visual`/`reach` screens
# job, the other shells, the fuzzers.
#
# --all runs every step (for a measurement or after a base merge); --list says
# what would run and why; --only/--skip take step names. Use the build directory
# of the worktree (CARGO_TARGET_DIR) so the second run is warm. Exit 2: usage.
set -uo pipefail

root="$(git rev-parse --show-toplevel 2> /dev/null)" || { echo "quick-check: not inside a git worktree" >&2; exit 2; }
cd "$root" || exit 2
case ":$PATH:" in *":$HOME/.cargo/bin:"*) ;; *) PATH="$HOME/.cargo/bin:$PATH" ;; esac

# Builds: sccache in front of rustc and no incremental builds (they do not mix with it).
export CARGO_INCREMENTAL=0
# The sccache store: with the default 8 GiB it was full, so it evicted the entries of the feature sets of
# the last base before the next worktree could hit them (Nov 19: 90 % misses, cold hand-overs cut on time).
# The server reads the size when it starts, so this only counts for the first compile after a restart.
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"
# A branch started before the wrapper landed has none: take it from the base. (A branch started before THIS
# script landed runs its own, older script, with no limits: run the base's copy instead,
#   git show origin/abi7/integration:tools/quick-check.sh > ~/qc.sh && bash ~/qc.sh )
t_start=$SECONDS
budget="${QUICK_CHECK_BUDGET:-470}"
step_limit="${QUICK_CHECK_STEP_LIMIT:-440}"

base=origin/abi7/integration
all=0 list=0 only="" skip=""
steps=" fmt generated clippy doc interface checks libs desktop memory app snapshots publish wasm32 webemu x4tests frames frames-s3 "
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

# CI's wasm32/riscv32 build: the app with the standard feature set and the libraries it names.
wasm_pkgs=(-p paperpaw-app --no-default-features --features packs,store-plugins,diagnostics
    -p paperpaw-image -p paperpaw-sqlite -p paperpaw-stardict -p paperpaw-x4 -p paperpaw-fat)
touches() { grep -Eq "$1" <<< "$files"; }
rs_pkg_change() { touches '(\.rs|Cargo\.toml)$'; }
# Which steps run, and why.
declare -A why
if [[ $all = 1 ]]; then for s in $steps; do why[$s]="--all"; done; else
    why[fmt]="always"
    why[generated]="always (a few seconds)"
    ((${#pkg_names[@]})) && rs_pkg_change && why[clippy]="changed: ${pkg_names[*]}"
    ((${#pkg_libs[@]})) && rs_pkg_change && why[doc]="changed: ${pkg_libs[*]}"
    touches '^crates/(paperpaw-(sdk|platform|graphics|i18n|config|text|document|layout|xml|zip|hyphen|image|update|digest|json|qr|native|lang|sort))/|^tools/plugin-files7/paperpaw-plugin7\.x$|^abi/|^tools/interface' && why[interface]="an interface crate or file changed"
    touches '^(plugins/|tools/checks/|crates/paperpaw-ui-kit/|tools/fonts|crates/paperpaw-plugin-fonts)' && why[checks]="plugins, ui-kit or the checks changed"
    touches '^(plugins/|crates/paperpaw-ui-kit/)' && why[libs]="plugin libraries or ui-kit changed"
    touches '^(plugins/|crates/paperpaw-(i18n|ui-kit|app|core|sdk|native|config)/|shells/desktop/)' && why[desktop]="plugins, i18n, ui-kit, app, core, SDK or the emulator changed"
    touches '^(crates/paperpaw-(text|image|layout|document|epub-family|hyphen|zip|xml|core|native|json|digest)/|shells/desktop/|plugins/(reading|format-|picture|home|library|store))' && why[memory]="a crate on the reading path or the emulator changed"
    touches '^(plugins/|crates/paperpaw-(app|core|sdk|ui-kit|native)/)' && why[app]="plugins, app, core, SDK or ui-kit changed"
    touches '^(plugins/|crates/paperpaw-(app|ui-kit|layout|text|graphics|fonts|i18n|core|sdk|native|config)/|shells/desktop/|tools/fonts)' && why[snapshots]="plugins, app, ui-kit, layout, text, fonts, i18n, core, SDK or the emulator changed"
    touches '^(plugins/[^/]+/Cargo\.toml|tools/publish/)' && why[publish]="a plugin manifest or the publisher changed"
    if touches '^Cargo\.lock$'; then why[wasm32]="Cargo.lock changed"
    elif ((${#pkg_names[@]})) && rs_pkg_change; then
        wasm_closure="$(cargo tree --offline --target wasm32-unknown-unknown --prefix none -e normal,build "${wasm_pkgs[@]}" 2> /dev/null | awk '/\(\/|\(\.\.?\//{print $1}' | sort -u)"
        wasm_hit="$(comm -12 <(printf '%s\n' "${pkg_names[@]}" | sort -u) <(printf '%s\n' "$wasm_closure") | tr '\n' ' ')"
        [[ -z "$wasm_hit" ]] || why[wasm32]="changed, in the no_std build: $wasm_hit"
    fi
    touches '^(shells/(desktop|web)/|crates/paperpaw-(x4|core|sdk|emulator)/)' && why[webemu]="the emulator, the web shell or a crate under them changed"
    touches '^(crates/paperpaw-x4/|shells/xteink/|\.github/actions/install-image/)' && why[x4tests]="the X4 crate or the firmware's constants and tables changed"
    touches '^(plugins/|crates/paperpaw-(ui-kit|sdk|core|native|text|layout|i18n)/|tools/plugin-files7/)' && why[frames]="plugin code or what it builds with changed"
    touches '^(plugins/|crates/paperpaw-(ui-kit|sdk|core|native|text|layout|i18n)/|tools/plugin-files7/)' && why[frames-s3]="plugin code or what it builds with changed (S3 frames differ from the C3's)"
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
if [[ -z "${RUSTC_WRAPPER:-}" ]]; then
    wrap="$root/tools/sccache-wrap.sh"
    if [[ ! -x "$wrap" ]]; then
        wrap="$logs/sccache-wrap.sh"
        git show "$base:tools/sccache-wrap.sh" > "$wrap" 2> /dev/null && chmod +x "$wrap" || wrap=""
    fi
    [[ -z "$wrap" ]] || export RUSTC_WRAPPER="$wrap"
fi
echo "quick-check: base $base ($(wc -l <<< "$files") changed files); logs in $logs"
jobs_n="${QUICK_CHECK_JOBS:-8}"
lane2_target="${CARGO_TARGET_DIR:-$root/target}-qc-lane2"
lane3_target="${CARGO_TARGET_DIR:-$root/target}-qc-lane3"
lane4_target="${CARGO_TARGET_DIR:-$root/target}-qc-lane4"
lane5_target="${CARGO_TARGET_DIR:-$root/target}-qc-lane5"
# One step: run in its own process group, cut after its time limit or when the total budget is used up.
# Writes "$logs/NAME.res": the summary line of the step, FAIL or ok.
step() { # NAME COMMAND...
    local name="$1"; shift
    local t0=$SECONDS rc=0 cut=0 limit="$step_limit" left=$((budget - (SECONDS - t_start)))
    echo "--- $name: ${why[$name]}"
    if ((left < 10)); then
        echo "FAIL  $name: NOT RUN, the budget of ${budget}s is used up (QUICK_CHECK_BUDGET); nothing was checked here" > "$logs/$name.res"
        echo "SKIPPED $name: the budget of ${budget}s is used up"
        return
    fi
    ((left < limit)) && limit=$left
    set -m
    "$@" > "$logs/$name.log" 2>&1 &
    local pid=$!
    set +m
    while kill -0 "$pid" 2> /dev/null; do
        if ((SECONDS - t0 >= limit)); then
            cut=1
            kill -TERM -- "-$pid" 2> /dev/null
            sleep 3
            kill -KILL -- "-$pid" 2> /dev/null
            break
        fi
        sleep 1
    done
    wait "$pid" 2> /dev/null
    rc=$?
    local dt=$((SECONDS - t0))
    if ((cut)); then
        echo "FAIL  $name: CUT after ${dt}s (limit ${limit}s, step limit ${step_limit}s, budget ${budget}s): nothing was checked here; the build directories are warm now, run again: $logs/$name.log" > "$logs/$name.res"
        echo "CUT $name after ${dt}s: $(tail -n 3 "$logs/$name.log" | tr '\n' ' ')"
    elif ((rc)); then
        echo "FAIL  $name (${dt}s): $logs/$name.log" > "$logs/$name.res"
        echo "FAILED $name:"; tail -n 40 "$logs/$name.log"
    else
        echo "ok    $name (${dt}s)" > "$logs/$name.res"
    fi
}
publish_step() { CARGO_TARGET_DIR="$lane2_target" cargo test -j"$jobs_n" -p paperpaw-publish; }
webemu_step() { CARGO_TARGET_DIR="$lane2_target" cargo clippy -j"$jobs_n" --target wasm32-unknown-unknown -p paperpaw-web -- -D warnings; }
x4tests_step() { CARGO_TARGET_DIR="$lane2_target" cargo test -j"$jobs_n" -p paperpaw-x4 --features paperpaw-core/testing --test update_path --test updates; }
# CI's job "no_std targets", its wasm32 leg (the riscv32 leg builds the same packages for the C3
# firmware, which `frames` covers by the plugins): the same command, in the build lane's directory.
wasm32_step() { CARGO_TARGET_DIR="$lane2_target" cargo build -j"$jobs_n" --release --target wasm32-unknown-unknown "${wasm_pkgs[@]}"; }
pk() { local a=(); for n in "$@"; do a+=(-p "$n"); done; printf '%s\n' "${a[@]}"; }

# A path starting with `-` is a tool that took `--help` for its output folder (#367).
fmt_step() {
    if git ls-files | grep -q '^-'; then git ls-files | grep '^-' | head -5; echo "a tracked path starts with '-' (#367)"; return 1; fi
    cargo fmt --all -- --check && rustfmt --edition 2024 --check shells/xteink/src/main.rs
}
# CI's generator checks that take seconds (they need cargo and rustfmt on the PATH, which is set above).
# NOT here: `tools/interface.py --check` (api.txt/shape.txt: a nightly build; the `interface` step's test is its cheap half)
# and the line-break table (it downloads the Unicode data).
generated_step() {
    local rc=0 c
    for c in "tools/abi-metadata-table.py --check" "tools/abi-gen.py --check" "tools/abi-metadata-fixtures.py --check" \
        "tools/licences/build.py --check" "tools/check-lockfiles.py" \
        "tools/stage-codec.py plugins/about/stages plugins/about/src/stages.rs --check" \
        "tools/stage-codec.py plugins/reader/stages plugins/reader/src/stages.rs --check --plugin reader" \
        "tools/stage-codec.py plugins/aids/stages plugins/aids/src/reader_filter/stages.rs --check --plugin reader" \
        "tools/stage-codec.py crates/paperpaw-sdk/stages/reader crates/paperpaw-sdk/src/native/page_layers/codec.rs --check --plugin reader" \
        "tools/stage-codec.py plugins/readwise/stages plugins/readwise/src/stages.rs --check" \
        "tools/stage-codec.py plugins/wikipedia/stages/subscribed plugins/wikipedia/src/stages.rs --check" \
        "tools/stage-codec.py plugins/joplin/stages plugins/joplin/src/stages.rs --check"; do
        # shellcheck disable=SC2086
        python3 $c || { echo "FAILED: python3 $c"; rc=1; }
    done
    cmp plugins/aids/stages/reader.filter@1.def plugins/reader/stages/reader.filter@1.def || rc=1
    for d in reader.text@1.def reader.layer@1.def; do
        cmp "crates/paperpaw-sdk/stages/reader/$d" "plugins/reader/stages/$d" || rc=1
    done
    cmp plugins/reader/stages/reader.notes-exported@1.def plugins/readwise/stages/reader.notes-exported@1.def || rc=1
    return $rc
}
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
doc_step() { CARGO_TARGET_DIR="$lane4_target" RUSTDOCFLAGS="-D warnings" cargo doc -j"$jobs_n" --workspace --no-deps; }
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
# The goldens of the emulator (every screen and layout fault of its snapshots.rs: it is a test of
# the same workspace build as `desktop`'s, so warm after it) and the app's own screens, in the
# feature set `app` builds (nothing more to compile).
snapshots_step() {
    local rc=0
    ci_test --test snapshots || rc=1
    cargo test -j"$jobs_n" --no-fail-fast -p paperpaw-app --features native-linked,native-linked-reading,native-linked-store \
        --test kernel_screens --test screens || rc=1
    return $rc
}
# Packs the changed plugins (DEVICE: x4 | x4-pro) into the out directory: the packer refuses a frame of 2,048 B or more.
frames_of() {
    local device="$1" ids=("${plugin_ids[@]}") out
    ((${#ids[@]} == 0 || all)) && ids=(store)
    ((${#ids[@]} > 4)) && { echo "more than 4 changed plugins: the first 4 only"; ids=("${ids[@]:0:4}"); }
    out="$(mktemp -d "$logs/plugins-$device.XXXXXX")"
    local id rc=0
    for id in "${ids[@]}"; do
        echo "== $id ($device)"
        tools/plugin-files7/build.sh "$device" "$out" --only "$id" || rc=1
    done
    return $rc
}
frames_step() { CARGO_TARGET_DIR="$lane3_target" frames_of x4; }
# One persistent build directory for the Xtensa plugin builds, per worktree (two trees with other
# sources must not share one: cargo would reuse the other's artifacts).
frames_s3_step() {
    local dir="${QUICK_CHECK_S3_TARGET:-$HOME/.cache/paperpaw-quick-check/s3-$(basename "$root")-$(printf %s "$root" | cksum | cut -d' ' -f1)}"
    mkdir -p "$dir" && echo "S3 build directory: $dir"
    # The Xtensa linker comes with espup's toolchain, which is not on PATH by itself
    # (espup's export-esp.sh does that; the toolchain's own folder is found here).
    if ! command -v xtensa-esp32s3-elf-gcc > /dev/null; then
        local gcc
        gcc="$(ls -d "$(rustc +esp --print sysroot 2> /dev/null)"/xtensa-esp-elf/*/xtensa-esp-elf/bin 2> /dev/null | tail -n 1)"
        if [[ -n "$gcc" ]]; then PATH="$gcc:$PATH"; elif [[ -f "$HOME/export-esp.sh" ]]; then . "$HOME/export-esp.sh"; fi
    fi
    CARGO_TARGET_DIR="$dir" frames_of x4-pro
}

# The lanes run side by side: each has one cargo at a time and its own build directory, so none waits
# on another's lock, and a slow step (a cold Xtensa build) is not queued behind the others.
lane() { local s; for s in "$@"; do run_step "$s" && step "$s" "${s//-/_}_step"; done; return 0; }
lane frames-s3 &
lane_s3=$!
lane frames &
lane_frames=$!
lane doc &
lane_doc=$!
lane wasm32 webemu publish x4tests &
lane_wasm=$!
# The tests of the workspace build (desktop, memory, snapshots) and the app's: the longest chain when cold, so
# a lane of their own (the one lane of all host steps ran 700 s cold while the others were done after 300).
(
    export CARGO_TARGET_DIR="$lane5_target"
    lane desktop memory app snapshots
) &
lane_tests=$!
lane fmt generated clippy interface checks libs
wait "$lane_s3" "$lane_frames" "$lane_doc" "$lane_wasm" "$lane_tests"

echo
echo "quick-check summary ($((SECONDS - t_start))s of ${budget}s):"
failed=0
for s in $steps; do
    [[ -f "$logs/$s.res" ]] || continue
    echo "  $(cat "$logs/$s.res")"
    [[ "$(cut -c1-2 "$logs/$s.res")" == ok ]] || failed=1
done
((failed)) && { echo "quick-check: FAILED"; exit 1; }
echo "quick-check: passed"

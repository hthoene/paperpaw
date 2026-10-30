#!/usr/bin/env bash
# The host-side checks of .github/workflows/ci.yml, in one command, so a
# change is checked here before it is pushed (CI minutes are limited):
#
#   tools/ci-local.sh [quick|full] [-j N] [--only <group>[,<group>...]] [--list] [-v]
#
#   quick   fmt, lint, test: what every change needs (the default)
#   full    every group below except `fuzz` and `firmware-build`
#   --only  just these groups (names as `--list` prints them)
#   -j N    cargo's parallel jobs (default 4; the container has 16 GB of RAM,
#           an out-of-memory kill takes every agent's work with it)
#   -v      show each step's output (otherwise it is kept in
#           target/ci-local/<step>.log, and the end of a failed step's is shown)
#
# Each step is a real command with its real exit code: a failing step is
# listed in the summary and the script exits 1, whatever the output looked
# like. RUSTFLAGS is `-D warnings` as in CI. A step whose tool is not
# installed is SKIPPED, with the reason, and listed; it is not a pass.
#
# CI jobs that cannot run here: the X4 Pro and X4 Classic firmware (Xtensa
# compiler, `esp` toolchain), the Windows and macOS emulator, the update path
# (needs the release before's image), Android's APK (needs the SDK), the
# reproducible build, CodeQL, release.
set -uo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if ! command -v cargo > /dev/null && [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi

mode=quick
jobs=4
only=""
verbose=0
list=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        quick | full) mode=$1 ;;
        -j) jobs=${2:?-j needs a number}; shift ;;
        -j*) jobs=${1#-j} ;;
        --only) only=${2:?--only needs groups}; shift ;;
        --list) list=1 ;;
        -v) verbose=1 ;;
        -h | --help) sed -n '2,/^set -uo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
    esac
    shift
done

export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS="$jobs"
export RUSTFLAGS="-D warnings"

all_groups=(fmt lint test doc msrv screens sdk embedded native architecture emulator deny firmware android)
quick_groups=(fmt lint test)
opt_groups=(firmware-build fuzz)

if [[ $list -eq 1 ]]; then
    echo "quick: ${quick_groups[*]}"
    echo "full:  ${all_groups[*]}"
    echo "only on request (--only): ${opt_groups[*]}"
    exit 0
fi

if [[ -n "$only" ]]; then
    IFS=, read -r -a groups <<< "$only"
elif [[ $mode == full ]]; then
    groups=("${all_groups[@]}")
else
    groups=("${quick_groups[@]}")
fi

logs="$root/target/ci-local"
mkdir -p "$logs"
passed=()
failed=()
skipped=()

# The firmware's Rust (as the CI action xteink-tools reads it).
x4_toolchain="$(sed -n 's/^channel = "\(.*\)"/\1/p' shells/xteink/rust-toolchain.toml)"
nightly="$(sed -n 's/^ *NIGHTLY: *\(.*\)$/\1/p' .github/workflows/ci.yml | head -1)"

have_toolchain() { rustup toolchain list 2> /dev/null | grep -q "^$1"; }
have_cmd() { command -v "$1" > /dev/null; }

# run <name> <command string>: the command in a shell of its own with
# `set -e -o pipefail`, its output to a log, its exit code counted.
run() {
    local name=$1 cmd=$2 log start status
    log="$logs/$(printf '%s' "$name" | tr -c 'A-Za-z0-9._-' _).log"
    start=$SECONDS
    if [[ $verbose -eq 1 ]]; then
        echo "--- $name"
        bash -e -o pipefail -c "$cmd" 2>&1 | tee "$log"
        status=${PIPESTATUS[0]}
    else
        bash -e -o pipefail -c "$cmd" > "$log" 2>&1
        status=$?
    fi
    if [[ $status -eq 0 ]]; then
        printf 'ok    %-58s %4ss\n' "$name" $((SECONDS - start))
        passed+=("$name")
    else
        printf 'FAIL  %-58s %4ss (exit %s)\n' "$name" $((SECONDS - start)) "$status"
        [[ $verbose -eq 1 ]] || tail -n 25 "$log" | sed 's/^/      | /'
        echo "      log: ${log#"$root"/}"
        failed+=("$name")
    fi
}

skip() {
    printf 'skip  %-58s %s\n' "$1" "$2"
    skipped+=("$1: $2")
}

# ---- the groups, in the order of ci.yml's jobs ---------------------------

g_fmt() {
    run "fmt: workspace" "cargo fmt --all --check"
    run "fmt: shells/xteink/src/main.rs" "rustfmt --edition 2024 --check shells/xteink/src/main.rs"
    run "fmt: tools/bluetooth-tests" "cargo fmt --manifest-path tools/bluetooth-tests/Cargo.toml --check"
}

g_lint() {
    run "clippy: workspace" "cargo clippy --workspace --all-targets"
    run "clippy: tools/bluetooth-tests" "cargo clippy --manifest-path tools/bluetooth-tests/Cargo.toml --locked --all-targets --target x86_64-unknown-linux-gnu -- -D warnings"
}

g_test() {
    run "test: workspace" "cargo test --workspace"
    run "test: tools/bluetooth-tests" "cargo test --manifest-path tools/bluetooth-tests/Cargo.toml --locked --target x86_64-unknown-linux-gnu"
    run "test: text and fonts without default features" "cargo test -p paperpaw-text -p paperpaw-plugin-fonts-default --no-default-features"
}

g_doc() {
    run "doc: no broken links" "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps"
}

g_msrv() {
    if ! have_toolchain 1.88; then
        skip "msrv: oldest Rust (1.88)" "toolchain not installed (rustup toolchain install 1.88)"
        return
    fi
    run "msrv: check workspace" "cargo +1.88 check --workspace --all-targets"
    run "msrv: bluetooth host tests" "cargo +1.88 test --manifest-path tools/bluetooth-tests/Cargo.toml --locked --target x86_64-unknown-linux-gnu"
}

g_screens() {
    run "screens: build paperpaw-dev" "cargo build --release -p paperpaw-dev"
    run "screens: every screen drawn" "target/release/paperpaw-dev visual --quick --network fixtures"
    run "screens: at the largest text" "target/release/paperpaw-dev visual --quick --network fixtures --set core.ui-size=24"
    run "screens: reached with the buttons" "target/release/paperpaw-dev reach --quick --network fixtures"
}

g_sdk() {
    if have_cmd cargo-semver-checks && have_toolchain "${nightly:-nightly}"; then
        run "sdk: semver-checks against crates.io" '
            packages=()
            for crate in paperpaw-sdk paperpaw-platform paperpaw-graphics paperpaw-i18n \
              paperpaw-config paperpaw-text paperpaw-document paperpaw-layout paperpaw-xml \
              paperpaw-zip paperpaw-hyphen paperpaw-image paperpaw-update paperpaw-digest \
              paperpaw-json paperpaw-qr paperpaw-sqlite; do
              if curl -sfo /dev/null "https://index.crates.io/${crate:0:2}/${crate:2:2}/$crate"; then
                packages+=(-p "$crate")
              else
                echo "$crate: not released yet"
              fi
            done
            cargo semver-checks "${packages[@]}"'
    else
        skip "sdk: semver-checks" "cargo-semver-checks or ${nightly:-the nightly} missing"
    fi
    if have_cmd cargo-public-api && have_toolchain "${nightly:-nightly}"; then
        run "sdk: interface.py --check (api.txt, shape.txt)" "python3 tools/interface.py --check"
    else
        skip "sdk: interface.py --check" "cargo-public-api or ${nightly:-the nightly} missing"
    fi
    run "sdk: interface test" "cargo test -p paperpaw-sdk --test interface"
    if [[ -f tools/shared-table.py ]]; then
        run "sdk: shared table's index and signatures" "python3 tools/shared-table.py --check && python3 -I tools/test_shared_table.py"
    else
        skip "sdk: shared-table" "tools/shared-table.py is not in this tree (interface 5)"
    fi
    if [[ -f tools/table-compat.sh ]]; then
        if [[ -n "${TABLE_BASELINE:-}" ]]; then
            run "sdk: table-compat against $TABLE_BASELINE" "tools/table-compat.sh '$TABLE_BASELINE'"
        else
            skip "sdk: table-compat" "set TABLE_BASELINE=<tag or commit of the last interface raise>"
        fi
    fi
}

g_embedded() {
    run "embedded: kernel and plugins, riscv32imc" "cargo build --release --target riscv32imc-unknown-none-elf -p paperpaw-app --no-default-features --features packs,store-plugins,diagnostics -p paperpaw-image -p paperpaw-sqlite -p paperpaw-stardict -p paperpaw-sample-hello -p paperpaw-x4 -p paperpaw-fat"
    run "embedded: kernel and plugins, wasm32" "cargo build --release --target wasm32-unknown-unknown -p paperpaw-app --no-default-features --features packs,store-plugins,diagnostics -p paperpaw-image -p paperpaw-sqlite -p paperpaw-stardict -p paperpaw-sample-hello -p paperpaw-x4 -p paperpaw-fat"
    run "embedded: lint the web emulator" "cargo clippy --target wasm32-unknown-unknown -p paperpaw-web -- -D warnings"
    run "embedded: build the web emulator" "python3 shells/web/build.py --limit-mb 16"
}

g_native() {
    run "native: plugin from another workspace" "cargo build --release --manifest-path crates/paperpaw-core/tests/native-plugin/Cargo.toml && cargo test -p paperpaw-core --test native_plugin -- --ignored"
    run "native: the SDK keeps no mutable statics" '
        CARGO_PROFILE_RELEASE_LTO=false cargo build --release --target riscv32imc-unknown-none-elf -p paperpaw-sdk --target-dir target/statics
        CARGO_PROFILE_RELEASE_LTO=false cargo run --release -p paperpaw-sign -- statics target/statics/riscv32imc-unknown-none-elf/release/deps/*.rlib'
    run "native: NOTICE names every crate that ships" "python3 tools/licences/build.py --check"
    run "native: the store's rules are the site's" '
        if curl -sSf --max-time 20 https://paperpaw.org/api/v1/store-rules.json -o target/ci-local/store-rules.json; then
          cargo run --release -p paperpaw-sign -- store-rules target/ci-local/store-rules.json
        else
          echo "paperpaw.org serves no store rules: not compared"
        fi'
    run "native: sample and templates name the firmware compilers" '
        x4="$(sed -n "s/^channel = \"\(.*\)\"/\1/p" shells/xteink/rust-toolchain.toml)"
        . shells/xteink/versions.env
        template=samples/hello-native/.github/workflows/build.yml
        release=tools/dev/templates/.github/workflows/paperpaw-release.yml
        status=0
        for found in \
          "samples/hello-native/rust-toolchain.toml:channel = \"$x4\"" \
          "$template:RUST_X4: \"$x4\"" \
          "$template:RUST_X4_PRO: \"$ESP_RUST_VERSION\"" \
          "$template:ESP_GCC: \"$ESP_GCC_VERSION\"" \
          "$release:RUST_X4: \"$x4\"" \
          "$release:RUST_X4_PRO: \"$ESP_RUST_VERSION\"" \
          "$release:ESP_GCC: \"$ESP_GCC_VERSION\""; do
          if ! grep -qF -- "${found#*:}" "${found%%:*}"; then
            echo "${found%%:*} should say ${found#*:}"
            status=1
          fi
        done
        exit $status'
}

g_architecture() {
    run "architecture: plugins depend on the SDK only" '
        status=0
        for manifest in plugins/*/Cargo.toml samples/*/Cargo.toml; do
          if awk "/^\[/ { part = \$0 } part == \"[dependencies]\" && /^paperpaw-core/" "$manifest" | grep -q .; then
            echo "$manifest: plugins must depend on paperpaw-sdk, not paperpaw-core"
            status=1
          fi
        done
        exit $status'
    run "architecture: store folders and x4-remote tests" "python3 -m unittest discover -s tools/plugin-files && python3 -m unittest discover -s tools -p test_x4_remote.py"
    if git rev-parse --verify -q origin/main > /dev/null; then
        # CI checks out the whole history; versions.py compares with the last release.
        run "architecture: a changed store plugin has a newer version" "python3 tools/plugin-files/versions.py"
    else
        skip "architecture: store plugin versions" "no origin/main to compare with"
    fi
    run "architecture: handbook tests" "python3 -m unittest discover -s tools -p test_handbook.py"
    run "architecture: CI tool setup tests" "python3 -m unittest discover -s tools -p test_ci_tools.py"
    run "architecture: Android version codes" '
        for pair in "0.4.0-dev 40050" "0.5.0-beta.1 50001" "0.5.0-beta.2 50002" \
          "0.5.0-beta.10 50010" "0.5.0-dev 50050" "0.5.0-rc.1 50051" "0.5.0 50099" \
          "0.5.1-beta.1 50101"; do
          set -- $pair
          got="$(shells/android/version-code.sh "$1")"
          [[ "$got" == "$2" ]] || { echo "$1 has version code $got, not $2"; exit 1; }
        done'
    run "architecture: release gate lists what OTA depends on" "tools/release-gate.sh > target/ci-local/release-gate.txt"
}

g_emulator() {
    run "emulator: build (Linux)" "cargo build -p paperpaw-emulator"
    local smoke
    smoke="$(cat <<'EOS'
        mkdir -p out
        emu="cargo run -q -p paperpaw-emulator --"
        for profile in $($emu --list-profiles | cut -d' ' -f1); do
          $emu --headless --profile "$profile" --script "wait:600" --screenshot "out/$profile.png"
        done
        $emu --headless --profile xteink-x4 \
          --script "confirm,right,confirm,page-forward,page-forward,page-back,wait:2000" \
          --screenshot out/xteink-x4-reading.png
        $emu --headless --profile xteink-x4 \
          --set reader.open-last=off --set slots.core.home=files.browser \
          --script "confirm,wait:500,confirm,confirm,right,right,right,right,confirm,wait:300" \
          --screenshot out/xteink-x4-epub-chapter.png
        $emu --headless --profile kobo-clara-bw --set core.language=de \
          --script "tap:500:480,tap:500:190,swipe:900:700:100:700,tap:100:700,long-tap:500:700" \
          --screenshot out/kobo-clara-bw-reading.png
        $emu --headless --profile xteink-x4 --script "long:confirm,confirm,wait:300" \
          --screenshot out/xteink-x4-larger-text.png
        $emu --headless --profile xteink-x4 \
          --script "long:confirm,right,right,right,right,right,confirm,right,confirm" \
          --screenshot out/xteink-x4-settings.png
        $emu --headless --profile two-buttons \
          --script "long:page-forward,page-forward,long:page-forward,page-forward,long:page-forward,page-forward,page-forward,page-forward,page-forward,page-forward,long:page-forward,page-forward,long:page-forward,page-forward" \
          --screenshot out/two-buttons-settings.png
        $emu --headless --profile boox-go-7 --set slots.core.home=diagnostics.screen \
          --script "page-forward,long-tap:600:800"
        $emu --registry out/registry.json
        printf '%s\n' '{"op": "quit"}' | $emu --stdio | grep -q '"event": "ready"'
        py="$(command -v python3 || command -v python)"
        "$py" tools/handbook.py out/registry.json ci out/handbook.zip
EOS
)"
    run "emulator: headless smoke test on every profile" "$smoke"
}

g_deny() {
    if ! have_cmd cargo-deny; then
        skip "deny: licences and advisories" "cargo-deny not installed"
        return
    fi
    run "deny: workspace" "cargo deny check"
    run "deny: X4 firmware's dependencies" "cargo deny --manifest-path shells/xteink/Cargo.toml --all-features check"
    run "deny: bluetooth host-test dependencies" "cargo deny --manifest-path tools/bluetooth-tests/Cargo.toml --all-features check"
}

g_firmware() {
    local device
    if ! have_toolchain "$x4_toolchain"; then
        skip "firmware: x4, x3 clippy" "toolchain $x4_toolchain not installed (rustup toolchain install $x4_toolchain --profile minimal --component clippy --target riscv32imc-unknown-none-elf)"
    else
        for device in x4 x3; do
            run "firmware: clippy $device + remote" "cd shells/xteink && cargo +$x4_toolchain clippy --release --target riscv32imc-unknown-none-elf --no-default-features --features $device,remote -- -D warnings"
            run "firmware: clippy $device" "cd shells/xteink && cargo +$x4_toolchain clippy --release --target riscv32imc-unknown-none-elf --no-default-features --features $device -- -D warnings"
        done
    fi
    skip "firmware: x4-pro, x4-classic" "needs the Xtensa (esp) toolchain; CI only"
}

g_android() {
    if have_cmd cargo-ndk && [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
        run "android: clippy the shell" "cargo ndk -t arm64-v8a --platform 26 clippy -p paperpaw-android"
    else
        skip "android: clippy the shell" "cargo-ndk or ANDROID_NDK_HOME missing (APK build is CI only)"
    fi
}

# Not in `full`: slow, ask for them with --only.
g_firmware-build() {
    if ! have_toolchain "$x4_toolchain"; then
        skip "firmware-build: x4" "toolchain $x4_toolchain not installed"
        return
    fi
    run "firmware-build: x4 release build" "cd shells/xteink && cargo +$x4_toolchain build --release --target riscv32imc-unknown-none-elf --no-default-features --features x4"
    run "firmware-build: sample plugin fits the firmware" '
        fp="$(cargo run -q --release -p paperpaw-sign -- fingerprint shells/xteink/target/riscv32imc-unknown-none-elf/release/paperpaw-xteink)"
        (cd samples/hello-native && env -u RUSTFLAGS cargo +'"$x4_toolchain"' x4)
        cargo run -q --release -p paperpaw-sign -- pack samples/hello-native/target/riscv32imc-unknown-none-elf/release/hello target/ci-local/hello-esp32c3.ppn
        cargo run -q --release -p paperpaw-sign -- plugin target/ci-local/hello-esp32c3.ppn | grep -Eq "^fingerprint: +$fp\$"'
    run "firmware-build: the store's plugins fit the firmware" '
        fp="$(cargo run -q --release -p paperpaw-sign -- fingerprint shells/xteink/target/riscv32imc-unknown-none-elf/release/paperpaw-xteink)"
        out=target/ci-local/plugins
        rm -rf "$out"
        tools/plugin-files/build.sh x4 "$out" --fingerprint "$fp"'
}

g_fuzz() {
    if ! have_cmd cargo-fuzz; then
        skip "fuzz: parsers" "cargo-fuzz not installed"
        return
    fi
    run "fuzz: one minute per parser" '
        mkdir -p fuzz/corpus/epub fuzz/corpus/txt fuzz/corpus/image
        cp samples/library/books/*.epub fuzz/corpus/epub/
        cp samples/library/books/*.txt fuzz/corpus/txt/
        cp crates/paperpaw-image/tests/data/*.jpg crates/paperpaw-image/tests/data/*.png fuzz/corpus/image/
        for target in epub txt xml hyphen image lua; do
          cargo +nightly fuzz run "$target" -- -max_total_time=60 -rss_limit_mb=1024
        done'
}

echo "ci-local: ${groups[*]} (jobs $jobs, RUSTFLAGS '$RUSTFLAGS')"
for g in "${groups[@]}"; do
    if declare -F "g_$g" > /dev/null; then
        "g_$g"
    else
        echo "unknown group: $g (try --list)" >&2
        exit 2
    fi
done

echo
echo "passed ${#passed[@]}, failed ${#failed[@]}, skipped ${#skipped[@]}"
if [[ ${#failed[@]} -gt 0 ]]; then
    echo "FAILED:"
    printf '  %s\n' "${failed[@]}"
fi
if [[ ${#skipped[@]} -gt 0 ]]; then
    echo "SKIPPED (not checked here):"
    printf '  %s\n' "${skipped[@]}"
fi
[[ ${#failed[@]} -eq 0 ]]

#!/usr/bin/env bash
# Builds the mock plugin for the emulator and takes the screenshots of #236
# into out/ (X4, 480x800). Usage: shots.sh [<paperpaw-emu>]
set -eu
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
emu="${1:-../../target-qc-lane5/debug/paperpaw-emu}"
CARGO_INCREMENTAL=0 cargo build --release --target x86_64-unknown-linux-gnu
rm -rf library out
mkdir -p out library/plugins/launcher-mocks library/.paperpaw/plugins/setup
cp -r ../library/. library/
cp -r plugin/. library/plugins/launcher-mocks/
cp target/x86_64-unknown-linux-gnu/release/launcher-mocks library/plugins/launcher-mocks/code-host.so
: > library/.paperpaw/plugins/setup/done

# shot <file> <screen> <script> [--set ...]
shot() {
    local file="$1" screen="$2" script="$3"
    shift 3
    "$emu" --profile xteink-x4 --headless --library library --trust-code \
        --free-space 20000000000 --set "slots.core.home=launcher-mocks.$screen" "$@" \
        --script "wait:800,confirm,wait:300$script" --screenshot "out/$file.png" 2>&1 | tail -1
}
for concept in a b c d; do
    case $concept in
        a) screens="home library quick" ;;
        b) screens="home library quick" ;;
        c) screens="home shelf more" ;;
        d) screens="home panel library" ;;
    esac
    for screen in $screens; do
        shot "$concept-$screen" "$concept-$screen" ""
    done
    # The focus moved once, in German, and the largest text size.
    shot "$concept-home-de" "$concept-home" ",right" --set core.language=de
    shot "$concept-home-largest" "$concept-home" "" --set core.ui-size=24
done

#!/usr/bin/env bash
# Builds the owner's X4 TEST IMAGE (decision 156, #243) and the bundle that goes with it:
#   dist/x4-test-image-<commit>/
#     paperpaw-x4-TEST-<commit>.bin   one image for the whole flash (bootloader, table, app)
#     SHA256SUMS                      checksums of every file in the bundle
#     BUILD-INFO.txt                  commit, features, toolchain, espflash, sizes, date
#     OWNER-STEPS.md / OWNER-SCHRITTE.md   the steps the owner follows (flash by hand)
# Nothing is flashed, published or signed here; the agents never touch a device.
#
# Usage:  tools/x4-test-image.sh [--out DIR] [--allow-dirty] [--dry-run]
#   --dry-run       print the steps, run none
#   --allow-dirty   build a tree with uncommitted changes (the bundle says so)
# Env:    FEATURES   cargo features (default: x4,test-image, the release recipe plus the test feature; never `release`)
#         CARGO_TARGET_DIR   where the build goes (default: shells/xteink/target)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fw="$root/shells/xteink"
features="${FEATURES:-x4,test-image}"
out_base="$root/dist"
dry=0
dirty_ok=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry=1 ;;
    --allow-dirty) dirty_ok=1 ;;
    --out) out_base="$(realpath -m "${2:?--out needs a folder}")"; shift ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

# The guards: it IS the test image, and it is NEVER a release (the firmware also
# refuses `test-image` with `release` at compile time).
case ",$features," in
  *,test-image,*) ;;
  *) echo "FEATURES must contain test-image (got: $features)" >&2; exit 2 ;;
esac
case ",$features," in
  *,release,*) echo "FEATURES must never contain release (got: $features)" >&2; exit 2 ;;
esac
case ",$features," in
  *,x4,*) ;;
  *) echo "FEATURES must contain x4: this is the X4's image (got: $features)" >&2; exit 2 ;;
esac

commit="$(git -C "$root" rev-parse --short=9 HEAD)"
full="$(git -C "$root" rev-parse HEAD)"
branch="$(git -C "$root" rev-parse --abbrev-ref HEAD)"
dirty=""
if [ -n "$(git -C "$root" status --porcelain --untracked-files=no)" ]; then
  dirty="yes"
  if [ "$dirty_ok" = 0 ] && [ "$dry" = 0 ]; then
    echo "The tree has uncommitted changes: commit them, or pass --allow-dirty." >&2
    exit 1
  fi
fi
bundle="$out_base/x4-test-image-$commit"
image="paperpaw-x4-TEST-$commit.bin"
elf="${CARGO_TARGET_DIR:-$fw/target}/riscv32imc-unknown-none-elf/release/paperpaw-xteink"

run() { printf '\n$ %s\n' "$*"; if [ "$dry" = 0 ]; then "$@"; fi; }

echo "X4 test image: $branch @ $commit${dirty:+ (uncommitted changes)}"
echo "Features:      $features"
echo "Bundle:        $bundle"

for tool in cargo espflash cmake ninja; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; [ "$dry" = 1 ] || exit 1; }
done
if ! command -v clang >/dev/null && [ -z "${CLANG_PATH:-}" ]; then
  echo "missing tool: clang (or set CLANG_PATH)" >&2; [ "$dry" = 1 ] || exit 1
fi
export CMAKE_GENERATOR="${CMAKE_GENERATOR:-Ninja}"

# 0. The guard: the test image is in no default list and no release build.
cd "$root"
run cargo test -q --locked -p paperpaw-checks --test test_image_guard

# 1. Build: the cargo profile `--release` (an optimised image), never the `release` FEATURE.
cd "$fw"
run cargo build --release --locked --target riscv32imc-unknown-none-elf --no-default-features --features "$features"

# 2. One image for the whole flash, as the release makes it.
run mkdir -p "$bundle"
run espflash save-image --chip esp32c3 --merge --skip-padding --flash-size 16mb \
  --partition-table partitions.csv "$elf" "$bundle/$image"

# 3. The image can take the next update (two app slots, the app fits them).
cd "$root"
run cargo run -q --release -p paperpaw-sign -- check-image "$bundle/$image"

# 3b. The frames of the interface-7 host stay within the X4's budget (2,048 B each).
run cargo run -q --release -p paperpaw-sign -- frames "$elf" --budget "$fw/frame-budget-ppn5.txt"

# 4. The notes and the checksums.
if [ "$dry" = 0 ]; then
  cp "$root/tools/x4-test-image/OWNER-STEPS.md" "$root/tools/x4-test-image/OWNER-SCHRITTE.md" "$fw/partitions.csv" "$bundle/"
  sed -i "s/<IMAGE>/$image/g" "$bundle/OWNER-STEPS.md" "$bundle/OWNER-SCHRITTE.md"
  {
    echo "PaperPaw X4 test image (decision 156, #243). NOT a release, NOT encryption: secrets are only veiled."
    echo "commit:    $full${dirty:+ (with uncommitted changes)}"
    echo "branch:    $branch"
    echo "features:  $features"
    echo "profile:   release (cargo --release), feature 'release' is NOT set"
    echo "rustc:     $(cd "$fw" && rustc --version)"
    echo "espflash:  $(espflash --version | head -1)"
    echo "image:     $image  ($(stat -c %s "$bundle/$image") bytes)"
    echo "app slot:  0x7f0000 bytes (ota_0 at 0x10000, ota_1 at 0x800000; shells/xteink/partitions.csv)"
    echo "built:     $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "Hashes hold for THIS build only: the paths of the build machine are in the file."
  } > "$bundle/BUILD-INFO.txt"
  (cd "$bundle" && sha256sum "$image" BUILD-INFO.txt OWNER-STEPS.md OWNER-SCHRITTE.md partitions.csv > SHA256SUMS && cat SHA256SUMS)
else
  printf '\n$ (copy OWNER-STEPS.md, OWNER-SCHRITTE.md; write BUILD-INFO.txt; sha256sum > SHA256SUMS)\n'
fi
echo
echo "Bundle ready: $bundle  (nothing was flashed or published)"

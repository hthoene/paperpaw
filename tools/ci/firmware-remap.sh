#!/usr/bin/env bash
# The path remapping of a firmware build that is released (#258): no folder of
# the machine that built it is in the image, and a little flash is saved.
#
#   tools/ci/firmware-remap.sh <target> [<toolchain>]
#
# Prints the value for cargo's `--config`: the flags are added to those of
# shells/xteink/.cargo/config.toml (RUSTFLAGS would replace them, and the
# frame budgets are measured with them). A source file is then named as
#   crates/paperpaw-core/src/kernel.rs   (this repository)
#   esp-hal-1.2.2/src/system.rs          (a crate of the registry)
#   library/core/src/fmt/mod.rs          (the toolchain's own sources)
# in panic messages and in the debug information. The crash record keeps what
# it needs: the file's name, and `exception_handler/` in the path of the chip's
# own handler (paperpaw_x4::blame).
#
# The scan builds (frame budgets) and the flash gate are not built with this:
# their budgets are measured without it.
#
# For tests (tools/test_ci_tools.py): CARGO_HOME and FIRMWARE_SYSROOT name the
# folders instead of the defaults and `rustc --print sysroot`.
set -euo pipefail

[[ $# -eq 1 || $# -eq 2 ]] || { echo "usage: $0 <target> [<toolchain>]" >&2; exit 2; }
target=$1
toolchain=${2:-}
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo_home=${CARGO_HOME:-$HOME/.cargo}
if [[ -n "${FIRMWARE_SYSROOT:-}" ]]; then
  sysroot=$FIRMWARE_SYSROOT
else
  sysroot="$(rustc ${toolchain:++"$toolchain"} --print sysroot)"
fi

# Of several prefixes that match a path rustc takes the last one given: the
# wider prefix comes first.
flags=()
add() {
  # A path that would need escaping in the TOML string is refused, not guessed at.
  case "$1" in
    *[\"\\=]* | *$'\n'*) echo "$0: cannot remap the path '$1'" >&2; exit 1 ;;
  esac
  flags+=("\"--remap-path-prefix=$1=$2\"")
}
add "$cargo_home" cargo
for registry in "$cargo_home"/registry/src/*/; do
  [[ -d "$registry" ]] && add "${registry%/}/" ""
done
add "$sysroot/lib/rustlib/src/rust/" ""
add "$root/" ""

joined=$(IFS=,; echo "${flags[*]}")
echo "target.$target.rustflags=[$joined]"

#!/usr/bin/env bash
# Size gate (#258), two things the firmware must not link.
#
# The names of all icons (`paperpaw_sdk::icons::names`, 64 KB): the firmware finds an icon
# by the hash of its name; the names are for tools and tests.
#
# `core`'s parser and formatter of floats
# (`core::num::dec2flt`, `core::num::flt2dec`, `core::fmt::float`): some 36 KB for a
# handful of numbers. What brings them in: `str::parse::<f32>()` (or `f64`), `{}` and
# `{:?}` on a float, `f32::to_string()`, and `f32::clamp` with bounds the compiler cannot
# see through (it says both where they are the wrong way round). Instead: the SDK's
# `ParamValue for f32` (`from_param`, `to_param`), `paperpaw_json` for a JSON number,
# `.min(most).max(least)`.
#
#   tools/firmware-symbols.sh <elf>...
#
# Needs `nm` (binutils) or `llvm-nm`. Exit 0: clean; 1: found; 2: usage or no symbols.
set -euo pipefail
[ $# -ge 1 ] || { echo "usage: $0 <elf>..." >&2; exit 2; }
nm=$(command -v llvm-nm || command -v nm || true)
[ -n "$nm" ] || { echo "firmware-symbols: no nm" >&2; exit 2; }
# As the modules are spelt in a symbol's name, mangled either way (legacy and v0),
# wherever in `core::num` they are.
pattern='7dec2flt|7flt2dec|4core3fmt5float'
# The tables only `icons::names` reads.
names='12paperpaw_sdk5icons(5NAMES|6STARTS|7LENGTHS)'
status=0
for elf in "$@"; do
    [ -f "$elf" ] || { echo "firmware-symbols: $elf is not a file" >&2; exit 2; }
    # A stripped ELF has no symbols to look for: that is no proof of anything.
    symbols=$("$nm" "$elf" 2>/dev/null || true)
    if [ -z "$symbols" ]; then
        echo "firmware-symbols: $elf has no symbols (stripped?): nothing to check" >&2
        exit 2
    fi
    hits=$(grep -E "$pattern" <<<"$symbols" || true)
    if [ -n "$hits" ]; then
        echo "$elf links core's parser or formatter of floats ($(wc -l <<<"$hits") symbols):" >&2
        head -5 <<<"$hits" >&2
        status=1
    else
        echo "firmware-symbols: $elf: no float parser or formatter"
    fi
    hits=$(grep -E "$names" <<<"$symbols" || true)
    if [ -n "$hits" ]; then
        echo "$elf links the names of all icons (icons::names is for tools and tests):" >&2
        head -3 <<<"$hits" >&2
        status=1
    else
        echo "firmware-symbols: $elf: no icon names"
    fi
done
exit $status

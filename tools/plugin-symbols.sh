#!/usr/bin/env bash
# Flash gate (decision 136): a built plugin must not link the whole records codec. Fails
# when an ELF (the file a `.ppn` is packed from) has the enum's generic decoder
# (`record_decode`) or encoder (`Record::write`, `body_len`, `tag`, `Record as WireEncode`).
# These link every record type's codec (161 KB in End of book before the typed facades).
# It also fails on the text-escaping tables (`core::unicode::printable`, `char`/`str` as
# `Debug`), about 4 KB per plugin, which `{:?}` on text or a failing `&str[a..b]` slice
# (`core::str::slice_error_fail`) pulls in. Slice with `.get(..)` instead.
#
# With `--events a,b,c` the plugin must read only those event kinds (the names of the
# decoders in `native/events/*_events.rs`, such as `started` or `screen_shown`, which
# `Incoming::decode_as::<kinds::ScreenShown>` calls): any other event decoder, or the
# one that reads every kind (`kinds::decode_any`, what `Incoming::decode` calls), fails.
#
#   tools/plugin-symbols.sh [--events kind,kind...] <elf>...
#
# Needs `nm` (binutils) or `llvm-nm`. Exit 0: clean; 1: found; 2: usage or no nm.
set -euo pipefail
events=""
if [ "${1:-}" = "--events" ]; then
    [ $# -ge 3 ] || { echo "usage: $0 [--events kind,kind...] <elf>..." >&2; exit 2; }
    events=",$2,"
    shift 2
fi
[ $# -ge 1 ] || { echo "usage: $0 [--events kind,kind...] <elf>..." >&2; exit 2; }
nm=$(command -v llvm-nm || command -v nm || true)
[ -n "$nm" ] || { echo "plugin-symbols: no nm" >&2; exit 2; }
pattern='records_wire::record_decode$|records_wire::Record>::(write|body_len|tag)|Record as paperpaw_sdk::native::declare::records_wire::WireEncode'
tables='core::unicode::printable|<char as core::fmt::Debug>::fmt|<str as core::fmt::Debug>::fmt|core::str::slice_error_fail'
status=0
for elf in "$@"; do
    [ -f "$elf" ] || { echo "plugin-symbols: $elf is not a file" >&2; exit 2; }
    bad=0
    # A stripped ELF has no symbols to look for: that is no proof of anything.
    symbols=$("$nm" "$elf" 2>/dev/null || true)
    if [ -z "$symbols" ]; then
        echo "plugin-symbols: $elf has no symbols (stripped?): nothing to check" >&2
        exit 2
    fi
    hits=$("$nm" -C "$elf" | grep -E "$pattern" || true)
    if [ -n "$hits" ]; then
        echo "$elf links the whole records codec:" >&2
        echo "$hits" | head -5 >&2
        status=1
        bad=1
    fi
    hits=$("$nm" -C "$elf" | grep -E "$tables" || true)
    if [ -n "$hits" ]; then
        echo "$elf links text-escaping tables (Debug on text, or a failing str slice):" >&2
        echo "$hits" | head -5 >&2
        status=1
        bad=1
    fi
    if [ -n "$events" ]; then
        extra=$("$nm" -C "$elf" \
            | sed -nE 's/.*native::events::(host_events|core_events|uix_events)::([a-z_0-9]+)::alone$/\2/p; s/.*native::events::kinds::(decode_any)$/\1/p' \
            | while read -r kind; do
                case "$events" in *",$kind,"*) ;; *) echo "$kind" ;; esac
            done || true)
        if [ -n "$extra" ]; then
            echo "$elf reads event kinds it should not (only$events):" >&2
            echo "$extra" | head -5 >&2
            status=1
            bad=1
        bad=1
        fi
    fi
    if [ "$bad" = 0 ]; then
        echo "$elf: clean"
    fi
done
exit $status

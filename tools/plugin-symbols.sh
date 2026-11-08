#!/usr/bin/env bash
# Flash gate (decision 136): a built plugin must not link the whole records codec. Fails
# when an ELF (the file a `.ppn` is packed from) has the enum's generic decoder
# (`record_decode`) or encoder (`Record::write`, `body_len`, `tag`, `Record as WireEncode`).
# These link every record type's codec (161 KB in End of book before the typed facades).
#
#   tools/plugin-symbols.sh <elf>...
#
# Needs `nm` (binutils) or `llvm-nm`. Exit 0: clean; 1: found; 2: usage or no nm.
set -euo pipefail
[ $# -ge 1 ] || { echo "usage: $0 <elf>..." >&2; exit 2; }
nm=$(command -v llvm-nm || command -v nm || true)
[ -n "$nm" ] || { echo "plugin-symbols: no nm" >&2; exit 2; }
pattern='records_wire::record_decode$|records_wire::Record>::(write|body_len|tag)|Record as paperpaw_sdk::native::declare::records_wire::WireEncode'
status=0
for elf in "$@"; do
    [ -f "$elf" ] || { echo "plugin-symbols: $elf is not a file" >&2; exit 2; }
    hits=$("$nm" -C "$elf" | grep -E "$pattern" || true)
    if [ -n "$hits" ]; then
        echo "$elf links the whole records codec:" >&2
        echo "$hits" | head -5 >&2
        status=1
    else
        echo "$elf: clean"
    fi
done
exit $status

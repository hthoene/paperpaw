#!/usr/bin/env bash
# Standalone CI hook for the draft ABI generator; no production ABI is enabled.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "${1:-}" == "--ci" ]]; then
    : "${ABI_BASELINE:?CI requires an independently supplied trusted ABI lock}"
    : "${ABI_BASE_REV:?CI requires the trusted base commit the lock came from}"
elif [[ $# -ne 0 ]]; then
    echo "usage: tools/abi-check.sh [--ci]" >&2
    exit 2
fi
python3 tools/abi-gen.py --check
python3 tools/reader-filter-gen.py --check
python3 -m unittest discover -s tools/tests -p 'test_abi_*.py'
python3 tools/abi-metadata-fixtures.py --check
printf '#include "abi/generated/paperpaw-abi.h"\n' | "${CC:-cc}" -x c -std=c11 -Wall -Werror -fsyntax-only -I. -
if [[ -n "${ABI_BASELINE:-}" ]]; then
    # A baseline replacement is single-use: once merged, the next change removes
    # it. One that is already in the trusted base and still here is refused.
    if [[ -n "${ABI_BASE_REV:-}" && -e abi/baseline-replacement.json ]] \
        && git cat-file -e "${ABI_BASE_REV}:abi/baseline-replacement.json" 2>/dev/null; then
        echo "abi-check: abi/baseline-replacement.json is already in the trusted base; remove it" >&2
        exit 1
    fi
    python3 tools/abi-gen.py --baseline "$ABI_BASELINE" --check
fi

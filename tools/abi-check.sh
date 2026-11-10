#!/usr/bin/env bash
# CI hook for the ABI generator: the generated files and the lock match the
# code; against the trusted base lock (--ci) only once a minor is released
# (one interface 7 until 1.0, owner decision 2026-11-10).
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
python3 -m unittest discover -s tools/tests -p 'test_abi_*.py'
python3 tools/abi-metadata-fixtures.py --check
printf '#include "abi/generated/paperpaw-abi.h"\n' | "${CC:-cc}" -x c -std=c11 -Wall -Werror -fsyntax-only -I. -
if [[ -n "${ABI_BASELINE:-}" ]]; then
    python3 tools/abi-gen.py --baseline "$ABI_BASELINE" --check
fi

#!/usr/bin/env bash
# Standalone CI hook for the draft ABI generator; no production ABI is enabled.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "${1:-}" == "--ci" ]]; then
    : "${ABI_BASELINE:?CI requires an independently supplied trusted ABI lock}"
elif [[ $# -ne 0 ]]; then
    echo "usage: tools/abi-check.sh [--ci]" >&2
    exit 2
fi
python3 tools/abi-gen.py --check
python3 -m unittest discover -s tools/tests -p test_abi_gen.py
python3 -m unittest discover -s tools/tests -p test_abi_layout.py
printf '#include "abi/generated/paperpaw-abi.h"\n' | "${CC:-cc}" -x c -std=c11 -Wall -Werror -fsyntax-only -I. -
if [[ -n "${ABI_BASELINE:-}" ]]; then
    python3 tools/abi-gen.py --baseline "$ABI_BASELINE" --check
fi

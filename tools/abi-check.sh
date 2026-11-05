#!/usr/bin/env bash
# Standalone CI hook for the draft ABI generator; no production ABI is enabled.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 tools/abi-gen.py --check
python3 -m unittest discover -s tools/tests -p test_abi_gen.py
printf '#include "abi/generated/paperpaw-abi.h"\n' | "${CC:-cc}" -x c -std=c11 -Wall -Werror -fsyntax-only -I. -
if [[ -n "${ABI_BASELINE:-}" ]]; then
    python3 tools/abi-gen.py --baseline "$ABI_BASELINE" --check
fi

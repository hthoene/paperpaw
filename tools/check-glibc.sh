#!/usr/bin/env bash
# The newest glibc a Linux binary needs: `tools/check-glibc.sh 2.35 FILE...`
# fails when one of them asks for a newer one (Debian 12 has 2.36, Ubuntu
# 22.04 has 2.35), and when a file needs none (not a dynamic Linux binary).
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 MAX_GLIBC FILE..." >&2
  exit 2
fi
max="$1"
shift

status=0
for file in "$@"; do
  # `objdump -T`: the dynamic symbols, each with the version it is bound to.
  needed="$(objdump -T "$file" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -Vu | tail -n 1 || true)"
  if [[ -z "$needed" ]]; then
    echo "$file: no GLIBC symbol versions found" >&2
    status=1
    continue
  fi
  newest="$(printf '%s\n%s\n' "$needed" "$max" | sort -V | tail -n 1)"
  if [[ "$newest" != "$max" ]]; then
    echo "$file needs glibc $needed, newer than $max" >&2
    status=1
  else
    echo "$file needs glibc $needed (at most $max)"
  fi
done
exit "$status"

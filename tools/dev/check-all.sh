#!/usr/bin/env bash
# The release check over every first-party plugin: `paperpaw-dev check` for
# each plugin with code (one by one, so that a store plugin is switched on
# alone, with the emulator's fixtures answering the requests) and for each
# data plugin by its folder (the hyphenation packs, built first). A line per
# plugin: its name and the errors and warnings it has.
#
#   tools/dev/check-all.sh [out-dir]     (default: target/paperpaw-dev-all)
set -u
cd "$(dirname "$0")/../.."
out=${1:-target/paperpaw-dev-all}
mkdir -p "$out"
cargo build -q -p paperpaw-dev || exit 2
bin=target/debug/paperpaw-dev
failed=0
summary() {
  # "N errors, M warnings, K notes (...)" is the report's last counting line.
  grep -E '^[0-9]+ errors, [0-9]+ warnings' "$1" | tail -1
}
for dir in plugins/*/; do
  id=$(basename "$dir")
  [ -f "$dir/Cargo.toml" ] || continue
  "$bin" check --plugin "$id" --network fixtures --out "$out/$id" >"$out/$id.log" 2>&1 || failed=$((failed + 1))
  echo "$id: $(summary "$out/$id.log")"
done
cargo run -q --locked -p paperpaw-hyphenation-packs -- build . "$out/data" || exit 2
for dir in "$out"/data/*/; do
  id=$(basename "$dir")
  "$bin" check "$dir" --network fixtures --out "$out/$id" >"$out/$id.log" 2>&1 || failed=$((failed + 1))
  echo "$id: $(summary "$out/$id.log")"
done
echo "plugins with errors: $failed"
[ "$failed" -eq 0 ]

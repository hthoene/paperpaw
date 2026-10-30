#!/usr/bin/env bash
# Lists what changed since the last release in the paths updates over the
# air depend on, for a review before every tag: a device that cannot take
# the next update needs a computer to recover.
#
#   tools/release-gate.sh [<since>]
#
# <since> is the last release's tag where not given. Prints the commits
# and the files changed, path by path, and what to check; the host tests
# and the release's check-image catch much, a reader's eyes the rest.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

since="${1:-$(git describe --tags --abbrev=0 --match 'v[0-9]*' HEAD)}"

# What updates depend on, with what each is.
paths=(
    "shells/xteink/src/ota.rs|the firmware's updates: slots, trial, install"
    "shells/xteink/src/firmware.rs|when the firmware installs, restarts and keeps a trial"
    "shells/xteink/src/native|code plugins in flash: the second slot's room, bundles"
    "shells/xteink/src/main.rs|the start: OTA before the kernel, safe mode"
    "shells/xteink/partitions.csv|the partition table of the X4, the X4 Pro and the X3"
    "shells/xteink/partitions-classic.csv|the X4 Classic's partition table"
    "shells/xteink/espflash.toml|how images are made (table, flash mode)"
    "shells/xteink/build.rs|the firmware's link: segments the bootloader maps"
    "shells/xteink/Cargo.toml|features and the bootloader's crates"
    "shells/xteink/.cargo/config.toml|build flags and aliases"
    "crates/paperpaw-x4/src/ota.rs|the OTA data and images (host-tested)"
    "crates/paperpaw-x4/src/updates.rs|start, trial and install as the firmware runs them (host-tested)"
    "crates/paperpaw-x4/src/partitions.rs|reading the partition table (host-tested)"
    "crates/paperpaw-update|update files: their signature and trailer"
    "crates/paperpaw-sdk/src/firmware.rs|what plugins see of updates"
    "crates/paperpaw-core/src/commands.rs|the kernel's firmware commands (the battery guard)"
    "plugins/update|the update page, the search online, downloads"
    "tools/sign|signing updates, check-image"
    ".github/workflows/release.yml|how the release makes, checks and signs images"
    ".github/actions/install-image|the app image updates are made of"
    ".github/actions/xteink-tools|the toolchain and espflash (its bootloader)"
)

echo "Changes since $since that updates over the air depend on:"
echo
any=0
for entry in "${paths[@]}"; do
    path="${entry%%|*}"
    what="${entry#*|}"
    commits="$(git log --oneline "$since"..HEAD -- "$path")"
    [[ -z "$commits" ]] && continue
    any=1
    echo "## $path ($what)"
    echo "$commits" | sed 's/^/  /'
    git diff --stat "$since"..HEAD -- "$path" | sed '$d' | sed 's/^/    /'
    echo
done

if [[ "$any" == 0 ]]; then
    echo "  none."
    exit 0
fi

cat <<'EOF'
Before tagging, check:
  - cargo test -p paperpaw-x4 (tests/updates.rs: every table, first flash,
    two updates, a failed trial) and cargo test -p paperpaw-sign pass;
  - the release's "Updates have somewhere to go" step (check-image) is
    still run for every device;
  - an update from the last release to this build installs, runs its
    trial and is kept, on a device of each chip (ESP32-C3, ESP32-S3),
    with an image made as the release makes it (no feature remote).
EOF

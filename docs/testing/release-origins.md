# Host checks for released update origins

`tools/qualify-release-origins.py` downloads the actual beta.1 and beta.2 X4
and X4 Pro full-flash, app-only install and signed update assets, with their
SHA-256 records. It writes only a local cache, manifest and JSON evidence.
It never contacts a reader, writes flash, signs, tags or publishes anything.
Run from this source checkout with its pinned Rust and an isolated target:

```sh
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=4
cargo build --locked --release -p paperpaw-sign
python3 tools/qualify-release-origins.py \
  --cache /tmp/released-origins --out /tmp/origin-evidence.json \
  --sign-tool "$CARGO_TARGET_DIR/release/paperpaw-sign" --download
```

Omit `--download` for an offline rerun; use a new output filename. Existing
cache pairs are never silently refreshed when their checksum fails. Missing
assets, mismatched filenames/hashes, failed commands or zero tested rows fail.
Run with a clean source checkout and keep the cache/report outside it. Each
run keeps a unique manifest and a completed-test receipt binding the exact
manifest, checked image hashes and optional candidate hashes. Exit zero without
that receipt cannot pass. The tool captures source identity before execution,
rejects dirty or changed sources and records the verifier binary hash, exact
asset hashes and complete Rust host-test output. Release signatures are verified against today's firmware
trusted-key list, not merely the key bundled in the update itself.

The host checks validate image chip/project/version, ESP checksum/hash, exact
app identity across the three packaging forms, old partition slots against
current main's table and current `OtaData` behavior on the released OTA bytes.
The transition test establishes a kept origin in memory, then checks both
slots, a successful trial, failed trial and a torn OTA-entry write. This uses
current firmware code: it does not execute a released bootloader or installer.
Existing `paperpaw-x4` update-path tests separately exercise current update UI,
download/install and failure paths; their synthetic defaults are not old-device
qualification. Run those with `cargo test -p paperpaw-x4 --test update_path`.

For a frozen target, add `--candidate x4=/path/install.bin` and
`--candidate x4-pro=/path/install-pro.bin`. Each must be an unsigned app-only
image, not a merged flash or signed update. The current host parsers validate
its checksum/hash, chip/project, fit in BOTH old slots and CrossPoint's smaller
slot with the existing 256 KiB release margin. Supply separately verified final
signed assets for the actual OTA/SD gates; app fit alone is not a trusted install.
Without both candidates the report marks target fit incomplete.

`--tag v0.6.0-beta.3` selects that origin when it has actually been published;
repeat `--tag` to select multiple origins. It fails if the required files are
absent. Do not substitute the unpublished candidate and call it a release.
The device matrix, actual-old-installer, signature trust bridge, failed durable
mark, user-state, dual-boot and physical interruption gates remain open in #229.
An exit-zero host audit intentionally reports full qualification INCOMPLETE.

`cargo test -p paperpaw-core --test beta_upgrade` checks frozen serialized
fixtures from the two release tags and explicitly synthetic plugin interface
headers. The fixture README records source paths and hashes. Current interface
6 keeps eligible interface-6 plugins; interface 5 is refused. A target-interface
bump needs a new refusal matrix, not an assumption that every addition breaks
old plugins. The tests preserve files; they do not claim signed native execution
or automatic reader-state migration on a device.

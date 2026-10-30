# PaperPaw's own plugins as files

The plugins the store offers as files (ADR 0024) that PaperPaw makes
itself: statistics, widgets, Calibre, KOReader sync and the others in
`plugins.txt`. The X4 and the X4 Pro leave them out of their released
firmware: `crates/paperpaw-app` builds them only with the feature
`store-plugins`, and the diagnostics only with `diagnostics`. Readers
install those they want from the store. A workspace of its own, as a plugin's
repository is, set up as `samples/hello-native`: one binary for each
plugin, `paperpaw_sdk::plugin!` around it.

The release pipeline (`.github/workflows/release.yml`) builds them for
every release and device, as the release's firmware is built, signs
them with the first-party plugin key, and publishes them with their
folders and their entries for the plugin library:

| | What |
|---|---|
| `plugins.txt` | the plugins, with their group in the plugin library |
| `build.sh <device> <out> [--key <file>] [--fingerprint <hex>]` | builds, packs and checks the files for one reader: `<id>-<version>-<device>.ppn`, `<version>` the plugin's own. `<device>` is `x4` or `x4-pro`. With `--only <id>`, one plugin alone (see below). |
| `anywhere.sh <x4\|x4-pro> [<work-dir>]` | checks that a plugin's file is the same wherever it is built, as the plugin library's rebuild needs: the sample, with the SDK from a registry as an author's repository has it, built in two folders and compared (CI runs it) |
| `sign.sh <dir> <paperpaw-sign> --key <file>` | signs the files apart from building them, so the key is on the disk only while it signs |
| `catalogue.py <dist> <release> <base-url>` | makes each plugin's folder as a ZIP with its `plugin.ini` and its How it works, `how/<lang>.txt`, which the store shows. It also makes the entry `catalogue/<id>.toml` for the plugin library, and `SHA256SUMS`. It takes `--date YYYY-MM-DD` too, `--only <id>` for one plugin alone and `--tag <tag>` for the tag the files are at (`v<release>` where not given). `test_catalogue.py` tests it: `python3 -m unittest discover -s tools/plugin-files`. |
| `versions.py [<since>]` | checks that each plugin that changed since the last release, or its own, has a newer version (CI runs it; see below) |
| `published.py files\|folders <dir> <releases.json>` | in a release, takes a plugin's file as published where its version and firmware were (`files`), and checks that a published version's folder is the same (`folders`) |
| `data.toml`, `data.py <dir> [--only <id>]` | PaperPaw's own data plugins (no code), such as the Literata packs the X4's firmware leaves out: their versions, groups, devices and texts, and the builder and inputs of each. `data.py` runs the builders into `<dir>`; `catalogue.py --data <dir>` makes their folders' ZIPs and entries (kind `data`, the folder alone) beside the others. `versions.py` asks a newer version of one whose builder or inputs changed since the last release or its own; `data.py --newer <id> <tag>` checks its version against a release. |

A plugin added to `plugins.txt` gets a binary here (`src/bin/<id>.rs`),
a dependency in `Cargo.toml`, and its place among the optional plugins
of `crates/paperpaw-app`. It keeps no writable statics
(`paperpaw-sign pack` refuses a file with any).

`Cargo.lock` is committed: the release builds with `--locked`, and the
plugin library rebuilds each file from it. When a plugin's dependencies
change, it is brought up to date here with `cargo update -w --offline`
(CI fails until it is).

## Versions

Each plugin has a version of its own, apart from PaperPaw's: its
crate's (`version` in `plugins/<id>/Cargo.toml`, 1.0.0 to start), which
its manifest says (`paperpaw_sdk::package_version!()`). Devices update a
plugin only to a newer version. A release builds every plugin again for
its firmware: one that did not change keeps its version, and the plugin
library adds the new build to it, where the firmware's fingerprint
changed; where it did not, the release takes the file as it was
published (`published.py`), so a version never comes with other bytes.
One that changed has a newer version: `versions.py` (CI) fails where a
plugin's files (`plugins/<id>/`, its binary here) changed since the
last release's tag, or since its own release, and its version did
not. A change no reader sees keeps the version when each commit with it
says so in its message, as a trailer:

    Plugin-unchanged: stats, widgets

(or `all`). A new version goes into the plugin's `Cargo.toml`, the
requirement in the workspace's `Cargo.toml`, and the `Cargo.lock` files
that list it (`cargo update -w --offline` brings them up to date).

## One plugin between releases

A fix to one of these plugins can reach readers before the next release:
`.github/workflows/store-plugin.yml` (run by hand, with the plugin, the
release it is for and the branch with the fix, made from that release's
tag) builds it for that release's firmware (the workflow checks the
fingerprint), signs it and publishes it as a release of its own, tagged
`<id>-v<version>`, with its files and its catalogue entry alone
(`paperpaw-<id>-v<version>-catalogue.zip`). Its version is its crate's,
newer than the release's. The plugin library adds the version to the
plugin's entry; the others keep theirs.

One of the data plugins of `data.toml` takes the same path, run with
its id: its version is `data.toml`'s, newer than the release's; its
builder makes its folder (`data.py --only`), nothing is built for a
device or signed, and the release holds its folder's ZIP and its entry
alone. Merged back, `versions.py` holds it to that version as it holds
the others.

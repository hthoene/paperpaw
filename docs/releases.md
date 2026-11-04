# Releasing PaperPaw

Firmware and application files are published by GitHub Actions when a
`v<version>` tag is pushed. Rust crates are published separately to
crates.io. Both use a clean, reviewed commit. A GitHub release alone does
not mean its SDK and plugin crates are available to plugin authors.

## Before the tag

1. Set the workspace version, the Android version code and the CHANGELOG
   section. Every plugin has its own version: change its manifest, the
   workspace requirement and the lockfiles when its package sources
   changed. This applies to built-in plugins as well as store plugins.
   A source refactor or changed package resource needs a new crate
   version even when the plugin's runtime behaviour stayed the same:
   crates.io cannot replace the sources of a published version.
   `python3 tools/plugin-files/versions.py --release --bytes` (or
   `tools/ci-local.sh --only release-versions`) builds the store files of
   the last release's tag and of this tree and fails for each plugin whose
   file differs under the same version, through the SDK or a crate it
   depends on or a bump that rippled, which the commit check cannot see;
   raise the versions it lists and run it again.
2. Build every plugin of the release (store, test cards, Workshop) from
   the baseline the CHANGELOG's section names, or later. The firmware
   refuses plugins of another interface fingerprint, but not those built
   before a change it cannot tell, such as the library's folders split
   by key (#280): an older build of a plugin that reads the library would
   keep its books' details where no version reads them any more.
3. Commit the prepared release. Run the checks in
   [CONTRIBUTING.md](../CONTRIBUTING.md) and review their GitHub results.
   Run `tools/release-gate.sh <previous-tag>` and complete the hardware
   OTA gate, including the second update hop on the X4 and X4 Pro.
4. Check the crate publication before tagging:

   ```sh
   python3 tools/publish-workspace.py --plan
   python3 tools/publish-workspace.py --dry-run
   ```

   `--plan` is the default. It lists missing versions and verifies every
   existing workspace crate against its sources in the registry. Only
   workspace members allowed on crates.io are selected. It uploads
   nothing; it writes a local publication journal under ignored
   `target/release/`.

   `--dry-run` also runs Cargo's package and build checks for the missing
   versions. Cargo handles their dependency order. Already-published
   crates are excluded only after their package sources match, including
   libraries and built-in plugins. A yanked version or registry/network
   error stops the check.

   CI's `tools/publish-dry-run.sh <test-version>` checks a temporary Git
   worktree with an unpublished prerelease number, so published release
   versions cannot hide a problem in the current library sources. It
   uses the same source guard for existing plugin versions. This script
   also uploads nothing.
5. Push the prepared commit and its version tag. Review the Release,
   CI, Publish check and Reproducible build workflows separately: they
   are independent, and a successful Release workflow does not imply
   the others passed. Do not move a tag whose files are already public.

## Publish the Rust crates

Use the clean release checkout and Cargo's configured crates.io
credentials. On Windows, `python` can be used in place of `python3`.

```sh
python3 tools/publish-workspace.py --publish
```

`--publish` is required for uploads. The script uses
`cargo publish --workspace --locked --exclude ...`, allowing Cargo to
order dependent packages. It does not pass tokens on the command line
or record credentials. Existing versions are verified and skipped,
never uploaded again. Source comparison ignores Cargo-generated
metadata and line-ending differences; inherited PaperPaw library
requirements may follow the same release's prerelease number, while
their features and other dependency options must still match.

The journal is
`target/release/publish-workspace-<commit-prefix>.json`. It records the
full commit, all package versions, pending and verified packages, and
each Cargo attempt with its exit code and separate log. `--journal
<path>` chooses another journal. A journal from a different commit or
package plan is refused, and the checkout must remain clean throughout
publication.

## Rate limits and resuming

crates.io can limit new crate names separately from new versions of
existing crates. A first publication of many language plugins can hit
the new-crate limit even when ordinary version updates worked. The
registry's response is authoritative; see its
[rate-limit documentation](https://crates.io/docs/rate-limits).

After an HTTP 429, the script respects the registry's `Retry-After`
header or its "Please try again after" UTC date, with a five-second
margin. It reports its wait at least every 30 seconds. If the response
contains no deadline, delays grow from 30 seconds to at most 15 minutes.
The default is at most eight automatic retries per invocation; use
`--max-retries <number>` to change that limit. Other Cargo errors stop
immediately, and network failures are never treated as missing crates.
The script does not change accounts, tokens or package identities to
bypass a limit.

Cargo can publish part of a workspace before a later crate hits a
limit. Run the same command again, from the same commit:

```sh
python3 tools/publish-workspace.py --publish
```

The saved deadline is respected and the registry is checked again.
Only missing versions are submitted. If Cargo reports success before
the registry index shows the new versions, the script waits for their
visibility instead of submitting that successful batch again. If they
remain invisible for ten minutes, it stops for inspection.

`--plan` can inspect a partial publication while a saved upload deadline
is still pending; it does not erase that deadline. Ctrl+C stops the
current command. Resume with the same commit and journal; the registry
is always checked again, because an upload may have reached the server
before the interruption.

## Complete and record the release

- Confirm the GitHub release contains the expected signed firmware,
  updates, applications, emulators, handbook, core libraries and plugin
  catalogue. Check every required workflow.
- Run `python3 tools/publish-workspace.py --plan` again: it must report
  no missing crates and verify all published sources.
- paperpaw.org imports release files and plugin catalogues hourly.
  An immediate website code deployment is not required solely because
  a new firmware release appeared.
- Record the immutable tag, any publication-recovery commit, checks,
  workflow URLs, package versions, rate-limit pauses and final registry
  status in `paperpaw-notes/paperpaw/versions/<version>.md` or a linked
  release record. If a public tag needs crate-version corrections,
  prepare a separate recovery commit and document precisely how it
  differs; do not replace the public tag or its signed files.

Offline regression tests for the publication tools:

```sh
python3 -m unittest discover -s tools -p 'test_publish*.py'
```

## The store release: where plugin files are published

A firmware release (`v<version>`) holds 73 files: the firmware of each device,
the emulators, the handbook, the web emulator, the ESP32-S3's core libraries,
the plugin library's catalogue (`paperpaw-<version>-catalogue.zip`) and the
checksums (`-plugins.sha256`). **No plugin file is in it** (#89): every `.ppn`
and every plugin or data folder's ZIP goes to the *store release* of the year,
tag `plugins-<year>` (`plugins-2026`), a pre-release that is never the latest
and is **added to and never replaced**. Only versions not published before are
uploaded (a version is published once; the catalogue names an old file by the
release that holds it). At 800 assets the next number is opened
(`plugins-2026-2`); the run stops at 900 (GitHub's limit is 1000). The tag does
not begin with `v`, so `release.yml` does not start for it.

`tools/plugin-files/store_release.py` chooses the release, plans the upload (it
refuses a name that is published with other bytes: raise the plugin's version),
uploads in batches with each file tried again, and checks that the release
lists every file with its size. `release_assets.py` holds the list of the
firmware release's files and checks that every address in a catalogue has an
asset. The release job uploads the plugin files **before** it creates the
firmware release, so paperpaw.org never reads an address without a file.

Try it without publishing: run the workflow *Release* by hand from a branch
(`version` = the workspace's version, `dry_run` on; a release is made only by a
tag). It builds everything and signs with throw-away keys (the real ones are for
a tag), then prints the commands that would upload and create, and fails if the
lists do not fit; no tag, no release, no upload.

## A store plugin between releases

One or several of PaperPaw's own store plugins, updates or new ones, can be
published between releases with the workflow *Store plugin update* (run by hand,
with the first-party plugin key): inputs `plugins` (ids, separated by commas or
spaces), `base` (the release whose firmware it is for, e.g. `v0.6.0-beta.2`),
`ref` (`main`, the default, or a branch made from that tag), `notes`, `dry_run`
(on by default: nothing is uploaded or tagged, the commands and the lists are
printed) and, only for a branch and only when needed, `accept`. It builds each
plugin for the base firmware of each device, checks that each file has the base
firmware's fingerprint, signs them and adds them to the store release, with one
catalogue ZIP for each plugin (`paperpaw-<id>-v<version>-catalogue.zip`, in the
store release too); the plugin's commit gets the git tag `<id>-v<version>`
(`versions.py` reads it). Devices are offered no firmware with it. Before it
builds, `store_release.py versions` checks each version against what is
published: not published already, not older than a published one.

From `main` (`ref` = `main`): what main has of the plugins now, built against the
base release's fingerprint and core libraries. A plugin that needs a newer SDK
or kernel than the base's firmware does not get that fingerprint, so the build
or the check before the signing fails: it needs a firmware release. Other
differences between main and the base are noted in the log, not refused
(`store_update.py --main`).

From a branch: the published firmware is the base's, so the source must differ
from the base's tag only in the plugins, the plugin files' project (the
manifest, lock file, list and binaries of `tools/plugin-files`, none of its
scripts), the CHANGELOG and docs (`store_update.py` checks this and the version:
above the base's for an update, any for a plugin the base does not have). A
change anywhere else (the kernel, the SDK, another plugin, a script or a
workflow) stops the run unless its path is named in `accept` (the workspace's
`Cargo.toml` may gain only the plugin's member and its `workspace.dependencies`
entry, and `Cargo.lock` only new packages, so a `[patch]`, a profile or another
SDK is refused; renamed files count at both ends; a path dependency of the
plugin must be another crate of the workspace): a decision for whoever runs it,
who has to know the change cannot differ from what the firmware does.

The source is somebody's push, so the workflow runs from `main` and keeps its
code away from the key: the check, the fingerprint test (`check_files.sh`), the
signing and the upload run this workflow's own scripts, from a checkout of the
commit the workflow is at (`github.workflow_sha`), not the source's. The
source's code runs only where there is no secret and the token only reads
(building the plugins, their catalogue entries); the signing job takes the
build's files, accepts only the names and the base firmware's fingerprint it
expects, and signs them; the release job, the only one that writes, runs none of
the source's code. A data plugin has no signature, and its version is read by
the source's script (`data.py`) with no secret.

Known limit: a data plugin has no such source rule. Its version is read by the
source's own `data.py`, and nothing checks what else the branch changed; it
carries no code and no signature, and the one who runs the workflow reviews the
branch. A diff rule for data plugins would be a separate change.

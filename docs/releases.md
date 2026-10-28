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
2. Commit the prepared release. Run the checks in
   [CONTRIBUTING.md](../CONTRIBUTING.md) and review their GitHub results.
   Run `tools/release-gate.sh <previous-tag>` and complete the hardware
   OTA gate, including the second update hop on the X4 and X4 Pro.
3. Check the crate publication before tagging:

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
4. Push the prepared commit and its version tag. Review the Release,
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
The default is at most 60 automatic retries per invocation (enough to
wait out the registry's limits for a whole release); use
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

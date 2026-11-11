# Preparing release notes

The full release procedure is in [docs/releases.md](docs/releases.md).
Collect notes on the release preparation branch before committing or
tagging the candidate:

```sh
python3 tools/changelog.py check
python3 tools/changelog.py collect --version 0.6.0-beta.4
python3 tools/changelog.py check --release
git diff -- CHANGELOG.md changelog.d
```

Use the version being prepared. Collection uses today's date; pass
`--date YYYY-MM-DD` for a planned release date. It merges the fragments
and existing Unreleased entries, creates a dated version section, leaves
an empty Unreleased section and removes the consumed fragments. Published
sections stay unchanged. Review the prose and the deletions, then commit
them together with the version changes. Give that preparation commit a
`No-changelog: Collect release notes and set release versions` trailer:
it consumes entries rather than adding one for the version bump itself.
Tag only after the normal review
and release gates; collection does not tag, publish or change versions.

A duplicate version or invalid fragment fails before changing files. The
changelog is replaced before fragment deletion: if cleanup is interrupted,
review the new release section and remove only the fragments already
included in it. Do not rerun with another version to bypass the duplicate
version check. Keep the release branch free of unrelated edits while
collecting. The release workflow rejects leftover fragments before builds.

## Freezing the plugin interface (1.0)

Interface 7 is one interface until 1.0 (`abi/interface-7/README.md`). Freezing it is the owner's
decision, in one pull request that:

1. deletes `native::DRAFT` and the line `draft = N` of `crates/paperpaw-sdk/interface.txt`, and
   appends `0 frozen ... (#...)` to `abi/interface-7/blessings.txt`;
2. releases the ABI lock: sets `released_minor` in `abi/abi.lock` to the schema's `minor`
   (`tools/abi-gen.py --check` then holds it), so every append-only check of the lock runs again.

`tools/abi-reference-check.py` refuses a frozen interface 7 whose lock is not released.

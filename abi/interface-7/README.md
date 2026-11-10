# Plugin interface 7: one interface until 1.0

Interface 7 (ADR 0034) has no users until 1.0 (owner decision, 2026-11-10). So there is **one** interface 7:
no draft steps, no carrier pull requests just to bless, no baseline replacements.

- **Changing it.** A port or a host piece that needs an interface change makes it in its own, ordinary pull
  request. The PR carries everything the change regenerates: `abi/abi.lock` (`tools/abi-gen.py --write-lock`),
  the generated files, and the reference files of ADR 0026 (`crates/paperpaw-sdk/interface.txt`,
  `crates/paperpaw-sdk/interface/` and each crossing crate's `api.txt`/`shape.txt`, written by
  `tools/interface.py` and `PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface`). Senior Reviewer
  reviews ABI changes briefly. If two PRs of a train both touch these files, QA regenerates them at train time.
- **What CI still checks.** The lock and the generated files match the code (`tools/abi-check.sh`), the
  reference files match what plugins see (the SDK's interface test), and `native::INTERFACE` and
  `native::DRAFT` never move back (`tools/abi-reference-check.py`). `native::DRAFT` stays at its value until
  the freeze; plugins built from an earlier tree are built again.
- **What it no longer checks.** Whether a change is an addition only, and whether it is named in
  `blessings.txt`. Both come back at the release: once a minor is released (`released_minor` in the lock), the
  lock is append-only against the trusted base again, and the frozen reference files change only with a raise.
- **Interface 6** (Beta 3) is released. Its plugins are refused by the fingerprint and the table check
  (`tools/table-compat.sh`); that gate stays.
- **Freezing** at 1.0 is the owner's decision. The freeze PR deletes `native::DRAFT` and the line `draft = N` of
  `interface.txt`, appends `0 frozen ... (#...)` to `blessings.txt` (which otherwise keeps the history of the
  drafts as it was) and releases the lock (`released_minor` = the schema's minor). The reference check refuses a
  frozen interface 7 with an unreleased lock (RELEASING.md).

`raise.md` records the rows of the raise from interface 6.

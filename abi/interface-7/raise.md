# Pending changes for the coordinated interface-7 raise

The interface-6 reference files are frozen until the coordinated raise (see `README.md`): the SDK interface
crates' `api.txt`/`shape.txt`, `crates/paperpaw-sdk/interface/*` and the comparator in
`crates/paperpaw-sdk/tests/interface/abi7_partition.rs`. A change that the interface-7 batch needs in one of
those files is listed here, never applied early and never stood in for by a temporary second constant. The raise
is then one mechanical commit: apply every row, retire the partition, regenerate, review.

Owner: Kernel Engineer (Manager decision, 2026-11-06). The raise commit is prepared as the last step of the
interface-7 batch, reviewed by the Senior Reviewer, and merged into `abi7/integration` only. Freezing and any
merge into `main` need the owner's approval.

## Rows

| # | File(s) | Change | Why | Source | Who waits |
|---|---|---|---|---|---|
| 1 | `crates/paperpaw-sdk/src/version.rs` (`DATA_FORMATS`), `crates/paperpaw-sdk/shape.txt`, `crates/paperpaw-sdk/interface/paperpaw-sdk.shape`, `CHANGELOG.md` | `DATA_FORMATS` 4 → 5 | The `[hyphenation]` data-plugin kind declares `formats = 5`; the installer (`paperpaw-sdk/src/install.rs`) and the kernel's data-plugin loader (`paperpaw-core/src/data_plugin.rs`, `FORMATS`) both read this constant | `docs/abi/proposals/declarative-registration.md`, "The `[hyphenation]` data-plugin kind" | The 13 hyphenation data plugins (PR #164) become installable |

## Steps of the raise itself

These are not rows of their own, but the raise is not complete without them:

1. Apply every row above.
2. Retire the temporary partition: the comparator in `abi7_partition.rs`, `ownership.txt`, and the separate
   snapshots in this folder, as `README.md` requires.
3. Raise the interface version and its fingerprint by ADR 0026, and regenerate every `api.txt`/`shape.txt` with
   `tools/interface.py`.
4. Record the raise in `CHANGELOG.md`.

Items behind the `kernel` cargo feature (ADR 0020), such as `paperpaw-hyphen`'s table check and card-backed
reader, are not part of the plugin interface and need no row.

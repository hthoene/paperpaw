# Rows of the interface-7 raise and the re-blesses after it

Until the early raise, the interface-6 reference files were frozen: a change the interface-7 batch needed in
them was listed here, never applied early and never stood in for by a temporary second constant. The early raise
applies row 1 and the raise steps below; every later row is applied as an ordinary, named re-bless of the draft-7
baseline (`README.md`, `blessings.txt`), in its own PR.

Owner: Kernel Engineer (Manager decision, 2026-11-06). Freezing and any merge into `main` need the owner's
approval; nothing here touches `main`.

**Early raise (Manager decision, 2026-11-06 06:18).** Interface 7 is unreleased and replaces interface 6 with no
compatibility (decision 122), and keeping the partition would block every port that needs new SDK surface. So the
raise happens early on `abi7/integration`, not as the last step of the batch:

1. One raise commit/PR bundles the rows due (see the split below) and the partition retirement, once the registration
   codecs, the card-backed hyphenation reader and the `[hyphenation]` loader have landed.
2. After it, interface 7 is a **draft baseline** on `abi7/integration`: every later change to it is re-blessed
   deliberately and named in its PR, until the owner approves the freeze. Interface-6 artefacts are retired
   cleanly, with no shims; old betas' plugins are refused as already implemented (the #747 refusal tests).
3. The Senior Reviewer reviews the raise PR, in particular that the trusted-base ABI CI (#147/#150) is retargeted
   to the draft-7 baseline, not silently disabled.

**Freeze (Manager condition, 2026-11-06 09:09).** The owner wants no remnants after the freeze: the freeze PR
deletes `native::DRAFT` and the `draft = N` line of `crates/paperpaw-sdk/interface.txt` completely (nothing
stays behind as 0). `DRAFT` is hashed into the fingerprint only while it is not 0, so the frozen interface-7
fingerprint is byte for byte the one without any draft concept. The freeze PR also replaces the interface
test's import of `DRAFT` with 0 and appends `0 frozen ... (#...)` to `blessings.txt`.

**Split (Manager decision, 2026-11-06 08:29).** The early raise applies **row 1 only**, so that the 13
hyphenation data plugins (PR #164) install. Row 2 follows directly as the first named re-bless, with its own
review; until it lands those plugins load, but their languages are not used on a page. Rows 3 and 4 follow as
named re-blesses too.

## Rows

| # | File(s) | Change | Why | Source | Who waits |
|---|---|---|---|---|---|
| 1 (applied by the early raise) | `crates/paperpaw-sdk/src/version.rs` (`DATA_FORMATS`), `crates/paperpaw-sdk/shape.txt`, `crates/paperpaw-sdk/interface/paperpaw-sdk.shape`, `CHANGELOG.md` | `DATA_FORMATS` 4 → 5 | The `[hyphenation]` data-plugin kind declares `formats = 5`; the installer (`paperpaw-sdk/src/install.rs`) and the kernel's data-plugin loader (`paperpaw-core/src/data_plugin.rs`, `FORMATS`) both read this constant | `docs/abi/proposals/declarative-registration.md`, "The `[hyphenation]` data-plugin kind" | The 13 hyphenation data plugins (PR #164) become installable |
| 2 (applied at draft 2) | `crates/paperpaw-sdk/src/hyphenation.rs` (`Hyphenation`, `Hyphenation::get`), `crates/paperpaw-sdk/src/env.rs` (`Env::hyphenation`), their `api.txt`/`shape.txt` | `Hyphenation::get` returns a table the kernel may read from the card (the card-backed reader in `paperpaw-hyphen`), not only `&Patterns<'static>`; the typesetter (`reading/typeset.rs`) and `widgets/grid.rs` follow | The tables of `[hyphenation]` data plugins are files on the card, which the frozen `get` cannot return; the kernel keeps them by language (`paperpaw-core/src/data_plugin/hyphenation.rs`, `Tables::find`) until then. With it, the reader reopens a table once after a transient I/O error before it gives up on it (Senior's remark B3 on PR #176), since only then is it read on a page | Contract D2 and the `[hyphenation]` section | Hyphenation from data plugins on a page (the 13 plugins of PR #164 load, but their languages are not used before this row) |
| 3 | `crates/paperpaw-sdk/src/native/declare.rs` and its child modules, `crates/paperpaw-sdk/api.txt`/`shape.txt` (about 158 lines) | The module `paperpaw_sdk::native::declare`: the builder of the declaration records 5-10 (setting groups, specs and extras, hyphenation, font faces and extensions) that plugins submit while they are made | A ported plugin declares its settings and resources through records, not the interface-6 `Registrar` | PR #180 (#768) | Every plugin port that declares settings or resources |
| 4 | `tools/abi-gen.py`, `crates/paperpaw-sdk/src/native/declare/records_wire.rs`, `crates/paperpaw-sdk/src/native/declare/settings_value.rs`, `abi/abi.def` | `tools/abi-gen.py` writes both SDK copies as generated outputs (checked by `--check`, replacing the byte-identity test of PR #180), and the choice limits (`CHOICES_MAX_VALUES`, `CHOICE_MAX_BYTES`) come from `abi.def` instead of two hand-kept copies in the SDK and `paperpaw-core/src/native_host/declarations.rs` | One source for the codec and its limits on both sides of the boundary | PR #180 review | Row 3 (its re-bless takes the generated copies) |

## Steps of the raise itself

These are not rows of their own, but the raise is not complete without them (all done by the early raise):

1. Apply every row above.
2. Retire the temporary partition: the comparator `crates/paperpaw-sdk/tests/interface/abi7_partition.rs`,
   `tools/abi_partition.py` and its test, `ownership.txt`, and the separate snapshots in this folder.
3. Raise the interface version and its fingerprint by ADR 0026 (`native::INTERFACE` 7, `native::DRAFT` 1),
   and regenerate every `api.txt`/`shape.txt` with `tools/interface.py`.
4. Turn on the tests that need formats 5: drop the `ignore` of the two tests in
   `crates/paperpaw-core/src/tests/hyphenation_tables.rs` and delete the test of today's refusal there.
5. Retarget the trusted-base ABI CI (#147/#150) to the draft-7 baseline: `tools/abi-frozen6-check.py` becomes
   `tools/abi-reference-check.py` (rules in `README.md`), still against the PR's base.
6. Record the raise in `CHANGELOG.md`.
7. Retarget the old-plugin check (`tools/table-compat.sh`, CI job "Old plugins, new firmware"): a baseline of an
   older interface or an older draft (`interface`/`draft` of its `interface.txt`) builds a plugin this firmware
   must refuse at load (fingerprint, `Native::problem`), before anything of it runs; only a baseline of this
   interface and draft must still run and agree. `crates/paperpaw-table/table-baseline.txt` stays at the
   interface-6 commit while interface 7 is a draft (draft builds are never released), so the job asserts the
   refusal; the first PR after the freeze sets it to the frozen commit (tag `interface-7`), and from then on the
   job asserts again that old builds run. `tools/ci-local.sh` runs the job from `table-baseline.txt` as CI does.

Items behind the `kernel` cargo feature (ADR 0020), such as `paperpaw-hyphen`'s table check and card-backed
reader, are not part of the plugin interface and need no row.

TODO (follow-up after the raise lands, Kernel): CI runs `tools/abi-reference-check.py` from `ABI_BASE_REV`'s
copy of the checker, not from the candidate's own, so a candidate cannot change the rules it is checked by.

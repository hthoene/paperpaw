# Interface-7 port (the native plugin and its settings; the rules wait for the reader)

Version 1.1.5. The plugin's part of interface 7 is small: it declares three settings and the reader
applies the rules. Everything else stays until the reader calls the `reader.filter` stage and the
host activates native plugins (then the interface-6 text filters go, no shim).

## Ported

- `NativeAids` (`src/native_plugin.rs`; `native_plugin!` in `src/native_entry.rs`, features `linked`
  and `native-entry`; `build.rs` turns `declarations.rs` into the batches built into the image):
  `make` submits the three settings and does nothing else, so it allocates nothing. Metadata in
  `native/` (no `uses.*` feature, no service: the plugin only registers; regenerate with
  `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
  native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`;
  `tools/tests/test_abi_plugin_metadata.py` fails on a stale one). `tests/linked_entry.rs` pins the
  linked descriptor to the generated metadata.

- `declarations.rs`: *Bold word beginnings*, *Replace words* (advanced, kept on reset) and
  *Pictures* in the reading aids group, with their defaults, choices, titles and help.
  `tests/abi7_declarations.rs` compares them with the interface-6 registration.
- Already in place (`src/reader_filter*`, feature `reader-filter`, `READER-FILTER.md`): the rules
  payload (`reader.filter` schema 1: encode, the whole-payload check before any block changes, the
  caption word, source positions), with fallible allocation and refusal sweeps
  (`tests/filter_allocation.rs`).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Removing the interface-6 text filters (`add_text_filter` in `register`, `bold_beginnings`,
  `replace`, `pictures::apply`, `filters_tests.rs`, the `Aids` plugin): with the reader calling the
  stage; until then they are the only way the rules apply, and the reader's cache keys depend on
  their signatures. They go in one commit then, no shim.
- The reader calling the stage (READER-filter-stage). The stage subscription is not declared: a
  subscription to a stage no one defines is refused, and the subscription's `signature` (the rules'
  hash, which the reader folds into its layout key) changes with the settings, so it has to be
  declared again when they change.
- Refusing a configuration the payload cannot hold before it is adopted (SETTINGS-validation): the
  settings editor has no hook for it yet.

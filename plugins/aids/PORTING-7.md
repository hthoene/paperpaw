# Interface-7 port (declarations; the rules wait for the reader)

Version 1.1.5. The plugin's part of interface 7 is small: it declares three settings and the reader
applies the rules. Everything else stays until the reader calls the `reader.filter` stage and the
host activates native plugins (then the interface-6 text filters go, no shim).

## Ported

- `declarations.rs`: *Bold word beginnings*, *Replace words* (advanced, kept on reset) and
  *Pictures* in the reading aids group, with their defaults, choices, titles and help.
  `tests/abi7_declarations.rs` compares them with the interface-6 registration.
- Already in place (`src/reader_filter*`, feature `reader-filter`, `READER-FILTER.md`): the rules
  payload (`reader.filter` schema 1: encode, the whole-payload check before any block changes, the
  caption word, source positions), with fallible allocation and refusal sweeps
  (`tests/filter_allocation.rs`).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The reader calling the stage (READER-filter-stage). The stage subscription is not declared: a
  subscription to a stage no one defines is refused, and the subscription's `signature` (the rules'
  hash, which the reader folds into its layout key) changes with the settings, so it has to be
  declared again when they change.
- Refusing a configuration the payload cannot hold before it is adopted (SETTINGS-validation): the
  settings editor has no hook for it yet.

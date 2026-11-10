# Interface-7 port (the native plugin and its settings; the rules wait for the reader)

Version 1.1.5 (above Beta 3's 1.1.1: no bump). The plugin's part of interface 7 is small: it declares
three settings and the reader applies the rules. The interface-6 plugin is removed (this port: the
`Aids` plugin, its registration and text filters, the plugin-file build of `tools/plugin-files`, the
app dependency and the store-plugin entry). **The current interface-6 app loses the three settings and
the rules until native activation and the reader asking the stage: merge right after both.**

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
- Already in place (`src/reader_filter*`, `READER-FILTER.md`): the rules
  payload (`reader.filter` schema 1: encode, the whole-payload check before any block changes, the
  caption word, source positions), with fallible allocation and refusal sweeps
  (`tests/filter_allocation.rs`).

- The `reader.filter@1` stage (schema: `stages/reader.filter@1.def`, a copy of the reader's
  `plugins/reader/stages/reader.filter@1.def`, Kernel-approved; the codec is `src/reader_filter/stages.rs`,
  made by `tools/stage-codec.py plugins/aids/stages plugins/aids/src/reader_filter/stages.rs --plugin reader`).
  `make` subscribes (`declarations::subscriptions`, a host that does not admit it yet answers
  `ERR_UNSUPPORTED` and the plugin goes on). `NativeAids::event` answers each `Event::StageCall` of
  `reader.filter` from the settings as they are then: `filter.rules` (signature of the canonical rules,
  bold, pictures policy, caption word: the host catalog's `aids.picture` as the interface-6 filter used (every language pack that has the key, `i18n.copy`), else `Bild` for the asked `de`, `Picture` for the rest; a book of
  pictures only keeps them) and up to 32 `filter.pair`. Rules the reply cannot hold (over 32 pairs, a
  string over 96 bytes, over 3,840 bytes) refuse the call (`ERR_NO_SPACE`), so the reader applies none
  of this plugin's rules. Buffers are heap (4 KiB setting, 3,840 B reply), fallibly allocated; frames
  stay small. The reader folds the reply's `signature` (it changes with the settings; the
  subscription's stays 0), the plugin id and the version into its layout key.
- `reader_filter::apply` (rules applied to one block) stays here until the reader screens take it
  over: the reader owns the pipeline (Kernel).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The reader asking the stage and applying the rules to each block (`BLOCKED READER-filter-fire`:
  the stage is defined and declared (PR #807), the native reader screens do not ask it yet).
  Without that, nothing applies the rules on a native host.
- Refusing a configuration the payload cannot hold before it is adopted (SETTINGS-validation): the
  settings editor has no hook for it yet.

## Interface-6 code removed

- The interface-6 text filters (`bold_beginnings`, `replace`, `pairs`, `pictures::apply`) live on only
  as `src/oracle.rs` (test code): the reference the stage's rules are compared with
  (`reader_filter/tests.rs`) and the property tests (`filters_tests.rs`). `whole_word` stays, the
  rules use it. Delete the oracle when the reader pipeline owns the parity tests.
- The declaration test pins the three settings as literals (it compared them with the interface-6
  registration before).

## Set aside until native activation (todo 380)

- Desktop snapshot cases `x4-pictures-caption` and `x4-pictures-hidden` (goldens deleted): they set
  `aids.pictures`, which nothing registers until the native plugin runs and the reader asks the stage.
  `boox-reading-de` is re-blessed (no *Reading aids* rows meanwhile); the `settings-reading` golden of
  the app loses its *Reading aids* row.
- `crates/paperpaw-app/tests/sets.rs`: the check that `aids` runs in the app is gone; the store-plugin
  list has ten entries.
- `tools/plugin-files/plugins.txt`: `aids` is commented out like the other ported plugins (the native
  file is built by `tools/plugin-files7`).

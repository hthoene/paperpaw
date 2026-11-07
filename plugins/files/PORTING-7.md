# Files on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin is still in place beside the native one. It is
removed in the PR that finishes the port (no shim, no dual registration shipped).

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`, `src/native_entry.rs`)

- Declarations: five settings, three screens, two actions; `build.rs` and `native/metadata*`
  generated with `tools/abi-metadata.py`. En/de texts in `i18n/` kept (parity test).
- Model (host-free, `native_model.rs`): the library list with its filter row, shelf lists and the
  shelves screen. All allocation is fallible (`mem::push`, `mem::string`, `mem::formatted`);
  `tests/native_alloc_refusal.rs` refuses each allocation in turn.
- Native plugin: list, shelves and shelf screens on `core.ui`, books read through the typed
  `Library` facade, opening through `command.open` (target `core.reader`).
- Tests: unit tests of the model, declarations, translations, linked entry, refusal sweep.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

- Authors, series, tags, folders and find-by-letter lists: the SDK has no `Library::facets`
  (asked of Services) and no typed library-changed event.
- Host acceptance of the native screens: BLOCKED on H1/H3/H4 and the host's library semantics.
- Covers: image slots (#385). Folder chooser: `core.files` (#339, #384).
- Later slices: the book sheet, the information screen, menus, help hints.

## Interim

The interface-6 code stays registered until the last slice; nothing in it is changed except the
version bump to 1.0.7.

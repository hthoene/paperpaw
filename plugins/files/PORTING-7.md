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

## Long libraries: the window slides

A window holds at most 96 books (three replies). "More books" grows a window that has room, and
on a full one it starts the next window at the host's cursor; "Previous books" (shown once a window
came before) reads the earlier window in full again from the start cursor kept for it. A stale
cursor starts over from the first book and forgets the earlier starts. `native_model::read_window`
holds the reading, generic over the cursor so `native_model` tests it with a fake source: the 97th
and the last book of 250, exactly 96 and 97 books, back to the window before, a stale cursor once.

## What the slice loses (interface 6 had it; each returns in a later slice or with its host piece)

- The "Next in your series" filter (needs LIB op 6 `next_in_series` on the list; Services/Host).
- The states *card gone*, *failed to read the folder*, *preparing the library* and *every book is
  in the archive* (`files.list.none.all`): the list shows what the host's query gives, and the
  empty-library text. The catalog phase is read only for "collecting".
- The button that looks for new books, and the request for a new catalog when the walk has not run.
  A stale book tap now says `files.list.gone` and reads the list again, but does not ask the host
  for a new catalog (no typed sender yet).
- "How it works" and the help hints.
- The search button, the find-by-letter lists and the series/author/tag/folder shelves (facets,
  below).
- Row look: interface 6 drew a progress bar and chips; the native list says the progress as text
  in the row (`45 %`, *Finished*, *New*), nothing for a book opened but not begun.
- The books count sits in the filter row's note; interface 6 had it at the row's end.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

- Authors, series, tags, folders and find-by-letter lists: the SDK has no `Library::facets`
  (asked of Services) and no typed library-changed event.
- Host acceptance of the native screens: BLOCKED on H1/H3/H4 and the host's library semantics.
- Covers: image slots (#385). Folder chooser: `core.files` (#339, #384).
- Later slices: the book sheet, the information screen, menus, help hints.

## Interim

The interface-6 code stays registered until the last slice; nothing in it is changed except the
version bump to 1.0.7.

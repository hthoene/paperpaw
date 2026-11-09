# Files on interface 7 (slices 1 and 2, partly ported)

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

## Slice 2: shelves, search, the next books of a series, book information

- **Shelves** (`files.shelves`): rows for *Next in your series* (only when a series begun has a
  next book), the favourites, and the lists of authors, series, tags and folders. Each list is a
  screen of its own (`files.names`, parameter `kind`): the names with their book counts from
  `Library::facets` (LIB op 5), in windows that slide like the books' (`read_window`, now generic
  over the row). Choosing a name lists its books. `native_shelves.rs` holds the rules, host-free.
- **Next in your series**: `native_shelves::collect_next`. The series with a book being read, then
  those with a finished one, from the series facet (`show` reading / finished); for each, the last
  begun book by number (`query`, sorted by series, descending) and the book after it (LIB op 6
  `next_in_series`). A next book that was already begun is not shown. At most 48 series; the
  catalog changing meanwhile is `State` and the list is read again.
- **Search**: the action `files.search` (menu `files.menu`, order 21) asks for words with the
  system keyboard (`Question::text`) and opens the books that have them (`Scope::Search`); more
  than 256 bytes says `files.search.long` and searches nothing (never a cut word).
- **Book information** (`files.info`, offered to `core.book-info`, parameter `path`): title and
  author, series with number, how far it is read, the fields plugins keep, the file (format and
  size) and the folder. A book the catalog does not know shows its file name and folder.
- Empty lists say their words: the next books, the favourites.
- Tests: `tests/scripted_library.rs` (what the plugin asks the scripted `core.library` and
  `core.ui`, and a refusal sweep of every call), unit tests of the model, `native_alloc_refusal.rs`.

## Differences from interface 6 in this slice

- The shelves are one list of categories, not names inline (interface 6 showed the first series
  and folders on the shelves page itself). The folders list shows what the host's folder facet
  gives (the folders books are in), by their own name, not a walk into sub-folders.
- *Next in your series* lists series being read first, then those only finished, each in the
  host's alphabetical order; interface 6 sorted by when the series was read last (the host's
  query has no such order for a facet). It is capped at 48 series.
- Search has no 500-books cap (the host has none, `library.md`); `files.search.capped` is unused.
- Information lacks: the cover, the tags' text (LIB op 4 has no SDK facade), the language and the
  description (the format's; `native::document` is not on integration yet).

## Still missing (interface 6 had it; each returns with its piece)

- **Covers** in the list, the grid and the last-read card: `Library::cover` (LIB op 7, asked of
  Services, todo 75 is the image facade) and an image blit on the native canvas (`document.md`
  section 5). Interface 6 stays registered until then.
- A request for a new catalog and the *look for new books* action: no typed command yet.
- The book sheet (hold a book: favourite, finished, archive, delete): needs `Library::delete` and
  `can_delete` (#668, on the draft-30 base) and a `core.files` move for the archive.
- Find by letter: the `letters` facet gives the letters, but a list cannot start at a facet's
  `first` index (a cursor is only made by a reply).
- The states *card gone*, *failed to read the folder*, *preparing the library* and *every book is
  in the archive*; "How it works" and the help hints; the search gesture on the home screen.
- The *Library* menu (sort, view, look again, folder, settings): only the shelves entries and the
  search are declared.
- The books count in a row's end (it is in the filter row's note); the row's progress bar and chips.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

- Host acceptance of the native screens: BLOCKED on H1/H3/H4, and the host's catalog behind LIB
  ops 2, 5 and 6 (the default host trait answers no facets).
- The typed library-changed event (the lists are read again when a screen is shown).
- The folder chooser: `core.files` (#339, #384).

## Interim

The interface-6 code stays registered until covers exist (no shim, no dual registration shipped);
nothing in it is changed. Version: 1.0.7 is above Beta 3 (1.0.6), so there is no bump.

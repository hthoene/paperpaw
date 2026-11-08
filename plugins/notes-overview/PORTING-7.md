# Notes overview on interface 7 (partly ported)

Status: **not complete** (the native plugin lists the books and a book's notes; the export and
the opening of a note wait for the host). The interface-6 plugin (`clippings.rs`, `scan.rs`,
`screen.rs`, the `Plugin` in `lib.rs`) is still in place, because the host
services it needs are not there. It is removed in the PR that finishes the port.

## Ported (no host needed): `src/model.rs`

- The list: books with notes, at most 200, by title without regard to case
  (ties by path), the *Export* row, the "more" flag, the count pieces
  (`3 highlights · 1 bookmark`).
- The `My Clippings.txt` text: BOM, CR LF, Kindle weekday/month names in
  English, `Highlight at location p-p`, own note as `Note`, bookmark without
  text, one-line texts, local time passed in. Written into a fallible
  `Sink`; every allocation can be refused (`Error::NoMemory`), tested at
  every byte of an export (`model/tests.rs`).
- Input is plain `Note` values: the caller copies the host's rows out, so
  the model does not depend on the library facade.
- Tests of the interface-6 `clippings.rs` moved over unchanged (date, entries,
  book line); texts (en/de) are unchanged.

## Ported (over `Storage`, no host service needed): `src/export_file.rs`

- `PartFile` is a `Sink` over the platform `Storage`: `Exports/My Clippings.txt.part`
  made anew, appended to in 6 KiB pieces from one reserved buffer, renamed into place
  when whole (`finish`). A refusal (memory: `NoMemory`, full card: `Full`, other: `Io`)
  or a drop before the end removes the part and leaves the old file; where a failed
  rename took the old file (FAT), the part is kept (it is all there is).
- The rename must replace an existing file: the `core.files` adapter the shell builds
  must offer it (`move_entry` with replace). Tested on a storage that allocates nothing,
  every allocation refused in turn (old file kept, no part left), and a full card at
  four places.
- `model::put_detail`: the second line of a note's row (`what · 42 %`).

## Ported (over the library service, slice 2): `src/native_scan.rs`, `src/native_library.rs`

- Which books have notes: `native_scan::scan` passes the catalog (by title) a page at a time and
  counts each book's highlights and bookmarks by paging its annotations (`core.library` op 15, one
  4,096-byte reply at a time; kinds only, the 512 B previews are not kept). Only the books of the
  list are kept (200, `MOST_BOOKS`); once it is full, a further book with notes is looked at until
  its first highlight or bookmark and only sets `more`, so a library of thousands costs host calls,
  not memory. The interface-6 scan listed the folder of notes files first; there is no such folder
  on interface 7.
- Memory of the list, worst case: a listed title is cut to `TITLE_MAX` = 128 bytes (at a
  character's end), and a book with a path over `PATH_MAX` = 512 bytes (the most the library names
  a book by; `BookRef::path` refuses longer ones) is left out, since it could not be opened from
  the list. So 200 books hold at most 200 x (128 + 512) = 128,000 bytes of text, plus 200 x 56
  bytes of headers (about 139 KiB; a library of ordinary paths and titles needs about 30 KiB).
  Interface 6 cut neither.
- The walk ends at the first book past the 200 (it only sets `more`), without walking the rest of
  the catalog. So the books listed are the first 200 with notes **in the catalog's title order**;
  interface 6 took the first 200 in the order of its walk of the library and then sorted them
  (deliberate; accepted in review). The list is sorted again by title without regard to case, ties
  by path, as before.
- The scan is written over a `Source` trait; `native_library::HostSource` fills it in from the SDK's
  `Library` (`query`, `annotations`, `book`; key and catalog generation make a `BookRef`). Tested
  over a scripted library (`src/native_scan_tests.rs`): order and counts, any page size, a full list,
  stale pages (a book's notes are read again, the whole scan after that, `RESTARTS` = 3 each, then
  `State`), replies that do not add up (rows over the total, a next page without rows: `Invalid`, so
  a host that never ends cannot hold the scan), a book gone meanwhile (left out), and every
  allocation refused in turn. `HostSource` itself is thin and not covered without a host (no SDK
  mock host): it compiles against the SDK and is exercised by the acceptance cases below.
- Not read here: the cut texts (op 4) and the notes' own texts; the export slice does that.

## Ported (the native plugin, slice 3): `src/native_plugin.rs`, `declarations.rs`

- `NativeNotesOverview` (`native_plugin!` in `src/native_entry.rs`, features `linked` and
  `native-entry`; `build.rs` builds the declarations into the image; metadata in `native/` with
  `uses.library`, `core.library` and `core.ui`, regenerated with `tools/abi-metadata.py` as for the
  other plugins). `declarations.rs`: the list screen (`notes-overview.screen`) and the book's
  (`notes-overview.book`), one instance at a time each, the action `notes-overview.open`, the Tools
  menu entry (order 37) and the `bookmarks` icon; `tests/native_declarations.rs` compares them with
  what the interface-6 plugin registered, field by field, and checks every record against the
  host's validation (the menu entry and the icon wait for the host's admission, as for every
  plugin of this port).
- The list screen: the books with notes by title with `3 highlights · 1 bookmark` under each, then
  the *Export* row (with the longer help when the list is cut at 200); with no books the empty
  text. Choosing a book opens the notes screen with the path handed over (`core.ui` handover, the
  parameter `path`). The book's screen: its notes in reading order, bookmarks as their label,
  highlights quoted, each with its own note (else its kind) and `· 42 %` under it; kinds a newer
  reader keeps are not listed. **New cap: 300 notes shown per book** (`MOST_NOTES`; interface 6
  showed all, from a file of up to 1 MiB); a note's text is the library's 512 B preview (the
  interface-6 screen had the whole text, wrapped to two lines in the row either way).
- Read through `Library` with 4,096-byte buffers; a pass over a book's notes ends after 64 pages.
- `tests/scripted_library.rs` drives the plugin with the scripted host over a scripted
  `core.library`: the pages of the catalog continued in their generation, each book's notes counted
  page by page, only the books with notes described, a stale catalog read again from its start,
  and a refusal of every service call and every service opening turning into an error status. The
  scripted host does not answer `i18n.copy`/`i18n.format` yet (`ERR_UNSUPPORTED`), so the rows'
  texts are not checked there and the callback ends in that status after the library was read: a
  gap for Core.

## Not ported yet (each is an ignored test in `tests/acceptance_pending.rs`)

- The export: the notes of every book with their cut texts (op 4, 1,024 B parts) written to
  `Exports/My Clippings.txt` through `core.files` (PR #339; `PartFile` is over `Storage`, the
  native files facade has none). The *Export* row refuses with `Unsupported` until then.
- Opening a book at a note (the draft-22 `doc_open`/`command.open` into `core.reader`, then the
  goto event): choosing a note refuses with `Unsupported`.
- The rows' texts and the pixels (H1/H3 and the catalog in the scripted host).
- Removing the interface-6 plugin (`clippings.rs`, `scan.rs`, `screen.rs`, the `Plugin` in
  `lib.rs`): with the activation, in one commit, no shim.

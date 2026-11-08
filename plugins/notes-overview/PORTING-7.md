# Notes overview on interface 7 (partly ported)

Status: **not complete** (the native plugin lists the books, pages a book's notes and exports them
all; the opening of a note at its place waits for the host). The interface-6 plugin (`clippings.rs`, `scan.rs`,
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
- Memory of the list, worst case: the list keeps each book by its **catalog handle** (`lib_entry.key`
  with the catalog generation, 8 bytes), not by its path (up to 512 bytes); the path is never
  fetched, and the book's screen is opened by the same handle (handover parameters `key` and
  `generation`, decimal). A listed title is cut to `TITLE_MAX` = 128 bytes (at a character's end).
  So 200 books hold at most 200 x 128 = 25,600 bytes of text plus the headers of the entries
  (about 30 KiB), however long the paths are (an earlier draft that kept the path was 139 KiB at
  worst). Interface 6 cut no title. Cost: a handle is valid in one catalog generation. Choosing a
  book first asks the library for it; on `State` (the catalog changed) or `NotFound` the list is
  made again once under the open screen, and the book is opened if it is still there (found by its
  title, among equal titles by its place among them; its new handle is used). If it is not, the
  refreshed list stays and a notice says so (`notes-overview.changed`). Interface 6 kept paths,
  which do not go stale. Tested: the matching of a book in the new list (`native_plugin` tests);
  the refresh itself needs `i18n.copy`/`i18n.format` in the scripted host (the gap named below).
- The walk ends at the first book past the 200 (it only sets `more`), without walking the rest of
  the catalog. So the books listed are the first 200 with notes **in the catalog's title order**;
  interface 6 took the first 200 in the order of its walk of the library and then sorted them
  (deliberate; accepted in review). The list is sorted again by title without regard to case, ties
  by handle (the catalog's order), where interface 6 broke ties by path.
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
  text. Choosing a book opens the notes screen with the book's handle handed over (`core.ui` handover,
  the parameters `key` and `generation`). The book's screen: its notes in reading order, bookmarks as their label,
  highlights quoted, each with its own note (else its kind) and `· 42 %` under it; kinds a newer
  reader keeps are not listed. **New caps (memory):** at most 100 notes shown per book
  (`MOST_NOTES`; interface 6 showed all, from a file of up to 1 MiB), a note's text cut to 160 bytes
  (`TEXT_MAX`) and the reader's own note to 64 bytes (`NOTE_MAX`), both at a character's end; the
  row shows two lines anyway (interface 6 wrapped the whole text to two lines), and the library's
  preview is up to 512 B. **Worst case of the book screen:** a row is the item (80 B) + 166 B of
  quoted text + 64 B of note + 16 B of detail suffix + 24 B of allocator headers, at most 350 B;
  100 rows are 35 KB plus the vector grown to 128 items (10 KB), about 45 KiB (a test holds it
  under 48 KiB). The first draft (300 rows of 512 + 512 B) was about 330 KiB.
- **Book screens open at once.** Both screens are per parameters, as in interface 6. Reopening the
  list from Tools closes a book screen under it (`docs/abi/ui.md`), so at most one book screen is
  open and the stack holds the list (about 30 KiB) and one book (about 45 KiB): **75 KiB** of rows
  at most, a real library about half that. As a safeguard for the day a note opens another screen
  above (the reader), a book screen that opens or is shown frees the rows of every other notes
  screen first (`free_notes`; the book's handle, its page and the focused row are kept) and a
  screen shown after it had freed its rows reads its page again from its handle; if the book
  changed meanwhile (`State`/`NotFound`) the screen says so (`notes-overview.gone`) and its list is
  empty. Nothing frees the list's rows under a book screen: the host sends no event when a screen
  is covered, and making the list again on each return would walk the library again. 75 KiB of the
  256 KiB heap is an X4 risk to measure (memory gate), not measured. Tested: the policy of
  `free_notes` (unit test over views); the opening of two books in the scripted host is not,
  because the scripted host does not answer `i18n.copy`/`i18n.format` yet and the instances are
  host-issued.
- **Paging (closes the parity item of the 100-note cap).** A book's notes are shown 100 to a page
  (`native_notes.rs`): a last row *Next notes* reads on from where the page ended and a first row
  *Previous notes* goes back; the page before is read again from its start (the starts of the pages
  shown so far are kept, 24 bytes each), so only **one page of rows is held** and the memory stays
  at about 45 KiB however long the book is. A page starts at a library reply and a row inside it, so
  every size of reply gives the same pages. A cursor the library no longer knows (the book's notes
  changed) starts again at the first page. A page is read from at most 256 replies (`MOST_REPLIES`; a reply holds a few notes). Pages beyond the
  256th (25,600 notes) are not reachable; those notes are in the export. Tested over a scripted
  reply source (`native_notes_tests.rs`): the same pages at any reply size, forward and back, the
  ends (exactly 100 notes, none), a stale cursor, a host that never ends, every allocation
  refused in turn. Not tested end to end: the rows' texts (scripted host without `i18n`).
- If the catalog changes between the handover of a chosen book and the book screen opening
  (`opened_book`; one host callback apart), the book's handle is stale (`State`/`NotFound`) and the
  screen does not open, with no notice. The list itself handles a stale handle before the handover
  (see above), so the window is only that one callback.
- Read through `Library` with 4,096-byte buffers; a pass over a page of a book's notes ends after 256 replies.
- `tests/scripted_library.rs` drives the plugin with the scripted host over a scripted
  `core.library`: the pages of the catalog continued in their generation, each book's notes counted
  page by page, only the books with notes described, a stale catalog read again from its start,
  and a refusal of every service call and every service opening turning into an error status. The
  scripted host does not answer `i18n.copy`/`i18n.format` yet (`ERR_UNSUPPORTED`), so the rows'
  texts are not checked there and the callback ends in that status after the library was read: a
  gap for Core.

## Ported (the export): `src/native_export.rs`, `native_staged.rs`, `native_run.rs`, `HostSource::for_export`

- Choosing *Export as My Clippings.txt* writes the notes of every book to `Exports/My
  Clippings.txt` beside the library folder (`core.library-folder`, default `books`; `Exports` is
  made if missing), in place of the file there, and says how many notes were saved and where (the
  interface-6 texts: `notes-overview.exported`, `.none`, `.export-failed`). The catalog is passed a
  page at a time and each book's notes a page at a time, every note written as it comes (the
  entries are those of `model::put_book`, tested unchanged from interface 6), so the memory is a
  page, one note and a 4 KiB piece however many books there are. A note or its own note that the
  library cut is read whole with `core.library` op 4 (1,024 B parts); **new cap: 8 KiB each**
  (`TEXT_LIMIT`; interface 6 read them whole, from a file of up to 1 MiB), cut at a character's end.
- The file is staged in the plugin's data folder (`export.part`, `storage` write/append, `sync`) and
  moved onto the card with `core.files` `move_entry` with replace (declared `uses.storage`,
  `core.files`); a refusal anywhere (memory, a full card, the host) removes the staged file and the
  old export stays. A drop of the staging object removes it too. With no notes nothing is written
  and the old file stays. A stale page restarts the whole export up to 3 times (`State` after that).
- The walk is synchronous in one callback, as in interface 6; a library of thousands of books costs
  one host call per book for its names and one per notes page. No budget or cancel yet (there is
  no declared work for it); a long library will hold the screen for that time.
- Note times are written in UTC until the system time zone comes with the snapshot (draft 25);
  interface 6 wrote local time. The library folder comes from the setting, as in `files`.
- Tested over a scripted library (`native_export_tests.rs`): the file byte for byte (a highlight with
  its note, a bookmark, a kind not exported, a book with no author), at any size of the pages, a
  book that is gone, a stale page, replies that do not add up, a sink that fills, every allocation
  refused in turn. Not tested: the staging and the move on a card (the scripted host has no
  `storage`/`core.files`): acceptance on the desktop shell when the host runs native plugins.

## Not ported yet (each is an ignored test in `tests/acceptance_pending.rs`)

- Opening a book at a note (the draft-22 `doc_open`/`command.open` into `core.reader`, then the
  goto event): choosing a note refuses with `Unsupported`.
- The rows' texts and the pixels (H1/H3 and the catalog in the scripted host).
- Removing the interface-6 plugin (`clippings.rs`, `scan.rs`, `screen.rs`, the `Plugin` in
  `lib.rs`): with the activation, in one commit, no shim.

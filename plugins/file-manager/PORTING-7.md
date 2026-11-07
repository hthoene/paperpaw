# File manager on interface 7 (partly ported)

Status: **not complete.** The interface-6 plugin (`lib.rs`: `Plugin`, `Browser` screen,
`folder.rs`) is still in place, because the host services and the native shell it needs are
not there. It is removed in the PR that finishes the port; `folder.rs` and the SDK `files`
helpers it uses are then dead.

## Ported (host-free, over `Storage`): `src/model/`

The native shell gives the model the platform `Storage` (the `core.files` adapter over
`native::files`, whose folder reads are page by page); nothing here holds more than a page
of entries, two paths, a page per folder level or one 4 KiB copy buffer.

- `path`: names, parents, `same_path`, `is_within` (no allocation), fallible `join`/`copy`,
  `free_path` (`name (2).ext`, one reused buffer).
- `folder`: `Folder::{count, rank, after, before, page, page_of}`, the keyset pages in the
  order folders first, then names regardless of case, then exact names (moved from `folder.rs`,
  fallible: each page reserves `n + 1` once, each name is copied fallibly).
- `ops`: `move_to` (rename, else copy and delete; never a folder into itself), `copy` (file
  to a `.part` beside it through a 4 KiB buffer (interface 6: 16 KiB; one fallible allocation per file), put in place whole; a folder with everything in it, a page of 8
  entries at a time and at most 16 levels deep, `NoSpace` beyond), `delete`, `rename` (to a
  free name where the name is taken; through a name between where the storage ignores case).
- Errors are the host's (`paperpaw_sdk::native::Error`): a refused allocation is `NoMemory`;
  a storage failure that has no variant of its own (card gone, any other I/O failure) is `Io`
  (interface 6 knew `CARD_GONE`: the shell tells it from `Io` by the storage's state).
- A file is never copied (or moved on a storage without rename) onto itself, whatever the
  case on a storage that ignores it: that would empty it first (`Invalid`).
- Tests: the pages and ranks of the interface-6 `folder.rs` tests on explicit expectations;
  every allocation refused in turn for the pages, a tree copy, a file copied over another, a
  move without rename and the three renames (source kept, a copy whole or not there, no
  `.part` left, no `.renaming` name left).

Behaviour changes, on purpose: a folder is copied a page at a time (interface 6 held the
whole listing, and a storage may not take calls from inside its listing); folders nested more
than 16 deep are refused (`NoSpace`) instead of recursing; `delete` of a missing path is
still not an error (the storage's rule).

## For the shell and for activation

- The shell needs its own texts for `NoSpace` (a folder nested deeper than 16, a path over
  the cap) and `NoMemory` (a refused allocation); interface 6 had no such messages.
- Measure a folder of 1,000 files on the X4 when this is activated: a page of 8 entries
  is one directory pass (`Folder::after`/`before`), so a page costs a pass over the folder and
  a copy of a big folder costs one pass per 8 entries. Raise the page (a few more names on the
  heap) if the measurement asks for it.

## Not ported yet

- The `Plugin`/`Browser` screens on `core.ui` compose (Input H3), the choice overlay, the
  keyboard and confirmation replies (`Question::choice`/`Ui::ask`), the registration (action,
  Tools menu entry order 45, icon `folder`, the `file-manager.hidden` setting) as in
  `plugins/power/PORTING-7.md`.
- The `core.files` adapter over `Storage` for the card (root 1, `uses.storage`): copy has no
  host op (`files.md`: the plugin copies with `read_at` and `append`), `move_entry` replaces
  a file; the model's `rename` call needs rename to replace and to work across folders.
- The reading-app bookkeeping (`library::books_in`, `formats().supports_file`, own data
  folders): `core.library` (op 15 / Services) and `core.format`.
- Reading `size` for the row text (`ops::shown_size`, from the SDK) and the page-turn
  state of the screen.

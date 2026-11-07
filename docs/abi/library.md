# Interface 7 family LIB: the library, reading state and book data (draft)

Contract draft for #228 (assignment #1025). Items marked [D] are proposed decisions, [Q] open
questions for Plugin and Senior. Inputs: the plugin needs inventory (#984,
`plugin-needs-reg-uix-dev-lib-doc-228.md`, LIB section and coverage ledger), the C-family proposals
(#890 2.7 LIB, Senior's N9: shared reading data as services, not files), the interface-6 library
(`paperpaw-sdk` `library`, `positions`, `annotations`), `document.md` (image ids, book key,
document cache), `ui.md` (host texts for long values), `core-events.md` (book events 65-69) and the
storage contract (D2, owner decisions 124 and 125). Doc-only; records follow once approved. Tags are
proposed and numbered without gaps at the records step.

## 0. Scope and principles

- [D] **Shared reading state is the host's.** The catalog of books, their status (finished,
  favourite, opened), reading positions, annotations (bookmarks, highlights, notes), plugins' book
  fields and badges, and the data plugins keep per book live under `.paperpaw/`, which root 1 never
  shows (decision 125). Plugins reach them only through the service `core.library` (section 2),
  which keeps them consistent, atomic and announced. Interface 6's direct file access
  (`positions::save`, `annotations::save`, `catalog::note`) has no interface-7 copy.
- [D] **One library for 1.0.** The host's walk and catalog (the kernel library of interface 6) is
  the only catalog. A plugin that knows more about books (calibre's metadata) **provides** fields to
  it (section 5). Interface 6 could also **replace** the library with one of a plugin's own (its
  `Library` trait; `files/tests/library_service.rs` tests a remote library with books not on the
  card); no shipped plugin does. This draft does not carry replacement: a deliberate capability
  loss, not parity. [Q] Manager (owner if needed): accept for 1.0, with a later additive
  `core.library-source` stage (a plugin lists books of its own, shown beside the card's under path
  scheme `<plugin>:`), or require it now (about a day more of contract and host work)?
- [D] **Nothing is lost on a move, a rename or an export** (inventory rule 1): positions,
  annotations, fields and book data follow the book when the host moves it, and when the user moves
  it on a computer the host finds them again by the book's fingerprint (section 6).
- [D] **Every change is visible to all** (rule 2): each one raises `event.library_changed` (91) and,
  for a book's file, EVT's 68/69. Readers of a value never cache it across that event.
- [D] **Feature `uses.library`** (signed metadata, as `uses.storage`): every op below needs it
  (`ERR_UNSUPPORTED` without). Ops that remove or move the user's files (`delete`, `move`) also need
  `uses.storage` (decision 125). `import` does not: it only puts a file the plugin made into the
  library folder (section 2.3), so downloaders need no card-wide access.
- Out of LIB: opening and reading books (DOC), the catalog walk's internals (host), sync protocols
  (koreader, kosync: plugins over NET/SOCK).

## 1. Declarations

| Tag | Record | Fields |
| --- | --- | --- |
| 231 | `book_field.spec` | `name: str 96`, `title: str 96`, `flags: u32` |
| 232 | `book_badge.spec` | `name: str 96`, `token: u32`, `flags: u32` |
| 233 | `book_data.spec` | `name: str 96`, `kind: u32` |

- [D] `book_field.spec` (interface 6 `BookField`): a field `<plugin>.<name>` the plugin keeps with
  books (a rating, "on loan"), `title` a catalog key; `flags` bit 0 `in_catalog` (a short value, at
  most 64 bytes, kept in the catalog so queries and rows show it), else at most 1,024 bytes kept per
  book. Other bits `ERR_INVALID`. At most 8 per plugin, 64 device-wide.
- [D] `book_badge.spec` (interface 6 `BookBadge`): a badge on a book's row, answered by its owner
  through the host-defined stage `core.book_badge` (major 1, generated stage schema; input
  `badge.ask` with up to 16 book keys of a page, reply up to 16 `badge.value`: `key`, `text: str 16`
  a few letters, or `icon: str 64`); asked when a page of rows is read, never while drawing; views
  show two at most. `flags` reserved, 0.
- [D] `book_data.spec` (interface 6 `add_book_data`, `add_book_cache`): the plugin keeps a folder
  per book in its own Data (root 0) at `books/<name>/<book key>/` (the book key of section 6);
  `kind` 0 user data (moved with the book, removed with it), 1 cache (made again from the book: also
  removed when the book is no longer found, at the walk's clean-up). The host moves and removes
  these folders; the plugin only writes inside them. At most 4 per plugin.

## 2. The service `core.library` (major 1)

Bare reply streams and paging as `core.registry` (`registry.md` 2.1), `page_rows` 32 (whole rows
that fit 4,096 bytes). Two kinds of generation, never mixed up:

- the **catalog generation** (`lib_state.generation`, `lib_query.generation`): it changes on every
  change that can add or remove a book from a query's result or reorder it: a book added, removed or
  moved, a walk step that found books, a status change, an in-catalog field change, a book opened
  (the "recently opened" order), and progress crossing from never opened to opened or to finished.
  A plain progress change does not (no order or filter uses the percent). A paged call with a stale
  catalog generation is `ERR_STATE`: start again; no page skips or repeats a book.
- the **annotation generation** of one book (`lib_page.annotation_generation`,
  `lib_edit_head.annotation_generation`): it changes on every committed annotation change of that
  book only.

| Op | Name | Input | Reply | Effect |
| --- | --- | --- | --- | --- |
| 1 | `state` | (none) | `lib_state` | query |
| 2 | `query` | `lib_query` | `lib_entry`*, `end` | query, paged |
| 3 | `book` | `lib_ref` | `lib_book` | query |
| 4 | `text` | `lib_text_at` | `lib_text` | query: one long text or list item, from an offset |
| 5 | `facets` | `lib_query` (`facet`) | `lib_facet`*, `end` | query, paged: authors, series, tags, letters, folders |
| 6 | `next_in_series` | `lib_ref` | `lib_entry` | query, `ERR_NOT_FOUND` for none |
| 7 | `cover` | `lib_cover` | `doc_image` | query: an image id (`document.md` 5) |
| 8 | `set_status` | `lib_status` | (none) | mutate |
| 9 | `can_delete` | `lib_paths` | `lib_result`* | query: the delete preflight |
| 10 | `delete` | `lib_paths` | `lib_result`* | mutate, card; `uses.storage` |
| 11 | `move` | `lib_move` | `lib_result` | mutate, card; `uses.storage` |
| 12 | `import` | `lib_import` | `lib_result` | mutate, card |
| 13 | `position` | `lib_ref` | `lib_position` | query |
| 14 | `set_position` | `lib_position_set` | `lib_result` | mutate |
| 15 | `annotations` | `lib_page` | `lib_annotation`*, `end` | query, paged |
| 16 | `anchors` | `lib_page` | `lib_anchor`*, `end` | query, paged |
| 17 | `edit_begin` | `lib_edit_head` | `lib_transaction` | mutate: a staged transaction |
| 18 | `edit_add` | `lib_transaction_ref`, `lib_edit`* | (none) | mutate (staged) |
| 19 | `edit_text` | `lib_edit_text` | (none) | mutate (staged): a long text or note in parts |
| 20 | `edit_commit` | `lib_transaction_ref` | `lib_committed` | mutate, compare-and-swap |
| 21 | `edit_abort` | `lib_transaction_ref` | (none) | mutate |
| 22 | `fields` | `lib_ref` | `lib_field`*, `end` | query |
| 23 | `set_field` | `lib_field_set` | (none) | mutate, own fields only |
| 24 | `provide` | `lib_provided` | (none) | mutate (section 5) |
| 25 | `book_key` | `lib_ref` | `lib_key` | query (section 6) |

- [D] `lib_ref` names a book by `path` (root 1, at most 512 bytes) or by `key`: exactly one of them,
  else `ERR_INVALID`; an unknown book `ERR_NOT_FOUND`. `key` here is the **catalog handle**
  (`lib_entry.key`, a `u32`, interface 6's catalog offset that "fits 32 bits") with the catalog
  generation it came from (stale: `ERR_STATE`). It is not the **book key** of section 6 (a 16-hex
  string of the path), which only names per-book folders.

### 2.1 The catalog

- [D] `lib_state`: `state` (0 no books yet, 1 walking: the books are those found so far, interface 6
  `core.library-walking`; 2 ready), `books`, `generation`, `walk_ppm`.
- [D] `query` (interface 6 `Query`, `Show`, `Sort`): `show` (0 all, 1 unread, 2 reading, 3 finished,
  4 favourites, 5 new: added in the last 30 days and not opened), `sort` (0 title, 1 author, 2
  series, 3 added, 4 recently opened), `flags` (bit 0 descending, bit 1 leave out the archive
  folder), `words` (any number of words, each found in the title, author, series, tags or file name,
  any case; at most 256 bytes in all, a longer search is `ERR_NO_SPACE`, never shortened), `author`,
  `series`, `tag`, `folder` (this folder and below), `added_since` (a day since 1970), `cursor`,
  `generation`. `end.total` is the number found (interface 6 `found`). No result cap: the files
  shelf's 500 is its own.
- [D] `lib_entry` (one row, short enough that 10 fit a page): `key`, `title: str 128`, `author: str
  96`, `series: str 64`, `number: str 16`, `progress` (0-100, 255 never opened), `flags` (bit 0
  favourite, 1 opened, 2 finished, 3 new, 4 indexed, 5-7: title, author, series cut for the row),
  `added`, `badges: str 64` (the first two badges, `\n` between, section 1). A row's texts are cut
  only for the row; op 3 and op 4 have them whole.
- [D] `lib_book` (op 3): `key`, `generation`, `path: str 512`, `title: str 512`, `author: str 512`,
  `series: str 256`, `number: str 32`, `progress`, `flags` (as `lib_entry`, bits 5-7 the long form
  was cut too), `added`, `size: u64`, `format: str 128`, `tags` and `fields` (their counts, no cap),
  `title_bytes`, `author_bytes`, `series_bytes`. Op 4 (`lib_text_at`: `lib_ref`, `kind` 0 title, 1
  author, 2 series, 3 tag, 4 in-catalog field `id=value`, 5 annotation text, 6 annotation note;
  `index` for lists below the count, else `ERR_NOT_FOUND`; `offset`) answers one `lib_text` (`kind`,
  `index`, `total`, `text` at most 1,024 bytes from `offset`): every list item is reachable by its
  index and every text whole by offsets.
- [D] `facets` (`lib_query.facet`: 0 authors, 1 series, 2 tags, 3 first letters, 4 folders; the
  other query fields filter): rows `lib_facet` (`text: str 256`, `count`, `first`: the index of its
  first book in the same query sorted by that facet, for jumping).
- [D] `cover` (`lib_cover`: `lib_ref`, `side` 0 for the list's size, else the longest side, at most
  the screen's long side; the default is interface 6 `cover_fit`, the larger of 300 and 3/8 of the
  screen's long side): the catalog's cover as an image id of the caller, made from the book's format
  (DOC op 15) on first use and kept in the host's cover cache; `ERR_NOT_FOUND` for a book without
  one; while it is being made `ERR_BUSY` and `event.library_changed` bit 6 when it is ready
  (interface 6 lazy covers).

### 2.2 Status, positions, annotations

- [D] `set_status` (`lib_status`: `lib_ref`, `set`, `clear`: bit 0 finished, 1 favourite, 2 opened):
  both masks applied together, idempotent; `finished` set raises nothing more (EVT 67 is the
  reader's session event, not this).
- [D] **Positions** (interface 6 `Position`): `lib_position` (`section`, `offset`, `insert`,
  `progress_ppm`, `conversion: u64` with `flags` bit 1 `has_conversion` (interface 6
  `Option<u64>`: absent and 0 differ), `updated: i64` seconds since 1970, 0 for an unknown clock,
  `xpointer: str 1024` with `flags` bit 0 `has_xpointer`, `flags` bit 2 `from_sync`).
  `set_position` (`lib_position_set`: `lib_ref`, the position, `flags` bit 0 `sync`, bit 1
  `confirmed`) writes atomically and answers `lib_result.status`:
  - a **local** write (the reader, any write without `sync`) always wins: it is what the user did
    on this device, whatever the clock says (an X4 without a valid clock writes 0);
  - a **sync** write (kosync, koreader pulling another device's place) wins only over a stored
    position that is also from sync and older, or that has an unknown clock and is from sync; it
    never overwrites a local position silently: then it answers `ERR_STATE` (observable, the
    plugin may ask the user) and stores nothing; an equal `updated` keeps the stored one;
  - a sync write with `confirmed` (the user chose "go to the other device's place",
    `kosync/confirm.rs`) counts as local.
- [D] **Annotations** (interface 6 `Annotation`, `Mark`): `annotations` (`lib_page`: `lib_ref`,
  `cursor`, `annotation_generation`) rows `lib_annotation` (`id`, `kind` 0 bookmark, 1 highlight,
  2+n other(n), `start_section`, `start_offset`, `end_section`, `end_offset`, `flags` bit 0 noted, 1
  text cut, 2 note cut, 3 has a conversion, 4 has XPointers, `conversion: u64`, `percent`, `created:
  i64`, `updated: i64`, `client_key: str 64`, `text: str 512`, `note: str 512`), in reading order;
  `end.generation` is the book's annotation generation. A cut text or note is read whole with op 4
  (kinds 5, 6, `index` the id).
- [D] **Anchors** (interface 6 `annotations.rs:134` conversion and start/end XPointers, needed by
  `relocate`): op 16 rows `lib_anchor` (`id`, `flags` bit 0 has a conversion, bit 1 has XPointers,
  `conversion: u64`, `start: str 1024`, `end: str 1024`) for every annotation that has them. An
  annotation's offsets belong to its own conversion; the host never remaps them, and a reader whose
  conversion differs relocates them through the XPointers (DOC ops 10, 11) and writes them back.
- [D] **Ids** are stable for the book's life: a `u32` from the book's own counter, never reused,
  kept in the annotation file, unchanged by edits of other annotations and across generations.
  `client_key` (optional, at most 64 bytes, unique per book) is the caller's own identity for an
  annotation (a KOReader highlight's datetime and position), so a repeated import adds nothing
  twice.
- [D] **Transactions** (koreader saves a whole merged set at once, `koreader/lib.rs:761`):
  `edit_begin` (`lib_edit_head`: `lib_ref`, `annotation_generation` the caller read) opens one
  staged transaction (`lib_transaction`: `transaction`, a `u32`) on the book; `edit_add` adds up to
  3 `lib_edit` per call (`action` 0 add, 1 replace, 2 remove, 3 upsert by `client_key`; `id` for 1
  and 2; the annotation's fields and anchors as above, texts and notes up to 512 bytes inline,
  longer ones then in parts with `edit_text`: `transaction`, `edit` its index in the transaction,
  `field` 0 text, 1 note, 2 start XPointer, 3 end XPointer, `offset`, `data` at most 3,072 bytes,
  `flags` bit 0 last); `edit_commit` applies all edits or none: if the book's annotation generation
  moved since `edit_begin`, `ERR_STATE` and nothing changes (read again, merge, begin again); an
  unknown id `ERR_NOT_FOUND` and a range outside the sections `ERR_INVALID` also refuse the whole
  commit. It answers `lib_committed` (`annotation_generation`, and the ids of the added annotations
  in order, as `lib_result` rows). A plugin advances its own sync state only after a commit answered
  `OK`.
- [D] Transaction limits and lifetime: one open transaction per plugin and two device-wide
  (`ERR_FULL`); it lives across callbacks until commit, abort or 10 minutes without a call (then
  aborted, logged) or the plugin stops; staged edits are kept in a host temp file on the card, not
  in memory (at most the annotation file's 1 MiB cap, `annotations.rs:45`, `ERR_NO_SPACE` beyond). A
  power cut aborts every open transaction (the temp file is removed at start); the book's
  annotations stay as they were. Notes keep interface 6's 3 KiB cap per note.

### 2.3 Files: delete, move, import

- [D] `can_delete` (`lib_paths`) answers per path `lib_result` (`path`, `status`, `flags`): `OK`
  deletable, `ERR_INVALID` not a book (unknown format by extension or sniffing, DOC op 20),
  `ERR_NOT_FOUND` gone already (a delete only forgets it), `ERR_IO` the card cannot, and `flags` bit
  0 has annotations, bit 1 has a position, bit 2 has plugins' book data: the view decides what to
  offer and what to say before it asks. Interface 6 `Library::can_delete`. It changes nothing.
- [D] `delete` (`lib_paths`: at most 7 paths, section 7.1, `flags` bit 0 `with_notes`): removes each
  book and everything kept for it (position, annotations, fields, every plugin's `book_data`
  folders, the document caches, the cover), interface 6 `Library::delete`. Only books are removed:
  anything else `lib_result.status` `ERR_INVALID`. A book with annotations is `ERR_STATE` unless
  `with_notes` (the view asked the user; read-later and wallabag pass it for their own downloads).
  One `lib_result` per path.
- [D] `move` (`lib_move`: `from`, `to`, root 1 both, `flags` bit 0 replace an existing `to`): the
  file and everything kept for it follow (keys change, section 6); EVT 68 and `library_changed`.
- [D] `import` (`lib_import`: `from` a file in the caller's root 0, `folder` a folder below the
  library folder setting, empty for the library folder itself, `name` the file name, `flags` bit 0
  replace): copies, syncs and renames into root 1, then catalogues it (indexed on the next walk
  step), and removes `from`. It answers the book's root-1 path in `lib_result`. Downloaders (opds,
  news, wallabag, read-later, transfer) need no `uses.storage` for it.

## 3. Events

| Tag | Record | Fields |
| --- | --- | --- |
| 91 | `event.library_changed` | `now_ms: u64`, `what: u32`, `generation: u32`, `path: str 512` |

- [D] A hint (registrable, coalesced per plugin): `what` bit 0 books added, removed or moved (a new
  generation), 1 status, 2 positions, 3 annotations, 4 fields or badges, 5 the walk progressed or
  ended, 6 a cover is ready; `path` the one book it was about, empty for several (re-read what you
  show). The changer gets it too, so every view updates the same way.

## 4. Book data and badges

- [D] **Book data folders** (section 1) are in the owner's root 0, so the owner reads and writes
  them with the storage slots 20-26 as any Data file. Op 25 gives the book key; the folder path is
  `books/<name>/<key>/`. On a move the host renames them (journalled, section 6), on a delete or an
  uninstall it removes them (an uninstall asks first about the whole Data folder, decision 124).
- [D] **Fields** (`lib_field`: `id`, `title` translated, `value: str 1024`): op 22 lists a book's
  fields of every plugin; `set_field` (`lib_field_set`: `lib_ref`, `field` one of the caller's own,
  `value`, empty removes) changes the caller's only (`ERR_DENIED` for another's). In-catalog fields
  over 64 bytes `ERR_NO_SPACE`.

## 5. Provided metadata (calibre)

- [D] `provide` (`lib_provided`: `lib_ref`, `title`, `author`, `series`, `number`, `tags` one per
  line at most 32, `flags` bit n: field n is provided, the rest left alone) sets what another source
  knows better than the file (interface 6 `library::provide`). Provided values win over the file's,
  and the user's own edits (a later REG or files action) win over provided ones; the last provider
  wins among providers. Kept per book with the book's data (it follows moves).

## 6. Keys, moves, power loss, card failure

- [D] **Book key:** `lib_key.key` is the 16 hex digits of FNV-1a 64 of the book's root-1 path
  (interface 6 `library::key`), what plugins name their per-book folders by. A move by the host
  changes it, and the host renames what it keeps under the old key.
- [D] **Fingerprint:** `lib_key.fingerprint: str 32` is interface 6's `Fingerprint`
  (`library/fingerprint.rs`): the file's size and KOReader's partial MD5 (up to twelve 1 KiB reads),
  so kosync and koreader keep matching other devices. It is kept with each book's data; a book moved
  outside the device is found again at the next walk by it: when a book's path is gone and exactly
  one new file has its fingerprint, the data follows that file. Two or more files with one
  fingerprint are copies, not a move: none of them inherits the data (logged), and a view may offer
  to attach it to one.
- [D] **Atomic writes.** Positions, annotation files, fields and status are each written as temp,
  sync, rename (D2.5); a power cut leaves the old state or the new one, never a mix.
- [D] **Journalled moves and deletes.** `move` and `delete` write a journal entry (`from`, `to` or
  the removal) into `.paperpaw/library/journal`, synced, before touching anything; then the file,
  then the kept data (renamed into `.paperpaw/.removing/` for a delete, removed there). At the next
  start the host finishes or rolls back every journalled step (fault-injection tests after every
  write, as the uninstall data removal of decision 124). The catalog walk never runs while a journal
  entry is open.
- [D] **Card failure.** A missing or failing card is `ERR_IO` for every op that reads the card (no
  partial rows: a page is answered whole or not at all); a full card `ERR_NO_SPACE` for writes, the
  old state stays. A pulled card raises `library_changed` bit 0 when it is back (the walk checks
  it).

## 7. Sizes and memory

### 7.1 Every record fits its limit

| Record or reply | At its caps | Limit |
| --- | --- | --- |
| `lib_entry` page | row 1 + 4 + 130 + 98 + 66 + 18 + 4×3 + 66 + 5 = 400; 10 rows + `end` = 4,030 | 4,096 |
| `lib_book` | 1 + 4×2 + 514 × 3 + 258 + 34 + 4×3 + 8 + 130 + 4×5 + 5 = 2,018 | 4,096 |
| `lib_text` | 1 + 12 + 1,026 + 5 = 1,044 | 4,096 |
| `lib_query` | 1 + 4×5 + 258 × 3 + 130 + 514 + 4×2 + 5 = 1,452 | 4,096 |
| `lib_annotation` page | 1 + 4×8 + 8×3 + 66 + 514 + 514 + 5 = 1,156; 3 rows + `end` = 3,498 | 4,096 |
| `lib_anchor` page | 1 + 4×2 + 8 + 1,026 × 2 + 5 = 2,074 at the caps (one row); real XPointers are about 100 bytes, so a page holds the rows that fit | 4,096 |
| `edit_add` input | 1 + `lib_transaction_ref` 10 + 3 × `lib_edit` (1 + 4×11 + 8×3 + 66 + 514 + 514 + 5 = 1,168) = 3,515 | 4,096 |
| `edit_text` input | 1 + 1 + 4×5 + 3,074 + 5 = 3,101 | 4,096 |
| `lib_paths` (16 paths) | 16 × 514 = 8,224 | over: at most 7 paths (3,605) |
| `lib_position` | 1 + 4×4 + 8×2 + 1,026 + 4 + 5 = 1,068 | 4,096 |
| `badge.value` × 16 | 16 × (1 + 4 + 18 + 66 + 5) = 1,504 | 3,840 |

- [D] So `edit_add` takes at most **3** edits per call inside one staged transaction (any number of
  calls, one atomic commit), and `delete` and `can_delete` at most **7** paths per call.

### 7.2 Memory (X4)

- The catalog, the walk and its sorted index are the host's library of interface 6 (#280, #328,
  #98), measured there; LIB adds none of it.
- Calls: one reply on the host scratch, freed after the call; cursors are offsets into the sorted
  index under the generation (no state per caller).
- Transactions: staged edits go to a host temp file on the card (no heap beyond a 4 KiB buffer
  while a call writes them); at most 2 open device-wide.
- Edits: one transaction's annotation file is rewritten in a streaming copy (read old, write temp
  with the edits, sync, rename), at most one 4 KiB buffer.
- Covers: image ids as DOC (24 bytes each).
- Journal: one entry in memory while a move or delete runs (about 1.1 KiB for two paths).
- **X4 worst case for LIB's host side: about 6 KiB beyond today's library** (edit buffer 4, journal
  1.1, scratch shared), all fallible; an activation-gate item measured with the files and reader
  ports.

## 8. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `library::service`, `Library::{book, catalog_state, each, records, version, query, found}` | ops 1-3 |
| `Library::{index, is_current, refresh, keep_index, kept_index, record}`, `Index` | `query` pages under the generation; views keep their own pages |
| `Index::next_in_series`, letters, series stacks | ops 5, 6 |
| `Library::{set_finished, set_favourite}`, `catalog::note` | op 8 |
| `Library::{can_delete, delete}`, `Deleted` | ops 9, 10 (`lib_result`) |
| `library::cover`, `cover_variant` | op 7 |
| `positions::{load, load_for_update, save, save_with_book, book}` | ops 13, 14 |
| `annotations::{load, marks, save, add, remove, set_note, note_of, relocate, stamp, changed}` | ops 15-21 (anchors, staged transactions); event 91 |
| `Registrar::add_book_field`, `library::set_field`, `Book::fields` | `book_field.spec`; ops 22, 23 |
| `Registrar::add_book_badge`, `BadgeFn`, `badges` | `book_badge.spec`; stage `core.book_badge` |
| `Registrar::add_book_data`, `add_book_cache`, `kept_path`, `move_data`, `remove_data`, `clean_up` | `book_data.spec`; op 25; host moves and removes |
| `library::provide`, `Provided` | op 24 |
| `library::key`, `Fingerprint` | op 25 |
| `Library::extended` (`core.library-walking`) | `lib_state.state` 1 |
| moving a downloaded book into the library | op 12 |

## 9. Hand-offs (after approval)

- **Kernel:** records, `core.library@1` schema, `core.book_badge@1` stage schema, the `uses.library`
  feature, this document final.
- **Core:** the service over the host library, the journal with fault-injection tests, the
  annotation transaction (streamed rewrite, CAS), book data folder moves and removals, provided
  values, the event; the cover cache through DOC.
- **Services:** the SDK facade (a typed query builder, annotation transactions with retry,
  positions, fields, book data paths).
- **Plugin:** the ports: files, file-manager, reader, end-of-book, stats, notes-overview, koreader,
  kosync, setup, sleep, news, opds, transfer, wallabag, read-later-cloud, calibre.

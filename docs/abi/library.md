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
- [D] **One library.** The host's walk and catalog (the kernel library of interface 6) is the only
  catalog. A plugin that knows more about books (calibre's metadata) **provides** fields to it
  (section 5); interface 6's replaceable `Library` trait has no copy. [Q] Plugin: does any plugin
  replace the whole library rather than add to it?
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
that fit 4,096 bytes). The **catalog generation** (`lib_state.generation`) changes when keys may
mean other books (a walk, a book added, removed or moved); a paged call with a stale generation is
`ERR_STATE` (start again). Status, fields and positions are read live (as REG values).

| Op | Name | Input | Reply | Effect |
| --- | --- | --- | --- | --- |
| 1 | `state` | (none) | `lib_state` | query |
| 2 | `query` | `lib_query` | `lib_entry`*, `end` | query, paged |
| 3 | `book` | `lib_ref` | `lib_book`, `lib_text`* | query |
| 4 | `text` | `lib_text_at` | `lib_text` | query: the rest of a long text |
| 5 | `facets` | `lib_query` (`facet`) | `lib_facet`*, `end` | query, paged: authors, series, tags, letters, folders |
| 6 | `next_in_series` | `lib_ref` | `lib_entry` | query, `ERR_NOT_FOUND` for none |
| 7 | `cover` | `lib_cover` | `doc_image` | query: an image id (`document.md` 5) |
| 8 | `set_status` | `lib_status` | (none) | mutate |
| 9 | `delete` | `lib_paths` | `lib_result`* | mutate, card; `uses.storage` |
| 10 | `move` | `lib_move` | `lib_result` | mutate, card; `uses.storage` |
| 11 | `import` | `lib_import` | `lib_result` | mutate, card |
| 12 | `position` | `lib_ref` | `lib_position` | query |
| 13 | `set_position` | `lib_position_set` | (none) | mutate |
| 14 | `annotations` | `lib_page` | `lib_annotation`*, `end` | query, paged |
| 15 | `edit_annotations` | `lib_edit_head`, `lib_edit`* | `lib_generation` | mutate, compare-and-swap |
| 16 | `fields` | `lib_ref` | `lib_field`*, `end` | query |
| 17 | `set_field` | `lib_field_set` | (none) | mutate, own fields only |
| 18 | `provide` | `lib_provided` | (none) | mutate (section 5) |
| 19 | `book_key` | `lib_ref` | `lib_key` | query (section 6) |

- [D] `lib_ref` names a book by `path` (root 1, at most 512 bytes) or by `key` (`lib_entry.key`,
  with the `generation` it came from): exactly one of them, else `ERR_INVALID`; an unknown book
  `ERR_NOT_FOUND`; a stale generation `ERR_STATE`.

### 2.1 The catalog

- [D] `lib_state`: `state` (0 no books yet, 1 walking: the books are those found so far, interface 6
  `core.library-walking`; 2 ready), `books`, `generation`, `walk_ppm`.
- [D] `query` (interface 6 `Query`, `Show`, `Sort`): `show` (0 all, 1 unread, 2 reading, 3 finished,
  4 favourites, 5 new: added in the last 30 days and not opened), `sort` (0 title, 1 author, 2
  series, 3 added, 4 recently opened), `flags` (bit 0 descending, bit 1 leave out the archive
  folder), `words` (each in the title, author, series, tags or file name, any case, at most 8),
  `author`, `series`, `tag`, `folder` (this folder and below), `added_since` (a day since 1970),
  `cursor`, `generation`. `end.total` is the number found (interface 6 `found`). No result cap: the
  files shelf's 500 is its own.
- [D] `lib_entry` (one row, short enough that 10 fit a page): `key`, `title: str 128`, `author: str
  96`, `series: str 64`, `number: str 16`, `progress` (0-100, 255 never opened), `flags` (bit 0
  favourite, 1 opened, 2 finished, 3 new, 4 indexed, 5-7: title, author, series cut; read them whole
  with op 3), `added`, `badges: str 64` (the first two badges, `\n` between, section 1). A row's
  texts are cut only for the row; op 3 has them whole.
- [D] `lib_book` (op 3): `key`, `generation`, `path: str 512`, `title: str 512`, `author: str 512`,
  `series: str 256`, `number: str 32`, `progress`, `flags`, `added`, `size: u64`, `format: str 128`,
  then `lib_text` rows for its tags and the in-catalog fields (`kind` 1 tag, 2 field `id=value`),
  each `text: str 1024` with `total`; a longer one is read with op 4. Never cut for good.
- [D] `facets` (`lib_query.facet`: 0 authors, 1 series, 2 tags, 3 first letters, 4 folders; the
  other query fields filter): rows `lib_facet` (`text: str 256`, `count`, `first`: the index of its
  first book in the same query sorted by that facet, for jumping).
- [D] `cover` (`lib_cover`: `lib_ref`, `side` 0 for the list's size, at most 300): the catalog's
  cover as an image id of the caller, made from the book's format (DOC op 15) on first use and kept
  in the host's cover cache; `ERR_NOT_FOUND` for a book without one; while it is being made
  `ERR_BUSY` and `event.library_changed` bit 6 when it is ready (interface 6 lazy covers).

### 2.2 Status, positions, annotations

- [D] `set_status` (`lib_status`: `lib_ref`, `set`, `clear`: bit 0 finished, 1 favourite, 2 opened):
  both masks applied together, idempotent; `finished` set raises nothing more (EVT 67 is the
  reader's session event, not this).
- [D] **Positions** (interface 6 `Position`): `lib_position` (`section`, `offset`, `insert`,
  `progress_ppm`, `conversion: u64`, `updated: i64` seconds since 1970 or 0, `xpointer: str 1024`,
  `flags` bit 0 has an XPointer). `set_position` writes one atomically; a write whose `updated` is
  older than the stored one is ignored (`OK`, flag `stale` in the log): the newest place wins, on
  any device order (kosync).
- [D] **Annotations** (interface 6 `Annotation`, `Mark`): `annotations` (`lib_page`: `lib_ref`,
  `cursor`, `generation`) rows `lib_annotation` (`id`: stable while the generation holds, `kind` 0
  bookmark, 1 highlight, 2+n other(n), `start_section`, `start_offset`, `end_section`, `end_offset`,
  `flags` bit 0 noted, bit 1 text cut, bit 2 note cut, `percent`, `created: i64`, `updated: i64`,
  `text: str 512`, `note: str 512`), in reading order; `end.generation` is the book's **annotation
  generation**. A cut text or note is read whole with op 4 (`lib_text_at`: `kind` 3 annotation text,
  4 note, `index` the annotation id).
- [D] `edit_annotations` (`lib_edit_head`: `lib_ref`, `generation` the caller read; then at most 3
  `lib_edit` (section 7.1): `action` 0 add, 1 replace, 2 remove, `id` for 1 and 2, the annotation's
  fields as in `lib_annotation`, and `text_id`/`note_id`: a host text (`ui.md` 3.1, a hand-over's
  text) for a text or note over 512 bytes, else 0) applies all of them or none: a generation that is
  not the current one is `ERR_STATE` (read again, merge, retry), an unknown id `ERR_NOT_FOUND`, a
  range outside the book's sections `ERR_INVALID`. It answers the new generation. Two writers can
  never lose each other's edits.
- [D] `relocate` is the reader's: it reads annotations, maps them with the document (DOC ops 10, 11)
  and writes them back with one `edit_annotations`.

### 2.3 Files: delete, move, import

- [D] `delete` (`lib_paths`: at most 7 paths, section 7.1, `flags` bit 0 `with_notes`): removes each
  book and everything kept for it (position, annotations, fields, every plugin's `book_data`
  folders, the document caches, the cover), interface 6 `Library::delete`. Only books (a known
  format by extension or by sniffing, DOC op 20) are removed: anything else `lib_result.status`
  `ERR_INVALID`. A book with annotations is `ERR_STATE` unless `with_notes` (the view asked the
  user; read-later and wallabag pass it for their own downloads). One `lib_result` per path (`path`,
  `status`).
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
  them with the storage slots 20-26 as any Data file. Op 19 gives the book key; the folder path is
  `books/<name>/<key>/`. On a move the host renames them (journalled, section 6), on a delete or an
  uninstall it removes them (an uninstall asks first about the whole Data folder, decision 124).
- [D] **Fields** (`lib_field`: `id`, `title` translated, `value: str 1024`): op 16 lists a book's
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
  (interface 6 `library::key`), what plugins name their per-book folders by. A move changes it; the
  host renames what it keeps under the old key. A **fingerprint** (size and a hash of the first and
  last 4 KiB, interface 6 `Fingerprint`) is kept with each book's data, so a book moved outside the
  device is found again at the next walk and its data is attached to the new key.
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
| `lib_book` + 2 `lib_text` | 1 + 4×2 + 514 + 514 + 514 + 258 + 34 + 4×3 + 8 + 130 + 5 = 1,998; each text 1 + 12 + 1,026 + 5 = 1,044 | 4,086 of 4,096 |
| `lib_query` | 1 + 4×5 + 258 × 3 + 130 + 514 + 4×2 + 5 = 1,452 | 4,096 |
| `lib_annotation` page | 1 + 4×8 + 8×2 + 514 + 514 + 5 = 1,082; 3 rows + `end` = 3,276 | 4,096 |
| `edit_annotations` input | head 1 + 4 + 514 + 4×3 + 5 = 536; each edit 1 + 4×10 + 8×2 + 514 + 514 + 5 = 1,090: 3 edits + head = 3,807 | 4,096 |
| `lib_paths` (16 paths) | 16 × 514 = 8,224 | over: at most 7 paths (3,605) |
| `lib_position` | 1 + 4×4 + 8×2 + 1,026 + 4 + 5 = 1,068 | 4,096 |
| `badge.value` × 16 | 16 × (1 + 4 + 18 + 66 + 5) = 1,504 | 3,840 |

- [D] So `edit_annotations` takes at most **3** edits per call (one transaction; a caller with more
  edits sends them in a row, each with the generation the last answered), and `delete` at most **7**
  paths per call. [Q] Senior: or a multi-call transaction (begin, edits, commit) for a bulk import
  of highlights (koreader)? My leaning: no, the generation chain is enough and keeps the host
  stateless.

### 7.2 Memory (X4)

- The catalog, the walk and its sorted index are the host's library of interface 6 (#280, #328,
  #98), measured there; LIB adds none of it.
- Calls: one reply on the host scratch, freed after the call; cursors are offsets into the sorted
  index under the generation (no state per caller).
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
| `Library::{can_delete, delete}`, `Deleted` | op 9 (`lib_result`) |
| `library::cover`, `cover_variant` | op 7 |
| `positions::{load, load_for_update, save, save_with_book, book}` | ops 12, 13 |
| `annotations::{load, marks, save, add, remove, set_note, note_of, relocate, stamp, changed}` | ops 14, 15; event 91 |
| `Registrar::add_book_field`, `library::set_field`, `Book::fields` | `book_field.spec`; ops 16, 17 |
| `Registrar::add_book_badge`, `BadgeFn`, `badges` | `book_badge.spec`; stage `core.book_badge` |
| `Registrar::add_book_data`, `add_book_cache`, `kept_path`, `move_data`, `remove_data`, `clean_up` | `book_data.spec`; op 19; host moves and removes |
| `library::provide`, `Provided` | op 18 |
| `library::key`, `Fingerprint` | op 19 |
| `Library::extended` (`core.library-walking`) | `lib_state.state` 1 |
| moving a downloaded book into the library | op 11 |

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

# Interface 7 family DOC: formats, documents, images, lookup (draft)

Contract draft for #228 (assignment #1025). Items marked [D] are proposed decisions, [Q] open
questions for Plugin and Senior. Inputs: the plugin needs inventory (#984,
`plugin-needs-reg-uix-dev-lib-doc-228.md`, DOC section and coverage ledger), the C-family proposals
(#890, `c-families-228.md` 2.6 DOC and the IMG wave), the reader extensibility notes (#241, N2,
N4-N7),
`stages-services.md` (stages, services, depth), `ui.md` (host texts, depth and stack rules, the X4
nested-callback gate), `screens-input.md` (N5 pages and progress, tag blocks), `core-events.md`
(book events 65-69) and the storage contract (`proposals/interface-7-events-storage-services.md` D2,
owner decision 125). Doc-only: records, schemas and generator follow once this is approved, as for
EVT, REG and UIX. Tags below are proposed; the records step numbers them without gaps after UIX.

## 0. Scope and principles

- [D] **Two sides, one broker.** A **format plugin** (format-epub, -txt, -docx, -mobi, -images,
  -xtc, notes-vault's markdown) knows how to read a kind of file; an **opener** (the reader,
  browser, files, koreader, kosync, dictionary, notes-vault, sleep for covers) wants a document.
  They never call each other. The opener calls the host service `core.document` (section 2); the
  host calls the format plugin through the host-defined stage `core.format` (section 3), which the
  host alone runs (`stages-services.md` 1.3). Interface 6's `Box<dyn Document>` handed across
  plugins has no interface-7 copy.
- [D] **Documents are host-held.** `open` returns a **document id** (a `u32` from the host's checked
  counter, as UIX instances: never 0, never reused while the host runs). The opener owns it (a
  lease); the format plugin keeps its parse state keyed by it. Every call names it; the host checks
  that the caller is its opener.
- [D] **Least privilege for formats.** A format plugin reads the book through a host root that is
  only that book (root 3) and writes its cache in a host root that is only that book's cache for
  that format (root 2), both only inside its `core.format` callback for that document. It needs no
  `uses.storage` (decision 125) and never sees the opener's data or the user's other files.
- [D] **Layout stays with the reader.** Blocks (section 4) are the interface-6 `Block` model on the
  wire; the reader lays them out with its linked layout crates. Text filters, decorators, page
  layers, page frames and voices are reader-defined stages (`reader.def`, #964, Plugin-owned), not
  kernel records (section 9).
- [D] **The user's reading settings win** (decision 131): publisher styles, user CSS, colour and
  image sizes are the opener's options, from the user's settings; a format plugin never overrides
  them.
- [D] **No interface-6 shapes:** `Document`, `FormatSpec::open`/`sniff` closures, `Formats`,
  `Readers`, `Document::extended` (new questions are new ops of a new revision) and `DocumentError`
  have no interface-7 copies.
- Out of DOC: book data, caches' lifecycle on move or remove, positions and annotations (LIB);
  network fetches by lookup providers (NET/ASY); the reader's own stage schemas (Plugin, #964).

## 1. Declarations

| Tag | Record | Fields |
| --- | --- | --- |
| 31 | `format.spec` | `name: str 96`, `title: str 96`, `extensions: str 512`, `flags: u32`, `priority: i32`, `token: u32` |
| 229 | `reader.spec` | `name: str 96`, `screen: str 128`, `formats: str 1032`, `flags: u32` |

- [D] `format.spec` (interface 6 `FormatSpec`): the format `<plugin>.<name>`; `title` a catalog key;
  `extensions` lowercase, without the dot, one per line, at most 16 of at most 31 bytes (`epub`,
  `fb2.zip`), non-empty, unique within the plugin (`ERR_INVALID`); `token` the plugin's callback
  state for its `core.format` calls. Declaring it subscribes the plugin to `core.format` for this
  format (no `registration.stage`). `flags`: bit 0 `prepared` (the first open converts into the
  cache and takes a while, interface 6 `FormatPrepared`), bit 1 `sniffs` (it can tell its files by
  their first 512 bytes, interface 6 `sniff`), bit 2 `folder` (a book is a folder, format-images),
  bit 3 `whole_pages` (each section is a whole-page picture: comics, XTC), bit 4 `siblings` (root 3
  is the book's folder and below, read only, not the file alone: format-images shows the pictures
  beside the one opened). Other bits `ERR_INVALID`. At most 8 formats per plugin (format-epub has
  five), 32 device-wide (`ERR_FULL`). A format that must read more of the card than its book's
  folder (notes-vault resolving links across a vault) declares `uses.storage` and reads root 1
  itself (decision 125); the store page shows it.
- [D] **Which format reads a file.** By extension (the longest match, `fb2.zip` before `zip`); when
  two formats claim one, the user's choice (host setting `core.formats.<extension>`, a choice of the
  claiming formats, REG-visible) wins, else the higher `priority`, else the first loaded. A file
  whose extension no format has is offered to the `sniffs` formats in priority order (section 3,
  op 1). The host never guesses from the path beyond that.
- [D] `reader.spec` (interface 6 `ReaderSpec`): a reading app `<plugin>.<name>` whose `screen` (one
  of its own screens, `ERR_INVALID` otherwise) shows books of `formats` (format ids, one per line,
  at most 8; empty: every format). `flags` reserved, 0. Op 21 lists them; which one opens a book is
  the opener's choice (the files plugin remembers it in its Data, as interface 6
  `readers::remember`).

## 2. The service `core.document` (major 1)

Bare reply streams and paging as `core.registry` (`registry.md` 2.1), `page_rows` 64 (rows fit by
bytes too: a page holds the whole rows that fit 4,096). Feature: none to list formats and readers;
`open` needs read access to the book's root (root 1 needs `uses.storage`, decision 125; root 0 the
caller's own Data). Every op on a document is the opener's only (`ERR_DENIED` for others).

| Op | Name | Input | Reply | Notes |
| --- | --- | --- | --- | --- |
| 1 | `open` | `doc_open`, `doc_option`* | `doc_state` | section 2.1 |
| 2 | `state` | `doc_ref` | `doc_state` | preparing progress, failure |
| 3 | `meta` | `doc_ref` | `doc_meta` | section 2.2 |
| 4 | `meta_text` | `doc_text_at` | `doc_text` | one author, subject or text, from an offset |
| 5 | `sections` | `doc_page` | `doc_section`*, `end` | paged |
| 6 | `block` | `doc_at` | the block stream (section 4) | the block at or after a position |
| 7 | `toc` | `doc_page` | `doc_toc`*, `end` | paged, in reading order |
| 8 | `toc_target` | `doc_index` | `doc_at` | exact position of an entry |
| 9 | `page_label` | `doc_at` | `doc_label` | printed page name, `ERR_NOT_FOUND` for none |
| 10 | `xpointer` | `doc_at` | `doc_xpointer` | `ERR_NOT_FOUND` where the format has none |
| 11 | `locate` | `doc_xpointer` | `doc_at` | the position of an XPointer |
| 12 | `links` | `doc_span` | `doc_link`*, `end` | links of a section overlapping a range, paged |
| 13 | `link_target` | `doc_href` | `doc_at` | `ERR_NOT_FOUND` for external or dangling |
| 14 | `fonts` | `doc_page` | `doc_font`*, `end` | embedded font faces, paged |
| 15 | `cover` | `doc_ref` | `doc_image` | `ERR_NOT_FOUND` for none |
| 16 | `image` | `doc_image_ask` | `doc_image` | prepare a block's image, or larger (section 5) |
| 17 | `progress` | `doc_at` | `doc_progress` | where the format knows better than section lengths |
| 18 | `close` | `doc_ref` | (none) | |
| 19 | `formats` | `doc_page` | `doc_format`*, `end` | every format, paged |
| 20 | `format_for` | `doc_path` | `doc_format` | which format reads a file; `ERR_NOT_FOUND` |
| 21 | `readers` | `doc_page` (`scope`: a format id or empty) | `doc_reader`*, `end` | reading apps |

| Tag | Record | Fields |
| --- | --- | --- |
| 1060 | `service.doc_open` | `root: u32`, `path: str 512`, `format: str 128`, `flags: u32`, `image_side: u32`, `cover_side: u32`, `css_root: u32`, `css_path: str 512` |
| 1061 | `service.doc_option` | `key: str 64`, `value: str 128` |
| 1062 | `service.doc_state` | `document: u32`, `state: u32`, `progress_ppm: u32`, `status: i32`, `flags: u32`, `conversion: u64`, `format: str 128` |
| 1063 | `service.doc_ref` | `document: u32` |
| 1064 | `service.doc_meta` | `title: str 512`, `language: str 35`, `series: str 512`, `series_index: str 32`, `direction: u32`, `writing_mode: u32`, `sections: u32`, `authors: u32`, `subjects: u32`, `description_bytes: u32`, `flags: u32` |
| 1065 | `service.doc_text` | `kind: u32`, `index: u32`, `total: u32`, `text: str 1024` |
| 1066 | `service.doc_text_at` | `document: u32`, `kind: u32`, `index: u32`, `offset: u32` |
| 1067 | `service.doc_page` | `document: u32`, `scope: str 128`, `cursor: u32`, `generation: u32` |
| 1068 | `service.doc_section` | `index: u32`, `length: u32`, `flags: u32` |
| 1069 | `service.doc_at` | `document: u32`, `section: u32`, `offset: u32`, `insert: u32`, `cursor: u32`, `generation: u32` |
| 1070 | `service.doc_toc` | `index: u32`, `level: u32`, `section: u32`, `offset: u32`, `flags: u32`, `title: str 512` |
| 1071 | `service.doc_index` | `document: u32`, `index: u32` |
| 1072 | `service.doc_label` | `text: str 32` |
| 1073 | `service.doc_xpointer` | `document: u32`, `text: str 1024` |
| 1074 | `service.doc_span` | `document: u32`, `section: u32`, `from: u32`, `to: u32`, `cursor: u32`, `generation: u32` |
| 1075 | `service.doc_link` | `start: u32`, `end: u32`, `kind: u32`, `href: str 1024` |
| 1076 | `service.doc_href` | `document: u32`, `href: str 1024` |
| 1077 | `service.doc_font` | `index: u32`, `family: u32`, `flags: u32`, `bytes: u32` |
| 1078 | `service.doc_format` | `id: str 128`, `title: str 128`, `extensions: str 512`, `flags: u32`, `priority: i32` |
| 1079 | `service.doc_path` | `root: u32`, `path: str 512`, `flags: u32` |
| 1080 | `service.doc_reader` | `id: str 128`, `screen: str 128`, `formats: str 1032`, `title: str 128` |
| 1081 | `service.doc_progress` | `progress_ppm: u32` |

- [D] `doc_page.scope` is op 21's format filter, else empty; `doc_path.flags` bit 0: sniff when no
  extension matches (op 20 reads the first 512 bytes; root 1 needs `uses.storage`).
  `doc_format.title` and `doc_reader.title` are translated by the host as REG rows are. Two-way
  records (plugin and host send them; never `HOST_ONLY`): `doc_state` and the block, meta, toc,
  link, font and image rows, because a format plugin answers them in its `core.format` reply and the
  host passes them on.

### 2.1 Opening, preparing, closing

- [D] `open` (`doc_open`: `root` 0 or 1, `path` at most 512 bytes, `format` empty for section 1's
  choice, `flags`, `image_side`, `cover_side`, `css_root`, `css_path`; then at most 12 `doc_option`
  rows `key`/`value`, interface 6 `OpenOptions::settings`): `flags` bit 0 publisher styles, bit 1
  colour, bit 2 background (an index build, not for showing; section 8); `image_side` the longest
  side of a page's picture (0: interface 6 `IMAGE_FIT`, 1,600), `cover_side` of the cover (0:
  interface 6 `cover_fit`, the larger of 300 and 3/8 of the screen's long side; at most the screen's
  long side); `css_path` a file of user CSS (at most 65,536 bytes, empty for none) on
  `css_root` (0 or 1 under the same rule as `path`). Other flag bits `ERR_INVALID`. The host checks
  the roots, finds the format, gives a new document id and calls the format's op 2 `open`; its
  answer becomes `doc_state`.
- [D] **Options reach their format whole.** An option's `key` is a full setting id
  (`<plugin>.<name>`) with the book's own value (interface 6 gathers them for every format owner,
  `reading/mod.rs:275`); the host forwards to the chosen format exactly the options of its plugin,
  each as one `fmt.option` record beside `fmt.call` (section 3), none cut and none dropped; options
  of other plugins are not for this format and go nowhere. More than 12 options of one format, or an
  option over its caps, is `ERR_NO_SPACE` for the open (never a silent drop).
- [D] `doc_state`: `document`, `state` (0 ready, 1 preparing, 2 failed), `progress_ppm` while
  preparing, `status` (an `ERR_*` when failed), `format` (the format id), `flags` (bit 0 whole
  pages, bit 1 has a table of contents, bit 2 has XPointers, bit 3 has embedded fonts, bit 4 has
  page labels, bit 5 right-to-left pages, bit 6 the cache was made now). A failed open still has an
  id until closed, so the opener can read `status` (and the reader can say "this book is damaged").
- [D] **Preparing** (interface 6 `prepared`): while `state` is 1, the host calls the format's op 3
  `prepare` again and again from its main loop, between inputs, each call for at most 50 ms of work
  by the format's own clock (the format stops at a safe point and answers its progress), until it
  answers done or fails. The opener gets `event.document_changed` (88, a hint per document,
  coalesced) when the state or the progress moves; other ops on a preparing document are
  `ERR_BUSY`. The X4 runs at most one preparation at a time, queued in open order.
- [D] **The cache is whole or not used.** A cache is made for one **conversion**: the book's
  identity (LIB's fingerprint), the format plugin's id and version, and the conversion key, a hash
  of everything that changes the output (publisher styles, colour, `image_side`, `cover_side`, the
  CSS file's content hash, the forwarded options), interface 6 `prepared_keeping` and `PreparedCss`.
  Root 2 is `.paperpaw/cache/<book key>/<format plugin>/<conversion key>/`; the host keeps at most
  two conversions per book (the older goes), and `doc_state.conversion` (a `u64`) names it, which
  LIB stores with positions and annotations (`library.md` 2.2). The host writes `.complete` (the
  stamp of those inputs) after the format answers done and the card is synced; a cache without a
  current stamp (power cut, card pulled, a new format version, other options) is removed and made
  again, so the format never sees a half-made or stale cache as finished.
- [D] `close` frees the id; the format gets op 4 `close` (its state for it goes) and every image id
  of the document (section 5) ends. The host closes the opener's documents when the opener stops,
  and closes a format's documents when the format plugin stops or is uninstalled: the opener gets
  `event.document_closed` (89: `document`, `reason` 1 the format stopped, 2 the card went, 3 the
  book was moved or removed; others treated as 1) and every later call on the id is `ERR_CANCELLED`
  until it closes it. A reply the format produces after that is never delivered (it is not called
  again).
- [D] Limits: at most 2 open documents per opener and 2 on the X4 device-wide (the shown book and a
  background index), 8 elsewhere; over that `ERR_FULL`.

### 2.2 Metadata

- [D] `doc_meta` (interface 6 `Metadata`): `title: str 512`, `language: str 35`, `series: str 512`,
  `series_index: str 32`, `direction` (0 left to right, 1 right to left), `writing_mode` (0
  horizontal, 1 vertical right to left, 2 vertical left to right), `sections`, `authors`, `subjects`
  (their counts, no cap), `description_bytes`, `flags` (bit 0 the title was cut, bit 1 the series).
  Op 3 answers `doc_meta` alone. Op 4 (`doc_text_at`: `document`, `kind` 0 title, 1 author, 2
  subject, 3 description, 4 series; `index` below the count, else `ERR_NOT_FOUND`; `offset`) answers
  one `doc_text` (`kind`, `index`, `total` its whole length, `text` at most 1,024 bytes from
  `offset`, cut at a UTF-8 boundary): an opener lists every author and subject by index and reads
  any text whole by offsets. Nothing is cut for good and nothing is left out.

### 2.3 Navigation and links

- [D] Positions are `doc_at` (`document`, `section`, `offset`, `insert`; `insert` always 0 here, the
  N5 locator's ordinal belongs to the reader's inserted content). `section` past the end
  `ERR_NOT_FOUND`; an offset past a section's end is the end.
- [D] `sections` rows `doc_section` (`index`, `length` in source units for progress, `flags` bit 0 a
  whole-page picture). Op 17 `progress` answers `progress_ppm` (0-1,000,000) where the format
  computes it itself (interface 6 `Document::progress`, EPUB and TXT); elsewhere `ERR_UNSUPPORTED`
  and the reader computes it from the section lengths (N5 rule 1).
- [D] `toc` rows `doc_toc` (`index`, `level` 0-15, `section`, `offset`, `flags` bit 0 approximate:
  `toc_target` finds the exact place, `title: str 512`, cut at a UTF-8 boundary with bit 1). Up to
  10,000 entries (FB2) in pages; the reader shows the first 500.
- [D] `links` (`doc_span`: `document`, `section`, `from`, `to`, `cursor`, `generation`) rows
  `doc_link` (`start`, `end`, `kind` 0 internal, 1 external, 2 footnote, `href: str 1024`). An
  `href` over 1,024 bytes is not listed (logged); `link_target` resolves one inside the book.
- [D] `page_label` (`doc_label: str 32`), `xpointer` and `locate` (`doc_xpointer`: `document`,
  `text: str 1024`) as interface 6 (KOReader XPointers for EPUB, for koreader and kosync).

## 3. The stage `core.format` (major 1)

A host-defined stage (generated stage schema, `abi/stages/core.format@1.def`, stage-local tags;
spec `/home/agent/shared/stage-schemas-spec-228.md`). The host calls the one format plugin that
serves the document, with the `token` of its `format.spec`; nobody else subscribes. Each call's
input is one `fmt.call` record (`op`, `document`, `section`, `offset`, `index`, `cursor`,
`generation`, `flags`, `image_side`, `cover_side`, `conversion: u64`, `text: str 1024`, `arg: bytes
512`) followed, for op 2 only, by the forwarded `fmt.option` records (at most 12: key 64, value 128
bytes); the reply is the op's records (local tags), the same records as the service's rows, so the
host passes them on after checking them.

| Op | Name and input | Rows of the reply | Roots |
| --- | --- | --- | --- |
| 1 | `sniff` (`arg`: the file's first at most 512 bytes) | `fmt.yes` (or none: not mine) | none |
| 2 | `open` (`flags`, sides, `conversion`; `fmt.option`*) | `doc_state` | 2, 3, 4 |
| 3 | `prepare` | `doc_state` (`progress_ppm`; `state` 0 done, 1 more, 2 failed) | 2, 3, 4 |
| 4 | `close` | none | none |
| 5 | `meta` | `doc_meta` | 2, 3 |
| 6 | `meta_text` (`index`, `flags` the kind, `offset`) | `doc_text` | 2, 3 |
| 7 | `sections` (`cursor`) | `doc_section`*, `end` | 2, 3 |
| 8 | `block` (`section`, `offset`, `cursor`, `generation`) | the block stream (section 4) | 2, 3 |
| 9 | `toc` (`cursor`) | `doc_toc`*, `end` | 2, 3 |
| 10 | `toc_target` (`index`) | `doc_at` | 2, 3 |
| 11 | `page_label` (`section`, `offset`) | `doc_label` | 2, 3 |
| 12 | `xpointer` (`section`, `offset`) | `doc_xpointer` | 2, 3 |
| 13 | `locate` (`text`: the XPointer) | `doc_at` | 2, 3 |
| 14 | `links` (`section`, `offset` from, `index` to, `cursor`) | `doc_link`*, `end` | 2, 3 |
| 15 | `link_target` (`text`: the href) | `doc_at` | 2, 3 |
| 16 | `fonts` (`cursor`) | `doc_font`*, `end` | 2, 3 |
| 17 | `cover` (`cover_side`) | `fmt.image` | 2, 3 |
| 18 | `image` (`index` the image, `image_side`) | `fmt.image` | 2, 3 |
| 19 | `progress` (`section`, `offset`) | `doc_progress` | 2, 3 |

- [D] Service ops map one to one: 3 → 5, 4 → 6, 5 → 7, 6 → 8, 7 → 9, 8 → 10, 9 → 11, 10 → 12,
  11 → 13, 12 → 14, 13 → 15, 14 → 16, 15 → 17, 16 → 18, 17 → 19; ops 1, 2, 18-21 of the service
  are the host's own (open, state, close, formats, readers). Sizes: `fmt.call` 1 + 9×4 + 8 + 1,026
  + 514 + 5 = 1,590, with 12 options 4,014: over the 3,840 payload, so op 2 carries `text` and
  `arg` empty (1,590 − 1,536 + 2,424 = 2,478).

- [D] **Roots** (the reserved roots of D2.2, valid only inside a call for that document): root 2 the
  document's cache for its conversion (section 2.1; read and write; the book key is LIB's, so the
  cache follows the book on a move and goes with it, LIB), root 3 the book (read only: `path` "" is
  the file; for a `folder` format the folder and below; for a `siblings` format the book's folder
  and below, the call's `text` naming the opened file within it), root 4 the user CSS file
  (read only, `path` "", during op 2 and 3 only). Any other root, and these outside such a call, are
  `ERR_UNSUPPORTED`. The format writes its cache atomically per file (D2.5); it may not write root
  3.
- [D] **Cache quota.** A document's cache is counted to the format plugin (S7) and capped at 64 MiB
  per book on the X4 (an EPUB with large images; `ERR_NO_SPACE` beyond and on a full card, the
  preparation fails with that status and the host removes the partial cache).
- [D] **Depth and stack.** The opener's call is depth 1 and the format's callback depth 2: the
  format cannot call `stage.run` or `core.document` (`ERR_BUSY`). On the X4 the format callback
  joins the nested-callback frame budget and stack gate of slot 35 (`ui.md` 5.1) with its own
  high-water case (EPUB `block` at a long chapter); until it passes, the X4 runs formats only at
  depth 1 from the host's main loop (ops 2-3) and answers the depth-2 ops from the cache the format
  made: [Q] Senior, or does the X4 need the gate before the reader port can start?
- [D] **Failures.** A format callback that fails, answers malformed rows or rows of another op is a
  failed call: the service op answers `ERR_IO` for a card error, `ERR_INVALID` for a malformed
  answer, else the format's status (logged with the format's id). Three malformed answers in a row
  close the document (reason 1). The format never panics the host; a fault is the plugin's fault
  (STG rules).
- [D] **Time.** `block` runs on every page laid out: its time counts against the reader's X4
  page-turn budget (timing gate of the reader port). A format answers op 6 from its cache, not by
  parsing the book again.

## 4. Blocks on the wire

The interface-6 `Block` (`paperpaw-document`), whole: every field has a record, and nothing a format
says is dropped. A block is a **record list** (header, text parts, runs, maps, columns, cells,
rubies, languages, image) that the format's SDK writer makes from the block deterministically. It
travels in **fragments** of at most 3,840 bytes (the stage payload), cut only between records, never
inside one: the logical block, its paragraph and its table row are never split or changed because a
reply filled up (no flattening, no `continues` invented by the transport). The first call answers
fragment 0; `service.end.flags` bit 1 `more` says the block goes on, with `next` the record index to
ask from and `generation` the block's identity (a hash of section, start and the conversion); the
opener asks op 6 again with `doc_at.cursor` = `next` and that generation (the format makes the same
list again from its cache and answers from that record on). A changed generation is `ERR_STATE`.
The reader joins the fragments into one block. `continues`/`continued` keep only their interface-6
meaning (the format's own split of a very long paragraph).

| Tag | Record | Fields |
| --- | --- | --- |
| 1082 | `service.doc_block` | `kind: u32`, `level: u32`, `flags: u32`, `start: u32`, `end: u32`, `align: u32`, `indent: i32`, `space: u32`, `size: u32`, `margin_start: u32`, `margin_end: u32`, `line_height: u32`, `dir: u32`, `writing_mode: u32`, `lang: str 35`, `drop_cap: u32`, `text_bytes: u32` |
| 1090 | `service.doc_text_part` | `text: str 3072` |
| 1091 | `service.doc_columns` | `weights: bytes 32` |
| 1083 | `service.doc_run` | `start: u32`, `end: u32`, `style: u32` |
| 1084 | `service.doc_map` | `text: u32`, `source: u32` |
| 1085 | `service.doc_cell` | `end: u32`, `span: u32`, `rows: u32`, `align: u32`, `vertical: u32`, `width: u32` |
| 1086 | `service.doc_ruby` | `start: u32`, `end: u32`, `kind: u32`, `size: u32`, `text: str 1024` |
| 1087 | `service.doc_inline_lang` | `lang: str 35` |

- [D] Order of the list: one `doc_block` (`text_bytes` the text's whole length), the text in
  `doc_text_part`s (each at most 3,072 bytes, cut at UTF-8 boundaries, joined in order to
  `text_bytes`), its `doc_run`s (sorted, not overlapping), `doc_map`s (the `OffsetMap` checkpoints),
  for an image block one `doc_image` (section 5), for a table row one `doc_columns` (interface 6
  `TableRow.columns`: up to 16 `u16` weights shared by the table's rows, little-endian) and its
  `doc_cell`s (at most 16, `MAX_COLUMNS`; `flags` bits 8-10 header, first, last row), its
  `doc_ruby`s and up to 15 `doc_inline_lang`s (`MAX_INLINE_LANGS`); the last fragment ends with
  `service.end` (`flags` bit 0: the last block of the section; `next` the offset of the next block).
  Runs, maps and rubies have no count cap (interface 6 has none); every record fits a fragment.
  Another order, an unknown record or a text whose parts do not add up is `ERR_INVALID` (the host
  checks the format's answer).
- [D] `kind`: 0 paragraph, 1 heading (`level` 1-6), 2 preformatted, 3 image, 4 table row, 5+n
  `Extended(n)`; `flags` bit 0 continues, bit 1 continued; `indent`, `space`, `size`, `line_height`:
  `i32::MIN` / `u32::MAX` for "the reader's" (interface 6 `None`); `style` the interface-6
  `TextStyle` bits unchanged (they are already a stable `u32`); `lang` a BCP 47 tag, empty for
  unknown.
- [D] Inserted blocks and `leading_above` are the reader's (its text filters, #964); formats answer
  none, as in interface 6.

## 5. Images (IMG, N6)

Pictures of books (covers, illustrations, comic pages) and the pictures plugins show (picture-home,
sleep): the host keeps decoded images on the card in the `paperpaw-image` gray (or colour) format
and draws them in bands, so no plugin holds a picture in memory.

```
image.info(ctx, image: u32, out: buffer) -> i32                                   // slot 36
canvas.draw_image(ctx, image: u32, x: i32, y: i32, width: u32, height: u32, flags: u32) -> i32
                                                                                   // slot 37
```

| Tag | Record | Fields |
| --- | --- | --- |
| 1088 | `service.doc_image` | `image: u32`, `width: u32`, `height: u32`, `flags: u32` |
| 1089 | `service.doc_image_ask` | `document: u32`, `image: u32`, `side: u32` |
| 1559 | `command.image_prepare` | `root: u32`, `path: str 512`, `side: u32`, `cookie: u32`, `flags: u32` |
| 1560 | `command.image_release` | `image: u32` |
| 90 | `event.image_ready` | `now_ms: u64`, `cookie: u32`, `status: u32`, `image: u32`, `width: u32`, `height: u32` |

- [D] An **image id** (`u32`, checked counter) names a decoded picture the host may draw. A
  document's images come from `doc_image` rows (a block's picture, the cover, op 16's larger one)
  and end with the document; a plugin's own come from `image_prepare` and end with `image_release`
  or when the plugin stops. Image ids are per plugin: another plugin's id is `ERR_NOT_FOUND`.
- [D] A format answers a picture as its cache path (stage-local `fmt.image`: `path: str 256` in root
  2, `width`, `height`, `flags` bit 0 not decoded yet); the host turns it into an image id for the
  opener. Op 16 (`doc_image_ask`: `image`, `side` 0 for the page's size, else the longest side for a
  zoom, at most 1,600) asks the format to decode it now (interface 6 `prepare_image`,
  `prepare_image_larger`): a new image id, or the same where nothing was to do.
- [D] `image_prepare` decodes a JPEG, PNG, GIF (first frame), BMP or WebP of the caller's root 0 or
  root 1 (`uses.storage`) into the host's image cache at most `side` pixels on the longest side (0:
  the screen's), in the background (bounded work per main-loop turn, as preparing);
  `event.image_ready` (to the caller only) tells `status` 0 ready, 1 unsupported, 2 damaged, 3 too
  large (over 8 Mpx source, `XTC` and decoder caps), 4 card failure (`ERR_IO`, no card or full). At
  most 4 waiting per plugin (`ERR_FULL`), 8 prepared images per plugin. `flags` bit 0 colour (where
  the screen has it).
- [D] `image.info` writes one `doc_image` record (`width`, `height`, `flags` bit 0 colour) into
  `out`. `canvas.draw_image` draws the image scaled into the rectangle (keeping its shape, centred;
  `flags` bit 0 fill and crop, bit 1 no dithering, bit 2 inverted for night mode), clipped by the
  canvas, only inside a `draw` callback (`ERR_STATE`); an unknown or ended id `ERR_NOT_FOUND`; a
  card read failure `ERR_IO` (nothing drawn). The host reads the file in bands of at most 2 KiB (the
  X4 frame band), so drawing costs no heap.

## 6. Fonts of a book

- [D] `doc_font` rows, one per **face** (`index` from 0, at most 124: 31 families × regular, bold,
  italic, bold italic; `family` 1-31 as the runs' `TextStyle` family, `flags` bit 0 bold, bit 1
  italic, `bytes` the file's size), paged. The format writes each embedded font (at most 16
  MiB, interface 6's EPUB cap) whole into its cache when it prepares the document; the reader gets a
  font handle with `font.acquire` (slot 7) and the key `document.<document>.<index>`, which the host
  resolves to that cache file and renders from the card like a card font. A damaged font is
  `ERR_INVALID` at `font.acquire` (the reader falls back to the user's font and counts it, interface
  6 `failed_reads`). Handles of a closed document's fonts draw nothing (`ERR_NOT_FOUND` at the next
  text call) and are released as usual.

## 7. Lookup (dictionary, wikipedia, translate)

- [D] A host-defined stage `core.lookup` (major 1, generated schema): providers declare
  `lookup.spec` (230: `name: str 96`, `title: str 96`, `languages: str 512` one BCP 47 tag per line
  or empty for any, `flags` bit 0 network (shown only when a network is up), `token: u32`) and are
  called in the user's order (a host setting listing them, REG). The asker (the dictionary screen,
  the reader's selection menu) calls the service `core.lookup` (major 1) op 1 `lookup` (1092
  `lookup_ask`: `text: str 1024`, `language: str 35`, `context: str 1024`, `cursor: u32`) and gets
  the providers' rows 1093 `lookup_entry` (`provider: str 64`, `title: str 256`, `text: str 2048`,
  `flags` bit 0 more pages, bit 1 pending: the provider answers later) in pages, one provider per
  page.
- [D] A provider that must fetch (wikipedia, translate) answers "pending" at once and finishes with
  ASY's async stage reply by request id (ASY family); the asker gets ASY's completion event and asks
  again with the cursor. Until ASY lands, network providers answer `ERR_UNSUPPORTED` and only local
  ones (stardict dictionaries) answer. No provider sees another's answer; the asker sees all.

## 8. Sizes and memory

### 8.1 Every record fits its limit

Encoded sizes at the caps (as `ui.md` 8.1: bitmap 1 byte; `str N` 2 + N; scalars 4 or 8; a record
frame adds 2 or 3 bytes of tag and 2 of length; a batch its count byte). Limits: service input and
reply 4,096 bytes, stage payload 3,840.

| Record or reply | At its caps | Limit |
| --- | --- | --- |
| `doc_open` + `doc_option`s (key 64, value 128) | `doc_open` 1 + 5×4 + 514 + 130 + 514 + 5 = 1,184; each option 1 + 66 + 130 + 5 = 202: 16 would be 4,417, so at most 12: 1,184 + 2,424 + 1 = 3,609 | 4,096 |
| `block` fragment | `doc_block` 1 + 15×4 + 4 + 37 + 4 + 5 = 111; a `doc_text_part` at most 1 + 3,074 + 5 = 3,080; a `doc_ruby` 1 + 16 + 1,026 + 5 = 1,048; every record fits, fragments are cut between records | 3,840 |
| 64 `doc_run` alone | 64 × (1 + 12 + 5) = 1,152 | in the 3,840 piece |
| `doc_meta`; `doc_text` | 1 + 514 + 37 + 514 + 34 + 4×7 + 5 = 1,133; 1 + 12 + 1,026 + 5 = 1,044 | 4,096 |
| `doc_toc` page | row 1 + 5×4 + 514 + 5 = 540; 7 rows + `end` = 3,810 | 4,096 |
| `doc_link` page | row 1 + 3×4 + 1,026 + 5 = 1,044; 3 rows + `end` | 4,096 |
| `doc_section` page | row 1 + 12 + 5 = 18; 64 rows + `end` = 1,182 | 4,096 |
| `fmt.call` (+ 12 `fmt.option` for op 2) | 1,590 (section 3); op 2: 1,590 − 1,536 + 12 × 202 = 2,478 | 3,840 |
| `lookup_entry` page | 1 + 4 + 66 + 258 + 2,050 + 5 = 2,384, one per page | 4,096 |
| `command.image_prepare` | 1 + 4 + 514 + 12 + 5 = 536 | 4,096 |

- [D] So `doc_open` takes at most **12** `doc_option` rows (key 64, value 128 bytes) per format;
  no first-party format reads options today (Plugin's audit), and more is `ERR_NO_SPACE`, never a
  silent drop. Values are the book's own (section 2.1), so a later global settings read is not a
  replacement.

### 8.2 Memory (X4)

- Documents: about 64 bytes of host state each, at most 2: 128 bytes. The format plugin's parse
  state is its own heap, per document; the format ports measure it (EPUB at a long book) and it
  counts to the plugin, not here.
- Calls: the opener's `out` and one stage call on the host scratch, both at most 4,096 bytes, freed
  when the call returns; nothing retained.
- Preparing: one at a time; the host keeps only the queue (8 bytes per waiting document).
- Images: about 24 bytes per image id, at most 8 per plugin and 64 device-wide (1.5 KiB); decoding
  for `image_prepare` uses one band buffer of at most 4 KiB plus the decoder's state (the image
  crate's, bounded per format; measured with the records), one decode at a time, fallible
  (`status` 4 when the heap cannot).
- Fonts: card fonts, no heap beyond the font engine's per-handle state.
- Lookup: one stage call on the scratch; nothing kept.
- **X4 worst case for DOC's host side: about 6 KiB** (documents 0.13 + images 1.5 + decoder band 4
  + queue), all fallible; an activation-gate item beside UIX's, measured with the reader port. The
  format plugins' own heaps are gated by their ports.

## 9. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `Registrar::add_format`, `FormatSpec { extensions, prepared, sniff }` | `format.spec` (flags `prepared`, `sniffs`, `folder`, `whole_pages`); `core.format` ops 1-3 |
| `Registrar::add_reader`, `ReaderSpec`, `env.readers()`, `for_book`, `len` | `reader.spec`; op 21 |
| `env.formats()`, `supports`, `supports_file`, `for_path`, `for_file` | ops 19, 20 |
| `env.formats().open`, `open_with`, `OpenOptions` | op 1 (`doc_open`, `doc_option`), ops 2, 18 |
| `Document::metadata` | op 3, op 4 |
| `sections`, `section_len`, `whole_pages`, `progress` | `doc_meta.sections`, op 5, `doc_state.flags`, op 17 |
| `block_at` | op 6, section 4 |
| `toc_len`, `toc_entry`, `toc_target` | ops 7, 8 |
| `page_label`, `xpointer`, `locate_xpointer` | ops 9, 10, 11 |
| `links`, `link_target` | ops 12, 13 |
| `fonts`, `font_data`, `env.fonts().failed_reads` | op 14, `font.acquire` key `document.<id>.<index>` |
| `cover`, `prepare_image`, `prepare_image_larger`, `ui::draw_image` | ops 15, 16; slots 36, 37; `image_prepare` |
| `Document::extended` | none: new ops in a new revision |
| `Registrar::add_text_filter`, `add_decorator`, `add_page_layer`, `add_page_frame`, `add_voice` | reader-defined stages (`reader.def`, #964), not DOC |
| `Registrar::add_lookup_provider`, `lookup` | `lookup.spec`, stage and service `core.lookup` |
| `add_book_cache`, `add_book_data`, `add_book_field`, `add_book_badge` | LIB |

## 10. Hand-offs (after approval)

- **Kernel:** records, `core.document@1` and `core.lookup@1` schemas, `core.format@1` and
  `core.lookup@1` stage schemas (Core's stage-schema generator first), slots 36-37 in `abi.def`,
  roots 2-4 in the storage contract, this document final.
- **Core:** the document table and leases, the format broker and its checks, preparing in the main
  loop with the `.complete` stamp, roots 2-4, image ids and the band drawer, `image_prepare`
  decoding, document fonts in `font.acquire`, the lookup broker; tests for every refusal above,
  power-cut tests of the cache stamp.
- **Services:** the SDK side: a `Document` facade for openers over op 1-21, the block writer (piece
  splitting by size), a `Format` trait for format plugins over `core.format`, image and font
  helpers.
- **Firmware:** the X4 nested-callback stack case for `block`, the image band drawer's speed, the
  DOC line of the heap budget.
- **Plugin:** the ports: the six format plugins, notes-vault, the reader, files, file-manager,
  browser, koreader, kosync, sleep, dictionary, wikipedia, translate.

# KOReader on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (the exchange of bookmarks and
highlights with KOReader's sidecar files, its events and its question to the
reader) is still in place beside the native one. It is removed in the PR that
finishes the port.

## Ported in slice 1 (`src/native_model.rs`, `src/native_plugin.rs`)

- Nothing is declared yet: the plugin has no settings, and its work starts from
  events (`BOOK_OPENED`, `ANNOTATIONS_CHANGED`, `BOOK_MOVED`) and a question to
  the reader, none of which the host delivers to native plugins yet. The native
  entry (`NATIVE_ENTRY`, `native/metadata*`, generated with `tools/abi-metadata.py`)
  exists so that the image can be built and measured.
- The files model (`native_model.rs`, host-free): the interface-6 `lua` module and
  the helpers of `lib.rs` made fallible. It reads KOReader's Lua files and writes
  them back in the same form, names the files beside a book (`Book.sdr`,
  `metadata.<suffix>.lua`, `<name>.annotations.lua`, the known file named by the
  book's key), fingerprints notes, and reads and writes the file of what both sides
  had at the last exchange. A refused allocation is `NoMemory`, never an abort;
  nothing indexes text or bytes in a way that can panic; a table nests at most
  `MAX_DEPTH` (64) deep. Tests (`src/native_parity_tests.rs`) read the same files,
  random pieces of Lua files and random bytes with both readers and compare what
  they write back, and compare the names, the fingerprints and the known file field
  by field. `tests/native_alloc_refusal.rs` refuses each allocation of reading,
  writing, building and naming.
- The caller bounds the file (`max_file`: 4 MiB, 32 KiB on small devices).

## What the slice loses (each returns in a later slice or with its host piece)

- **The whole exchange**: reading KOReader's file when a book opens, taking over
  what is new, leaving what is new here for KOReader, the removal rules, the keeping
  aside of removed annotations. It needs the book-opened and annotations-changed
  events, the document and annotations facades (positions as XPointers, the text
  between two places, the chapter), the files facade (#384) and the clock.
- The question to the reader whether to remove annotations KOReader's file no longer
  has (the ask service and H1/H3/H4).
- Moving the known file with its book: a `book_data` declaration (the declarations
  builder has none yet) and the book-moved event.
- "How it works" (`how/`), the texts (`i18n/`) stay as they are.

Each loss is an ignored case in `tests/acceptance_pending.rs`. The interface-6
tests (`tests/exchange.rs`, `tests/lua.rs`, `src/tests.rs`) stay as the references.

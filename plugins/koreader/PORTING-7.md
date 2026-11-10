# KOReader on interface 7

Status: **native only.** The interface-6 plugin and its `lua` module are removed;
the texts (`i18n/`) and "How it works" (`how/`) are unchanged.

## What it does

- Declares the per-book folder `koreader` (`book_data`, user data: what both sides
  had at the last exchange; moved and removed with the book by the host) and
  subscribes to the book-opened and library-changed events
  (`declarations.rs`, generated into `native/metadata*` by `tools/abi-metadata.py`).
- A book opened, or a change of its annotations (library_changed with the
  annotations bit; the plugin's own change is ignored once), starts an exchange:
  KOReader's file beside the book is read (`native_model.rs`, a fallible Lua reader
  and writer that nests at most `MAX_DEPTH`), the book's annotations come from
  `Library::annotations`/`anchors`, positions go through `Documents::xpointer` and
  `locate`, and the rules of `native_exchange.rs` (host free, unit tested) say what
  is taken over, offered, kept aside or removed. Changes to the library's
  annotations are one edit transaction (`Library::edit_*`); what is left for KOReader
  is written to `<name>.annotations.lua` with `core.files`.
- Where KOReader's file, seen with annotations before, has none any more, the reader
  is asked in the book (`Ui::ask`, once per book and run).
- A book with more than 2,048 annotations (`MOST_ANNOTATIONS`) is not exchanged: a
  cut list would make what KOReader has beyond it look removed here.
- The plugin's own change comes back as a `library_changed` event and is ignored
  once (`own`). If the host raised none for a commit, the next real change of that
  book is ignored instead; nothing is lost, the exchange is idempotent and runs again
  on the next book open or change.
- Every allocation on these paths is fallible; a refused one stops the exchange
  with nothing changed on either side (`tests/native_alloc_refusal.rs`). Nothing
  indexes text or bytes in a way that can panic.

## Differences from interface 6

- The file bound follows the device class (`core.device`, read once): 32 KiB on a
  small device (as before), 4 MiB on medium and large; where the class cannot be read
  the small bound applies. A larger file is "unreadable": the exchange stops with
  nothing changed.
- The interface-6 reference tests (`tests/exchange.rs`, `tests/lua.rs`,
  `src/tests.rs`, the parity tests) are gone with the code. The Lua model keeps its
  expected texts as goldens (`src/native_model_tests.rs`, checked against the
  interface-6 output before it was removed) and the exchange rules their unit tests
  (`src/native_exchange.rs`).

## Not accepted yet (host not wired; ignored cases in `tests/acceptance_pending.rs`)

- No scripted-host test drives the glue (the cap per class is unit tested) (`native_plugin.rs`) end to end: the book
  opened, annotations changed, answered and moved flows are accepted only through
  the host once native plugins are activated (Core #944, Host #417) and the host
  delivers the book-opened and library-changed events and the ask service (H1/H3/H4).
- X4 file size and peak memory are not measured.
- The edit transaction and `anchors`/`book_data` are in Services' PRs #712/#706
  (draft-31 carrier); this PR builds on them.

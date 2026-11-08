# Catalogs (OPDS) on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (catalog list and editor, browsing, downloads,
covers, sync, OPDS 2 and WebDAV readers) is still in place beside the native one. It is removed in
the PR that finishes the port.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`)

- Declarations: the two settings (covers in lists, how many books a sync fetches at most) in the
  library group with the catalogs' section and the step. `build.rs` and `native/metadata*` are
  generated with `tools/abi-metadata.py`. En/de texts are the existing ones.
- The OPDS 1 reader (`native_model.rs`, host-free): the interface-6 `feed` reader made fallible. It
  reads a feed in pieces from a `Source` (the file the network host saved), keeps at most 100
  entries with short texts, resolves links against the feed, finds the next page and the search
  address, and names a file's extension by type or by its address. A refused allocation ends the
  reading with what was read and the feed marked `memory`, never an abort. Tests
  (`src/native_parity_tests.rs`) read the same feeds, random pieces of feeds and random bytes with
  both readers and compare the results (same fields, same limits), join random links with
  `web::join_url`, clean random text, and compare each native setting with interface 6's field by
  field (name, title, group, help, kind, default, flags, section, step).
  `tests/native_alloc_refusal.rs` refuses each allocation of the reader and the joining.
- The XML tokenizer decodes an entity or an attribute into owned text where one occurs; that
  allocation has no refusal path. It is bounded by the tokenizer's own limits on a token (the sweep
  uses a feed without entities). Kernel's fallible JSON (#414) is for the OPDS 2 reader, not here.

## What the slice loses (each returns in a later slice or with its host piece)

- **Everything that talks**: fetching a catalog, browsing, downloading books, covers, the sync with
  its timers: they need the network host (`core.net`), library import (LIB), the files facade
  (#384), secrets (catalog passwords), the work SDK and image slots (#385).
- The OPDS 2 (JSON) reader, the WebDAV listing reader and the search-template functions: later
  slices, to be ported the same way (fallible, compared with interface 6).
- The catalog list and editor, the browse screen, the sync action, the menu entry, the icon, "how
  it works".

Each loss is an ignored case in `tests/acceptance_pending.rs`.

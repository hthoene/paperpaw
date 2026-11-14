# Interface-7 port

Version 1.0.2 (Beta 3: 1.0.1, so no further bump). Slice 2 of the format ports (#228, assignment #1344), with
format-images.

Status: **complete** (native only, #228, ticket 228): the interface-6 form is removed (see "Removal step", done).
`NativeXtc` is in `paperpaw_app::NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt`. Until the shells activate
native plugins, the interface-6 app opens no XTC or XTCH book (held until activation).

## Ported

`src/provider.rs`: `XtcProvider::call(io, input, out)` answers one `core.format@1` call with the generated
`core_format_v1` codecs: op 1 sniff (`XTC\0`/`XTCH`), 2 open (whole pages, table of contents flags), 4 close,
5 meta, 6 meta_text, 7 sections and 9 toc (paged, 63 rows and a closing `fmt.end` with `more`/`next`),
8 block (a page is one image block with its `fmt.image`, or its number when the page cannot be decoded),
10 toc_target, 17 cover, 18 image. Pages are decoded into root 6 (`p<n>.gray`) when first shown; the host
owns the cache stamp, so the interface-6 `source` stamp is not written (`XtcDocument::open_native`).
Ops 11-13 and 15 answer `NotFound`, 14 and 16 an empty list, anything else (3 prepare, 19 progress)
`Unsupported`. A refused allocation is `NoMemory` end to end and an open that hits it leaves no document.
`tests/provider.rs` compares the pages' pixels, sizes, chapters and metadata with the reader (`XtcDocument::open_native`
on a cache of its own); `src/lib.rs` (unit tests) and `tests/hostile.rs` pin what a page, the metadata and the chapters
are and that hostile and damaged books fail cleanly, on the provider's roots (book root 3, cache root 6).

## Native entry (slice 3)

`declarations.rs` (`formats`: `format-xtc.xtc`, extensions `xtc` and `xtch`, flags `whole_pages` and `sniffs`, not `prepared`), `build.rs` (batches built into the image),
`src/native_plugin.rs` (`NativeXtc`: `make` submits the declaration; `event` answers a `core.format`
stage call of its token with `XtcProvider::call` over `HostIo`, encoded in place through
`Reply::write_with`), `src/native_entry.rs` (`NATIVE_ENTRY`, features `linked` and `native-entry`),
`native/metadata*` (heap, registrations, storage size/read/write/append/remove; no service, no feature;
regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`). Tests:
`tests/native_declarations.rs`, `native_linked_entry.rs`, `hosted_construction.rs` (the host checks the
declaration and refuses it as `Unsupported` until Core #944 admits formats; the positive case is ignored with
that name). `tools/plugin-files7` builds it as a file.

## Not wired yet (`tests/provider_pending.rs`, ignored)

- Title fallback from the file name belongs to the library (root 3 has no name).
- X4: decoding a page writes up to a page's gray picture per `block` call; time and stack are not measured.

## Refusals

`tests/provider_allocation.rs` refuses every allocation of open, meta, sections, toc, block (decoding a page), cover and image in turn: each refusal is `NoMemory`, and the same provider reads on afterwards. A failure of memory or the card while a page is decoded is passed on by `block_at` and `cover` (not turned into a named page); only a damaged page is named. A chapter that cannot be read makes the toc call fail (an error, never a gap that would shift the list).

## Removal step (done: native only)

Removed with the interface-6 form:
1. `XtcFormat` and its `Plugin` impl (`FORMAT`, `PLUGIN`, the manifest, `add_book_cache`, `add_format` with its
   `sniff`). The `HOW` and `CATALOGS` statics are gone too: nothing in the native registry reads them (the texts in
   `how/` and `i18n/` stay for the native how-to and the built catalogs);
2. `XtcDocument::open` (and `open_in`, `fingerprint`) with the plugin-folder cache and the interface-6 `source` stamp
   (`XtcDocument::open_native` is the one opener; the pages are `p<n>.gray` in the host's cache, root 6). The title
   fallback to the file name went with it (see below);
3. the app's `install_with` line; the dependency is optional and belongs to `native-linked` now (`dep:` and
   `paperpaw-plugin-format-xtc/linked`). XTC was never offered in the store or as an interface-6 plugin file;
4. `NativeXtc` is listed in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt`.

Tests: the unit tests of `src/lib.rs`, `tests/hostile.rs`, `tests/provider.rs` and the fuzz target (`fuzz/fuzz_targets/xtc.rs`)
open through `open_native` on the provider's roots instead of `XtcDocument::open`. Two tests of the stamp (a changed book
of the same size, or any change, is decoded anew) are dropped: the stamp is the host's. No test of the app or the shells
opens an XTC book, so none is ignored.

## Still blocked on the host

- **Activation:** the shells do not register native plugins yet; until then no XTC or XTCH book opens in the app.
  `tests/hosted_construction.rs` and the two cases of `tests/provider_pending.rs` stay ignored (Core #944; Host #417).
- **Title from the file name** belongs to the library (root 3 has no name): a book without a title in its metadata
  has none here (interface 6 named it by its file).
- X4: decoding a page writes up to a page's gray picture per `block` call; time and stack are not measured.

- **Beta 3 caches stay on cards.** The interface-6 plugin-folder caches (`.paperpaw/plugins/format-xtc/<hash>/`) of
  books opened with Beta 3 are no longer read or written after this removal; they stay on the card until a cleanup
  (follow-up, not part of this PR: the host or the library removes them, or the owner deletes `.paperpaw/plugins`).

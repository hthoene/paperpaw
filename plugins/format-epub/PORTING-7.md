# Interface-7 port

Status: **complete, native only** (ticket #244). The interface-6 form is removed (see "Removal step"); the plugin is
listed in `paperpaw_app::NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`. Until the shells activate native
plugins (Core #944, Host #417) an EPUB book (`.epub`) does not open at all: the PR is held until activation.

Version 1.0.5 (as the crate before the split; no further bump).

## Ported

`declarations.rs` (`format-epub.epub`: extensions `epub`; flag `sniffs` (the `mimetype` entry), a book is the file itself; not `prepared`),
`src/native_plugin.rs` (`NativeEpub`: `make` submits the declaration; `event` answers a `core.format` call of its
token with the family's `epub_provider()` through `serve`), `native/metadata*`, `build.rs`,
`tools/plugin-files7/src/bin/format-epub.rs`. The parser, the provider and its parity tests are those of
`crates/paperpaw-epub-family` (see its `PORTING-7.md`: refusal sweeps and the gaps the five plugins share).

## Removal step (done)

Gone with interface 6: `EpubFormat` and its `Plugin` impl (`add_book_cache`, `add_format`, the
`HOW` table, the manifest, the two catalogs it registered), its ids, and the app's registration
(`paperpaw-plugin-format-epub` is an optional dependency of the app, enabled by `native-linked`). The `EpubDocument` stays: the provider opens through it. The family crate no longer includes any plugin file by path (the catalogs and `how/` texts are the plugins' own). Reader tests, desktop snapshots and the library walk test that read an EPUB are `#[ignore]`d until native activation (35 tests and 17 snapshot cases; assertions kept). Not removed: the
parsers' own plugin-folder cache with its stamps (shared with the other formats of the family; the provider routes
it to the document cache, root 6, where the stamps are redundant but harmless); it goes with the family's last
removal.

## Gap: untitled books

This is not a `siblings` format, so the opener gets an empty file name (root 3 is the book itself). A book with no
title of its own now has an empty title where Beta 3 used the file stem. The reader names such a book by its path
(#1045); the library does the same.

## Gaps (the family's)

A book is converted when it is opened (no `prepare` steps). X4 `.ppn` size and heap are not measured here.

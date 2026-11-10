# Interface-7 port

Status: **complete, native only** (ticket #244). The interface-6 form is removed (see "Removal step"); the plugin is
listed in `paperpaw_app::NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`. Until the shells activate native
plugins (Core #944, Host #417) a comic book archive (`.cbz`) does not open at all: the PR is held until activation.

Version 1.0.5 (as the crate before the split; no further bump).

## Ported

`declarations.rs` (`format-cbz.cbz`: extensions `cbz`; flag `whole_pages` (each section is a picture); a book is the file itself; not `prepared`),
`src/native_plugin.rs` (`NativeCbz`: `make` submits the declaration; `event` answers a `core.format` call of its
token with the family's `cbz_provider()` through `serve`), `native/metadata*`, `build.rs`,
`tools/plugin-files7/src/bin/format-cbz.rs`. The parser, the provider and its parity tests are those of
`crates/paperpaw-epub-family` (see its `PORTING-7.md`: refusal sweeps and the gaps the five plugins share).

## Removal step (done)

Gone with interface 6: `CbzFormat` and its `Plugin` impl (`add_book_cache` with the `cbz-` prefix, `add_format`, the
`HOW` table, the manifest, the two catalogs it registered), its ids, and the app's registration
(`paperpaw-plugin-format-cbz` is an optional dependency of the app, enabled by `native-linked`). The `CbzDocument` stays: the provider opens through it. Not removed: the
parsers' own plugin-folder cache with its stamps (shared with the other formats of the family; the provider routes
it to the document cache, root 6, where the stamps are redundant but harmless); it goes with the family's last
removal.

## Gap: untitled books

This is not a `siblings` format, so the opener gets an empty file name (root 3 is the book itself). A book with no
title of its own now has an empty title where Beta 3 used the file stem. The reader names such a book by its path
(#1045); the library does the same.

## Gaps (the family's)

A book is converted when it is opened (no `prepare` steps). X4 `.ppn` size and heap are not measured here.

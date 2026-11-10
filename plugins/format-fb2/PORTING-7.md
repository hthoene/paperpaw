# Interface-7 port

Status: **complete, native only** (ticket #244). The interface-6 form is removed (see "Removal step"); the plugin is
listed in `paperpaw_app::NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`. Until the shells activate native
plugins (Core #944, Host #417) a FictionBook (`.fb2`, `.fb2.zip`) does not open at all: the PR is held until activation.

Version 1.0.5 (as the crate before the split; no further bump).

## Ported

`declarations.rs` (`format-fb2.fb2`: extensions `fb2` and `fb2.zip`; flag `siblings` (the pictures beside the file); not `prepared`),
`src/native_plugin.rs` (`NativeFb2`: `make` submits the declaration; `event` answers a `core.format` call of its
token with the family's `fb2_provider()` through `serve`), `native/metadata*`, `build.rs`,
`tools/plugin-files7/src/bin/format-fb2.rs`. The parser, the provider and its parity tests are those of
`crates/paperpaw-epub-family` (see its `PORTING-7.md`: refusal sweeps and the gaps the five plugins share).

## Removal step (done)

Gone with interface 6: `Fb2Format` and its `Plugin` impl (`add_book_cache` with the `fb2-` prefix, `add_format`, the
`HOW` table, the manifest, the two catalogs it registered), its ids, and the app's registration
(`paperpaw-plugin-format-fb2` is an optional dependency of the app, enabled by `native-linked`). The `Fb2Document` stays: the provider opens through it. Not removed: the
parsers' own plugin-folder cache with its stamps (shared with the other formats of the family; the provider routes
it to the document cache, root 6, where the stamps are redundant but harmless); it goes with the family's last
removal.

## Gaps (the family's)

Listing the book's folder on root 3 needs `native::files` (#384): until then a picture beside the file is read by
name only. A book is converted when it is opened (no `prepare` steps). X4 `.ppn` size and heap are not measured here.

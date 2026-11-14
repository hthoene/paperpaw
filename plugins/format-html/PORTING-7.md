# Interface-7 port

Status: **complete, native only** (ticket #244). The interface-6 form is removed (see "Removal step"); the plugin is
listed in `paperpaw_app::NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`. Until the shells activate native
plugins (Core #944, Host #417) an `.html`, `.htm` or `.xhtml` file does not open at all: the PR is held until
activation.

Version 1.0.5 (as the crate before the split; no further bump).

## Ported

`declarations.rs` (`format-html.html`: extensions `html`, `htm`, `xhtml`, flag `siblings`: the pictures beside the
page are read through root 3; not `prepared`), `src/native_plugin.rs` (`NativeHtml`: `make` submits the declaration;
`event` answers a `core.format` call of its token with the family's `html_provider()` through `serve`),
`native/metadata*`, `build.rs`, `tools/plugin-files7/src/bin/format-html.rs`. The parser (`HtmlDocument`), the
provider and its parity tests are those of `crates/paperpaw-epub-family` (see its `PORTING-7.md`, which has the
refusal sweeps and the gaps shared by the five plugins).

## Removal step (done)

Gone with interface 6: `HtmlFormat` and its `Plugin` impl (`add_book_cache` with the `html-` prefix, `add_format`,
the `HOW` table, the manifest, the two catalogs it registered), the `HTML` / `HTML_PLUGIN` ids, and the app's
registration (`paperpaw-plugin-format-html` is an optional dependency of the app, enabled by `native-linked`).
`HtmlDocument` stays: the provider opens through it (so do the notes vault's Markdown pictures). Not removed: the
parser's own plugin-folder cache with its stamps (shared with the other four formats; the provider routes it to the
document cache, root 6, where the stamps are redundant but harmless). It goes when the family's documents are made
to ask the host for the stamp.

## Gaps (the family's)

Listing the book's folder on root 3 needs `native::files` (#384): until then a picture beside the page is read by
name only. A page is converted when it is opened (no `prepare` steps). X4 `.ppn` size and heap are not measured
here.

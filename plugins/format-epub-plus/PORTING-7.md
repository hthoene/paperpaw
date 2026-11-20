# Interface-7 port

Status: **complete, native only** (ticket #409). A store plugin (not linked into the firmware): it is `format-epub`
plus the code of `fixed.rs` and `convert/math.rs` of `paperpaw-epub-family`, which only its provider
(`epub_plus_provider`) reaches, so the built-in `format-epub` links none of it. Measured on the X4 (riscv32, one
build, `tools/plugin-files7/build.sh x4`): `format-epub` 330,540 B before, 331,112 B after (+572 B: the hooks in the
converter and the formula's trait object); this plugin's file 363,384 B.

Version 1.0.0.

## What it adds to format-epub

- `format-epub-plus.epub` claims `epub` with priority 10 (the built-in has 0), so once installed it reads the books
  (the user's choice `core.formats.epub` still wins). It reads every EPUB: the interface has no way for a format to
  hand a file back to the next one by what is inside it (`sniff` is only asked for files no extension claims).
- Fixed layout (`fixed.rs` of the family): a book whose package says `rendition:layout` `pre-paginated` (for the book, or
  `rendition:layout-pre-paginated` on spine items; also the old `fixed-layout` meta), and whose pages are pictures
  (an `<img>` or an SVG `<image>`, less than 120 characters of text) opens as a document of whole pages as a comic
  book archive does (`CbzDocument`, the pages in spine order, `page-progression-direction` the way), one section per
  page, the picture shown whole. A book with reading matter on its pages (a quarter of the pages or more) opens
  as any EPUB: reflowed, a section a page.
- MathML (`convert/math.rs`): `<math>` is read into text (fractions `a/b`, `x²`, `x^(n+1)`, `a_i`, `√(…)`, matrices
  as lines), inline, or with `display="block"` as its own centered block. `alttext` where nothing else comes out.

## Gaps

No panel mode and no per-book direction setting for fixed-layout pages (the plugin has no settings); pages that are
SVG or WebP pictures, a page with several pictures (the first is shown), text of a picture page (captions) and
positioned text layers are not shown; `epub:switch` fallbacks are not skipped, so a formula may show with its
fallback picture. Math: no `mmultiscripts`; the characters are the book's (`√`, `∑` need a font that has them).

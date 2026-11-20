# Interface-7 port

Status: **complete, native only** (tickets #409, #429). A store plugin (not linked into the firmware) with two parts:

- **The conversion hook** (`core.convert`, `docs/abi/document.md` 4.1): the plugin subscribes (`registration.stage`,
  token 2) and converts the `<math>` elements the EPUB formats hand on (`format-epub`, the built-in one, writes each
  as a foreign element with the text it says without help; the host puts this plugin's text there). It reads the
  element's markup only (`paperpaw_epub_family::math_text`, `convert/math.rs`), no book.
- **Fixed-layout picture books** (`fixed.rs` of `paperpaw-epub-family`): one format `format-epub-plus.epub`,
  priority 10, with `FormatDecl::asks` (#242). Before it opens an EPUB the host asks it (`sniff` with `claimed`);
  it says yes only for a fixed-layout book of pictures, and every other EPUB goes on to the built-in format. It no
  longer reads every EPUB and carries no EPUB reader of its own for reflowed books.

Measured on the X4 (riscv32, `tools/plugin-files7/build.sh x4`, one build each): this plugin's file 364,976 B before,
184,684 B after (-180,292 B); `format-epub` (built into the firmware) 332,880 B before, 337,308 B after (+4,428 B: the
foreign-element capture, `<n>.fgn` and the block's annotations).

Version 1.1.0.

## What it adds to format-epub

- Picture books (below) and MathML (below), nothing else: an EPUB that is neither opens in `format-epub` as without this
  plugin.
- Fixed layout (`fixed.rs` of the family): a book whose package says `rendition:layout` `pre-paginated` (for the book, or
  `rendition:layout-pre-paginated` on spine items; also the old `fixed-layout` meta), and whose pages are pictures
  (an `<img>` or an SVG `<image>`, less than 120 characters of text) opens as a document of whole pages as a comic
  book archive does (`CbzDocument`, the pages in spine order, `page-progression-direction` the way), one section per
  page, the picture shown whole. A book with reading matter on its pages (a quarter of the pages or more) is not
  claimed: it opens as any EPUB, reflowed, a section a page.
- MathML (`convert/math.rs`): `<math>` is read into text (fractions `a/b`, `x²`, `x^(n+1)`, `a_i`, `√(…)`, matrices
  as lines), inline, or with `display="block"` as its own centered block (the format makes the block, the hook gives
  its text). `alttext` where nothing else comes out. Without this plugin the format's text stays: the `alttext`, else
  the text of the formula's symbols (`x = 1`).

## Gaps

No panel mode and no per-book direction setting for fixed-layout pages (the plugin has no settings); pages that are
SVG or WebP pictures, a page with several pictures (the first is shown), text of a picture page (captions) and
positioned text layers are not shown; `epub:switch` fallbacks are not skipped, so a formula may show with its
fallback picture. Math: no `mmultiscripts`; the characters are the book's (`√`, `∑` need a font that has them).
A formula whose markup is over 1,000 bytes (or nested deeper than 32 elements) is not handed on and keeps the format's
text; the host puts the hook's text in a block that stays within one fragment (3,840 bytes), else the format's text stays.
The first page with a formula after the plugin was started by the host may show the format's text (the plugin is asked
to start and the page is already laid out); turning to the page again shows the hook's.

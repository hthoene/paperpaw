# Interface-7 port (plan, not started in code)

Assignment #1638 (#228). Branch based on #385 (draft 28, `5067fc377`); move to integration once #385 merges.

## Facts found

- **No `input_rows` change in #385.** `abi/stages/core.format@1.def` is not in #385's file list and
  `input_decode` still takes exactly one record. This plugin does not need `fmt.option`: nothing in
  the crate reads `OpenOptions::settings`; it uses only `publisher_styles` (call flags bit 0), `color`
  (bit 1), the user CSS (root 4) and the image/cover sides. So the port is not blocked on options.
  (docx/mobi: to check.)
- **One crate, five plugins, ~11.7k lines:** format-epub, format-html, format-fb2, format-markdown,
  format-cbz, sharing one converter (`convert`, `css`, `package`, `cached`, `fb2`, ...). Each is a
  `FormatDecl` of its own (`Declarations::format`, Services #436).
- **The interface-6 cache is the plugin's own** (`book_cache` from `Env::plugin_dir` + the book's path,
  a book fingerprint stamp, `css-<hash>` and `plain` variants, `remove_dir_all`/`list` cleanups). In
  interface 7 the host owns the cache per conversion (root 6, the conversion key already covers styles,
  colour, sides, CSS), so the stamps, variants and cleanups are redundant.

## Plan (slices, each a ready PR, each with parity tests against the interface-6 reader on `tests/data`)

1. **Shared provider** (`provider.rs`): `FormatProvider<D: Document>` answering the `core.format@1` ops
   (open/prepare/close, meta, meta_text, sections, block with the full record list incl. images, table
   rows, rubies, inline languages, fragments; toc, toc_target, page_label, xpointer, locate, links,
   link_target, fonts, cover, image, progress) over one storage that routes the book to root 3 and the
   cache to root 6 (the txt/xtc/images pattern). Allocation-refusal sweeps, hostile-file tests.
2. **format-markdown, format-html** (single file, no archive): declarations + provider.
3. **format-fb2, format-cbz** (archives, images).
4. **format-epub**: package, spine, toc, CSS, fonts, links; replace the plugin-managed cache by root 6.
5. Native-only: remove `FormatSpec`/`add_book_cache` registration at activation (Core #944) together with
   the app wiring; no interface-6 code left then.

## Open decisions / gaps (for Kernel / Services)

- Block field encodings not stated in `document.md` section 4: `dir` values (Auto/Ltr/Rtl), `writing_mode`,
  `lang` tag source, `align`. I will use the interface-6 `bits()` values and propose them in the doc.
- Fonts: `doc_font` rows give family/flags/bytes; how the host reads a face's data is not in section 3.
- X4: `.ppn` size vs Beta 3 not measurable until an entry point exists; heap and stack of `block` on a long
  chapter are open device gates.

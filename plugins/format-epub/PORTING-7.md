# Interface-7 port (slices 1–2 done: shared provider, Markdown, HTML)

Done: `provider.rs`, `markdown_provider()`, `html_provider()`, parity tests `tests/provider_markdown.rs`, `tests/provider_html.rs` (block by block against the interface-6 reader). Block `lang` (BCP 47), `dir` and `writing_mode` are sent with the draft-29 numbers. Op 18 serves the picture of the block that starts at `index`. A language or series index over the call's limit is cut, never refused. Tests: parity incl. block headers, links and contents (`provider_html.rs`), a made-up document and hostile calls (`provider_fake.rs`). Not done: an allocation-refusal sweep — the shared parser and `Block` use infallible `Vec`/`String`, so a refused allocation aborts the test process; it follows when the parser is fallible. Each fragment call parses its block twice (the second finds the last block); not changed. Not measured: X4 size and heap. The crate version stays 1.0.5 (one bump per release). Slice 3 (fb2, cbz): `fb2_provider()`, `cbz_provider()`, `tests/provider_fb2.rs`, `tests/provider_cbz.rs`; allocation-sweep cases ignored until Core todo 208. Slice 4 (epub): `epub_provider()` (sniff by `mimetype`), `tests/provider_epub.rs` (two chapters, header fields, links, contents, sniff); its allocation case is ignored. Follow-up (todo 263, draft 29): embedded faces are written to the root-6 file `fonts/<i>` (through a temporary name) and listed with their size (op 16, up to 124 faces); ruby kinds use `AnnotationKind` (0 ruby, 1 phonetic, 2 gloss, 3+n); the block's inline languages are sent as `fmt.inline_lang` rows (runs name them by `lang_index`); sniff tests for a wrong-case, deflated, not-first and too-short head. A plain or obfuscated font is copied in chunks (inflate, reveal in a 512 byte part, append to the temporary name, rename at the end; the temporary file is removed on any error) through the document's `extended` key `format-epub.font-copy`: no more than a chunk is held. The copied faces of one book may take 32 MiB together (`MAX_FONT_BYTES`); a face over a single file's 16 MiB, one unreadable, or one beyond the cap is listed with 0 bytes (its fault, never an error of the whole list) and is not inflated again while the document is open. Ruby kinds have no effect until a parser sets annotations (none does yet). Gaps: a WOFF face is converted whole in memory (up to 16 MiB; not workable on the X4, the face is then listed with 0 bytes when memory is short); an unreadable face is listed with 0 bytes; the plugin-managed cache is redundant and goes with the interface-6 path at activation.

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

## Reading on a full heap (Core, todo 208)

Status: Opening and reading Markdown, HTML, EPUB, FictionBook and comic books answers a refused
allocation, it never aborts. The refusal sweeps (`tests/allocation.rs`) cover all five (an EPUB of
two chapters, opened and read whole), and a book that converts a chapter again and finds no room part
of the way reads on as a clean run does. A
picture that finds no room to be decoded shows its alternative text for now, but its chapter is
**not marked complete** in the cache (`<n>.end` is not written), so the next opening converts it
again; one that cannot be decoded for other reasons is cached as its alternative text, as before.
Any other refused allocation fails the chapter with `OutOfMemory`. Until Kernel's decoder tells
"too big for the heap" from a refused allocation, a picture too big for the heap costs a conversion
on each opening.

- Every growth on the path of opening and reading a book (strings, lists, formatted text, the cache
  record reader) goes through `src/fallible.rs` or `try_reserve` itself. A refusal is
  `DocumentError::OutOfMemory`, which a provider maps to the format refusal. Successful parses
  give the same output as before.
- `tests/allocation.rs` refuses the n-th allocation for every n over a representative Markdown
  and HTML document, and checks: the answer is `OutOfMemory` or the same document, never a panic,
  and a retry on the same storage reads as a clean run does (nothing half-made stays).
  `list_infallible_sites` (ignored, developer aid) lists allocations that do not go through
  `try_*`.
- **Documented residual:** the one `Box<dyn Document>` per opened document, made by the format
  factory in `src/lib.rs` (`Box::new(d) as Box<dyn Document>`). The host's factory type is
  `Result<Box<dyn Document>, DocumentError>`, the SDK has no safe fallible box and a plugin crate
  holds no `unsafe`. A sweep skips that site.
- **Documented residual (SDK):** `Document::links` builds `paperpaw_document::Link`s with `Link::new`, which
  allocates its strings infallibly (Kernel's type); the sweeps do not call `links`. Where the trait
  returns an `Option` or a `Vec` (`toc_entry`, `xpointer`, `locate_xpointer`, `page_label`, `fonts`) a
  refused allocation answers none/empty; the sweeps call all of these.
- **Documented residual (SDK):** `paperpaw_sdk::ImageFile::open` / `Decoded::open` (crate
  `paperpaw-image`) copy a path with no way to refuse; the picture lookups of this crate call them.
  The sweep leaves allocations made inside `paperpaw_image` be (`in_sdk_image` in
  `tests/allocation.rs`). Follow-up: a fallible `ImageFile::try_open` (Kernel's API).
- Cache line reads (`cached::lines`) and picture lookups answer `OutOfMemory` where the caller can
  say so (conversion, block reads); lookups with no way to say it (labels, links, anchors, the
  Document trait's `Option` returns) find nothing for that call and are not cached.

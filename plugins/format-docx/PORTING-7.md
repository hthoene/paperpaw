# Interface-7 port

Version 1.0.3 (bumped since Beta 3 already; no further bump). The Word documents port (#228, assignment #1774):
slice 1 (PR #617) the fallible converter and the `core.format` provider; slice 2 (this one) the declaration, native
entry, metadata and `.ppn`, as format-txt, format-xtc and format-images did (PR #611, #612, #614).

Status: **complete** (native only, #228, ticket 228). The interface-6 form is removed (see "Removal step" below, done);
`NativeDocx` is in `paperpaw_app::NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt`. Until the shells activate
native plugins, the interface-6 app opens no `.docx` (held until activation).

## Ported

`src/provider.rs`: `DocxProvider::call(io, input, out)` answers one `core.format@1` call with the generated
`core_format_v1` codecs: op 1 no rows (not a sniffer: the extension names the format), 2 open, 3 prepare, 4 close,
5 meta, 6 meta_text, 7 sections (one, the text's length), 8 block (header, text parts, runs; cut into fragments
between records with `more`/`next`/generation; a picture's `fmt.image`), 9 toc (paged, 63 rows and a closing
`fmt.end`), 10 toc_target, 14 links (paged; a link to the web carries its address, a link inside the document
`#<number>`; a page holds as many addresses as fit a reply), 15 link_target, 18 image. Ops 11-13 and 17 (no cover)
answer `NotFound`, 16 an empty list, 19 and anything else `Unsupported`. The book (the archive) is root 3 (path `""`),
the cache root 6 (the names of the cache's files: `blocks.bin`, `index.bin`, `links.bin`, `toc.idx`, `toc.bin`,
`urls.idx`, `urls.bin`, `marks.bin`, `pictures.txt`, `info.txt`, `media-<n>`, `img-<n>.gray`, `done` last); the host
owns the stamp (`.complete`).

- **One converter and reader.** The converter (`DocxDocument::convert`, `writer.rs`, `parts.rs`), the cache
  (`cache.rs`) and the pictures (`images.rs`) serve the provider through `DocxDocument::native` (the archive and the
  cache are both `""`). `tests/provider.rs` compares blocks (text, kind, offsets, runs, alignment), contents, links,
  places, metadata and the pictures' pixels with `DocxDocument` converted and read on a cache of its own;
  `tests/docx.rs` pins what the text, headings, lists, tables, links and pictures of a Word document are (it
  converts and reads on the provider's roots) and the hostile and damaged documents, `tests/docx_memory.rs` that
  converting needs the same memory for a short and a long document.
- **Everything is fallible.** Every allocation of the converter (parts, styles, relationships, the writer's text,
  runs, titles, pending bookmarks, the cache's buffers), of the cache reader and of the pictures is reserved with
  `try_reserve` (`src/mem.rs`) or latched (`Output`, `Writer`, `Parts` set an `oom` flag the conversion ends with);
  a refusal is `OutOfMemory`, `NoMemory` at the provider, and never a damaged or unfinished document. Three silent
  failures of interface 6 are errors now: a part that could not be found for lack of memory (styles and
  relationships were left out and the document kept so), a card that fails while a small part is read, and a
  zip error named by allocating its message. `is_address` no longer slices a link inside a character.
- **Preparing is op 3, in one call.** An open of a document whose cache has no `done` answers `state` 1; the first
  `prepare` makes the whole cache (what interface 6 did at open) and answers `state` 0 with bit 6 (cache made now)
  and bit 1 (has contents), or `state` 2 with the status (`Invalid` for what is no Word document). A refused
  allocation answers `NoMemory`, drops what was made and the next call makes it again; a cache with `done` opens
  ready.
- **Pictures** are taken out of the archive and decoded when the host asks: a block's `fmt.image` has flags bit 0
  (not decoded yet) until `img-<n>.gray` exists; op 18 decodes picture `n` at the open's `image_side` (a larger
  side in the call is a zoom file `zoom-<n>-<side>.gray`). A picture that cannot be decoded is marked
  (`img-<n>.fail`), is `NotFound` and its block shows its alternative text, as interface 6 did; a failure of memory
  or the card is passed on, only a damaged picture is named.

## Check of `input_rows` (the `fmt.option` need of #385)

None: the plugin reads no setting (`OpenOptions::settings` is never read); `image_fit` and `color` are the open's
`image_side` and `flags` bit 1.

## Gaps

- **`prepare` is one long call.** The conversion reads the archive's `document.xml` as a stream
  (`ZipArchive::stream` into the XML tokenizer): there is no state to keep between calls, so it cannot give the
  host the 50 ms steps format-txt and format-mobi have, nor a progress. A resumable reader (the `EntryReader` of
  `paperpaw-zip` borrows its archive) would make the steps; time and stack of the call on the X4 are not measured.
- Declaration (`declarations.rs`: format `docx`, extension `docx`, `prepared`, no sniffing), `NativeDocx`, the `NATIVE_ENTRY`
  (`linked`/`native-entry` features) and metadata (`native/`): this slice. `tests/hosted_construction.rs` is ignored,
  BLOCKED #944 (the host's format broker does not admit formats yet; a second test pins the refusal).
- `ImageFile::open` of `paperpaw-image` makes its path infallibly; `images::decoded` reads the header the writers
  made itself (interim). Named step: switch to `ImageFile::try_open` after #610 and drop this header parser, so
  there is one parser, not two.
- Title fallback: root 3 has no file name, so a document without a title has none (the library names it by its
  file); interface 6 used the file name.
- Embedded fonts, direction, language of a run: a Word document in this reader has none (the records carry 0).
- Two documents of one conversion open at once would share a cache that one is still making; the host's pinning rule
  should keep them apart.

## Refusals

`tests/provider_allocation.rs` refuses every allocation of open, prepare (the whole conversion), meta, meta_text,
sections, contents, links, blocks and pictures in turn, over two documents (4,000 and 6,000 allocations a call
swept, and the sweep checks that is more than there are): each refusal is `NoMemory`, and the reading after it
equals the reading of a provider that had none. `tests/provider.rs` has a hostile-document loop (random damage and
cuts through open, prepare and reads: no panic), failed documents (not a document, short, text, empty: no cache is
left) and bad calls (unknown document, an id opened twice, too many documents, bad input).

## Removal step (done: native only)

Removed with the interface-6 form:

1. `DocxFormat` and its `Plugin` impl (`FORMAT`, `PLUGIN`, the manifest, `add_book_cache`, `add_format`), and
   `document::opener`. The `HOW` and `CATALOGS` statics are gone too: nothing in the native registry reads them
   (the texts in `how/` and `i18n/` stay for the native how-to and the built catalogs).
2. `DocxDocument::open` with its plugin-folder cache, `fingerprint` and the `VERSION` stamp (the native cache's
   stamp is the host's). The title fallback to the file name went with it (see Gaps).
3. The app's store-offer line (`install_with`), the entry in `STORE_PLUGINS` and the `store-plugins` dependency (the
   dependency is `native-linked`'s now), the interface-6 plugin file (`tools/plugin-files`: bin, dependency, the
   `plugins.txt` line is a comment like the other plugins that wait for interface 7).
4. `NativeDocx` is listed in `NATIVE_PLUGINS` (with `paperpaw-plugin-format-docx/linked` in `native-linked`) and in
   `tools/plugin-files7/complete.txt`.

Tests: `tests/docx.rs` and `tests/docx_memory.rs` convert and read through `DocxDocument::convert` and `native` on
the provider's roots instead of `DocxDocument::open`; two tests of pure interface-6 plumbing are dropped (the stamp
that did not convert again, and nothing left in the plugin's folder after a failed conversion: the provider's
failed-document test and `a_cache_that_is_whole_opens_ready_at_once` cover the cache now). The title test pins the
native behaviour (no title without properties). `tests/provider.rs` compares with the reader, not with interface 6.
In the emulator, the Word snapshot cases and the three Word memory tests need native activation and are ignored with
that reason (assertions kept).

## Still blocked on the host

- **Activation:** the shells do not register native plugins yet; until then no `.docx` opens in the app, and the
  ignored cases above stay ignored. `tests/hosted_construction.rs` stays ignored, BLOCKED #944 (the host's format
  broker admits no formats yet).
- **Title from the file name** belongs to the library (root 3 has no name).
- **Resumable preparing** (`prepare` is one long call) needs a resumable archive reader.

- **Beta 3 caches stay on cards.** The interface-6 plugin-folder caches (`.paperpaw/plugins/format-docx/<hash>/`) of
  books opened with Beta 3 are no longer read or written after this removal; they stay on the card until a cleanup
  (follow-up, not part of this PR: the host or the library removes them, or the owner deletes `.paperpaw/plugins`).

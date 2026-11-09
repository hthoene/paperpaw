# Interface-7 port

Version 1.0.3 (bumped since Beta 3 already; no further bump). Slice 1 of the Word documents port (#228,
assignment #1774): the fallible converter and the `core.format` provider. Slice 2 (declaration, native
entry, metadata, `.ppn`) follows the way format-txt, format-xtc and format-images did (PR #611, #612, #614).

Status: **not complete** (no native entry yet; the interface-6 `DocxFormat` is still in place).

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

- **The shared code is the interface-6 code.** The converter (`DocxDocument::convert`, `writer.rs`, `parts.rs`),
  the cache (`cache.rs`) and the pictures (`images.rs`) serve both openers (`DocxDocument::open` for interface 6,
  `DocxDocument::native` for the provider: the archive and the cache are both `""`). `tests/provider.rs` compares
  blocks (text, kind, offsets, runs, alignment), contents, links, places, metadata and the pictures' pixels with
  `DocxDocument` on the same documents.
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
- Declaration (`Declarations::format`: extension `docx`, `prepared`), the `NATIVE_ENTRY` and the `.ppn`: slice 2.
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

## Removal step (one sweep, with the reader port and Core #944)

1. `DocxFormat` and its `Plugin` impl (`FORMAT`, `add_book_cache`, `add_format`, the `HOW` table kept for the manifest),
   `document::opener`;
2. `DocxDocument::open` with its plugin-folder cache, `fingerprint` and the `VERSION` stamp (the native cache's
   stamp is the host's);
3. the app's store-offer line and the other plugins' tests that register `DocxFormat`;
4. list `NativeDocx` in `NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt` (slice 2).

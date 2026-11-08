# Interface-7 port

Version 1.0.2. Slice 2 of the format ports (#228, assignment #1344), with format-images.

## Ported

`src/provider.rs`: `XtcProvider::call(io, input, out)` answers one `core.format@1` call with the generated
`core_format_v1` codecs: op 1 sniff (`XTC\0`/`XTCH`), 2 open (whole pages, table of contents flags), 4 close,
5 meta, 6 meta_text, 7 sections and 9 toc (paged, 63 rows and a closing `fmt.end` with `more`/`next`),
8 block (a page is one image block with its `fmt.image`, or its number when the page cannot be decoded),
10 toc_target, 17 cover, 18 image. Pages are decoded into root 6 (`p<n>.gray`) when first shown; the host
owns the cache stamp, so the interface-6 `source` stamp is not written (`XtcDocument::open_native`).
Ops 11-13 and 15 answer `NotFound`, 14 and 16 an empty list, anything else (3 prepare, 19 progress)
`Unsupported`. A refused allocation is `NoMemory` end to end and an open that hits it leaves no document.
`tests/provider.rs` compares the pages' pixels, sizes, chapters and metadata with the interface-6 reader.

## Not wired yet (`tests/provider_pending.rs`, ignored)

- `format.spec` declaration builder in the SDK; Host #417 and Core #944 for the host side.
- Title fallback from the file name belongs to the library (root 3 has no name).
- X4: decoding a page writes up to a page's gray picture per `block` call; time and stack are not measured.

## Refusals

`tests/provider_allocation.rs` refuses every allocation of open, meta, sections, toc, block (decoding a page), cover and image in turn: each refusal is `NoMemory`, and the same provider reads on afterwards. A failure of memory or the card while a page is decoded is passed on by `block_at` and `cover` (not turned into a named page); only a damaged page is named. A chapter that cannot be read makes the toc call fail (an error, never a gap that would shift the list).

## Left in place (intermediate)

The interface-6 `XtcFormat` registration and `XtcDocument::open` stay until the host activates native plugins.
X4 `.ppn` size: not measurable yet (built in, no entry point).

# Interface-7 port

Version 1.0.4. Slice 2 of the format ports (#228, assignment #1344), with format-xtc.

## Ported

`src/provider.rs`: `ImagesProvider::call(io, input, out)` answers one `core.format@1` call with the generated
`core_format_v1` codecs: op 2 open (the opened picture is named by the call's `text`; flags bit 1 colour,
`image_side`/`cover_side`), 4 close, 5 meta, 6 meta_text, 7 sections (paged),
8 block (an image block with `fmt.image`, or the file name when the picture cannot be shown), 17 cover, 18 image.
One storage serves both roots: paths that start with `cache:` (a colon is no part of a name on the card) are
root 6, the others root 3; `fmt.image.path` is the root-6 path without the prefix. The title is the opened
file's name. Ops 10-13 and 15 answer `NotFound`, 9, 14 and 16 an empty list, anything else `Unsupported`.
A refused allocation is `NoMemory` end to end and an open that hits it leaves no document.
`tests/provider.rs` compares pages, order and decoded pixels with the interface-6 reader.

## Not wired yet (`tests/provider_pending.rs`, ignored)

- Listing the book's folder (root 3) has no SDK function: the host-side form opens with the one picture
  until `native::storage` can list root 3 (the pages after the opened picture are not found).
- `format.spec` declaration builder (flags `whole_pages`, `siblings`); Host #417 and Core #944.
- Sides (Kernel's ruling): the host sends non-zero `cover_side`/`image_side` in ops 2, 17 and 18 (the cover's 3/8 of the screen resolved by the host); the provider uses the side of the call as the longest side, and falls back to 300 / 1,600 (`COVER_FIT`/`IMAGE_FIT`) on 0 without failing.

## Refusals

`tests/provider_allocation.rs` refuses every allocation of open, meta, sections and block (of pages that are no pictures) in turn: each refusal is `NoMemory`, and the same provider reads on afterwards. **Gap:** decoding a picture (block, cover, image) reaches `paperpaw-image`'s `GrayWriter::start`, whose 12-byte header `Vec::with_capacity` is not fallible (Kernel: write the header from a stack array), so that path is not swept yet. A failure of memory or the card while a picture is decoded is now passed on by `block_at` (not turned into a named page); only a damaged picture is named. A name in the folder with a `:` is refused at open.

## Left in place (intermediate)

The interface-6 `ImagesFormat` registration and `ImagesDocument::open` stay until the host activates native
plugins. X4 `.ppn` size: not measurable yet (built in, no entry point).

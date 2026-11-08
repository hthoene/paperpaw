# Interface-7 port

Version 1.0.4. Slice 2 of the format ports (#228, assignment #1344), with format-xtc.

## Ported

`src/provider.rs`: `ImagesProvider::call(io, input, out)` answers one `core.format@1` call with the generated
`core_format_v1` codecs: op 2 open (the opened picture is named by the call's `text`; flags bit 1 colour,
`image_side`/`cover_side`, 0 meaning 1600 and 300), 4 close, 5 meta, 6 meta_text, 7 sections (paged),
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
- No allocation-refusal sweep for the provider yet.
- Cover size 0 uses 300 pixels, not the larger of 300 and 3/8 of the screen (the plugin does not know the screen).

## Left in place (intermediate)

The interface-6 `ImagesFormat` registration and `ImagesDocument::open` stay until the host activates native
plugins. X4 `.ppn` size: not measurable yet (built in, no entry point).

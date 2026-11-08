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

## Native entry (slice 3)

`declarations.rs` (`formats`: `format-images.pictures`, extensions `jpg`, `jpeg`, `png` and `gif`, flags `whole_pages` and `siblings`, not `prepared`, no sniffing), `build.rs` (batches built into the image),
`src/native_plugin.rs` (`NativeImages`: `make` submits the declaration; `event` answers a `core.format`
stage call of its token with `ImagesProvider::call` over `HostIo`, encoded in place through
`Reply::write_with`), `src/native_entry.rs` (`NATIVE_ENTRY`, features `linked` and `native-entry`),
`native/metadata*` (heap, registrations, storage size/read/write/append/remove; no service, no feature;
regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`). Tests:
`tests/native_declarations.rs`, `native_linked_entry.rs`, `hosted_construction.rs` (the host checks the
declaration and refuses it as `Unsupported` until Core #944 admits formats; the positive case is ignored with
that name). `tools/plugin-files7` builds it as a file.

## Not wired yet (`tests/provider_pending.rs`, ignored)

- Listing the book's folder (root 3) has no SDK function: the host-side form opens with the one picture
  until `native::storage` can list root 3 (the pages after the opened picture are not found).
- Sides (Kernel's ruling): the host sends non-zero `cover_side`/`image_side` in ops 2, 17 and 18 (the cover's 3/8 of the screen resolved by the host); the provider uses the side of the call as the longest side, and falls back to 300 / 1,600 (`COVER_FIT`/`IMAGE_FIT`) on 0 without failing.

## Refusals

`tests/provider_allocation.rs` refuses every allocation of open, meta, sections and block (of pages that are no pictures) in turn: each refusal is `NoMemory`, and the same provider reads on afterwards. `decoding_a_picture_may_be_refused_at_every_allocation` sweeps real pictures too (a PNG, a JPEG and a GIF as blocks, the cover and a page's picture): the header of the decoded file is read in the plugin (`pictures::open`, fallible path string) rather than by `ImageFile::open`, whose strings cannot be refused, and the writers' header is on the stack. A failure of memory or the card while a picture is decoded is now passed on by `block_at` (not turned into a named page); only a damaged picture is named. A name in the folder with a `:` is refused at open.

## Memory on the X4

The page list takes one entry (a name and a size, about 32 bytes plus the name) per picture of the folder, grown as the listing goes (no up-front room for the most there can be, which was about 19 KB per open), at most 2 x 300 names while listing and 300 after the cut. The listing sorts in place (`sort_unstable_by`: the order is total), so there is no scratch room that a refusal would abort. The refusal sweep includes a folder of 200 names. Peak heap on the device is not measured.

## Removal step (one sweep, with the reader port and Core #944)

Status: **not complete** (the interface-6 form is still beside the native one; nothing registers both in a
shipped build: `NativeImages` is not in `paperpaw_app::NATIVE_PLUGINS` yet). Remove together:
1. `ImagesFormat` and its `Plugin` impl (`FORMAT`, `add_book_cache`, `add_format`, the `HOW` table kept for the manifest);
2. `ImagesDocument::open`/`document::opener` and the plugin-folder cache (`CACHE` stays as the root-6 prefix of the provider);
3. the app's `install_with`/store-offer line and the other plugins' tests that register `ImagesFormat`;
4. list `NativeImages` in `NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`.

## Named step after #610 (Kernel, draft 30)

Switch `pictures::open` to `ImageFile::try_open` (fallible, with the `TooLarge` split) and drop the plugin-side header parser, so there is one parser of the decoded-file header, not two.

## Left in place (intermediate)

The interface-6 `ImagesFormat` registration and `ImagesDocument::open` stay until the host activates native
plugins. X4 `.ppn` size: not measurable yet (built in, no entry point).

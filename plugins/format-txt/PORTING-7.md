# Interface-7 port

Version 1.0.2. Slice 1 of the format ports (#228, assignment #1344).

## Ported

`src/provider.rs`: `TxtProvider::call(io, input, out)` answers one `core.format@1` call
with the generated `core_format_v1` codecs (no hand-encoded records): op 2 open, 3 prepare
(UTF-16 book to the UTF-8 copy `text.utf8` in root 6, `Preparation`), 4 close, 5 meta,
6 meta_text, 7 sections, 8 block (header, text parts, runs, `OffsetMap` checkpoints; cut into
fragments between records with `more`/`next`/generation), 19 progress. Ops 1 and 9-18 answer
answer as `document.md` says a format with none does: op 1 no rows (not a sniffer), ops 9, 14 and 16 an empty list (`fmt.end`, total 0), ops 10-13, 15, 17 and 18 `NotFound`; anything else `Unsupported`. The book
is root 3 (path `""`), the copy root 6; `HostIo` maps both. The parser (`TxtDocument`) is the
interface-6 one, shared, so the blocks are the same; `tests/provider.rs` compares the two on the
same files (UTF-8, Windows-1252, UTF-16 both orders, Gutenberg header), fragments, refusals and
random bytes.

## Native entry (slice 2)

`declarations.rs` (`formats`: `format-txt.txt`, extensions `txt` and `text`, flag `prepared`, no
sniffing), `build.rs` (batches built into the image), `src/native_plugin.rs` (`NativeTxt`: `make`
submits the declaration; `event` answers a `core.format` stage call of its token with
`TxtProvider::call` over `HostIo`, encoding straight into the host's reply through
`Reply::write_with`, no copy), `src/native_entry.rs` (`NATIVE_ENTRY`, features `linked` and
`native-entry`), `native/metadata*` (heap, registrations, storage size/read/write/append; no
service, no feature; regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`). Tests: `tests/native_declarations.rs`, `native_linked_entry.rs`,
`hosted_construction.rs` (the host checks the declaration and refuses it as `Unsupported` until
Core #944 admits formats; the positive case is ignored with that name).

## Not wired yet (`tests/provider_pending.rs`, ignored, each names its owner)

- Host adapter `fmt.call` (Host #417) is unwired; activation is Core #944.
- `fmt.option` input rows (`input_rows` of `core.format`, Kernel #385). format-txt takes no
  options, so one input record is enough for it; the other formats are blocked on it.
- Title fallback: root 3 has no file name, so a book without a Gutenberg title gets no title
  from the plugin (interface 6 used the file name). The library (LIB) names an untitled book by
  its file; Host/LIB to confirm.
- `doc_meta` flag bit 1 (series) is left 0, as in Services' #341 facade.

## Refusals

A refused allocation is `NoMemory` end to end: `HostIo` carries it through `StorageError::Other`, an open that hits it leaves no document (the host tries again), and a finished UTF-16 conversion is opened again by the next prepare call.
`tests/provider_allocation.rs` refuses every allocation of open, meta, sections and block in turn.

## Open gates

- X4: a UTF-16 conversion takes 1 KiB per `prepare` call (about 50 ms budget per call is not measured); stack and time on the device are not measured. Gate stays open until the device test (owner OK).

## Removal step (one sweep, with the reader port and Core #944)

Status: **not complete** (the interface-6 form is still beside the native one; nothing registers both
in a shipped build: `NativeTxt` is not in `paperpaw_app::NATIVE_PLUGINS` yet). Remove together:
1. `TxtFormat` and its `Plugin` impl, `FORMAT`, the `add_book_cache` and `add_format` registration,
   the `HOW` table kept for the manifest (the texts move to the native how-to when the host shows it);
2. `TxtDocument::open`/`utf8_copy` and the plugin-folder UTF-8 cache (`document.rs`): the document is
   opened by the provider from roots 3 and 6 only;
3. the app's `install_with` line and the dev-dependencies of reader, files, file-manager and kosync
   on `TxtFormat` (their tests move to the native host path), `fuzz/fuzz_targets/txt.rs` to the provider;
4. list `NativeTxt` in `NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`.

## Left in place (intermediate)

The interface-6 `TxtFormat` registration, `TxtDocument` use of `utf8_copy` and its plugin-folder
cache stay until the host activates native plugins; they go together then (no shim).
X4 `.ppn` (`tools/plugin-files7/build.sh x4 --only format-txt`, version 1.0.2): 55,984 bytes; the packer accepts every frame (below 2,048 B). The `linked` hosted tests pass (`cargo test --features linked`).

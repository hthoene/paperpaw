# Interface-7 port

Version 1.0.2. Slice 1 of the format ports (#228, assignment #1344).

Status: **complete, native only** (ticket #228). The interface-6 form is removed (see "Removal step"); the
plugin is listed in `paperpaw_app::NATIVE_PLUGINS` and `tools/plugin-files7/complete.txt`. Until the shells
activate native plugins (Core #944, Host #417) a `.txt` book does not open at all: the PR is held until
activation.

## Ported

`src/provider.rs`: `TxtProvider::call(io, input, out)` answers one `core.format@1` call
with the generated `core_format_v1` codecs (no hand-encoded records): op 2 open, 3 prepare
(UTF-16 book to the UTF-8 copy `text.utf8` in root 6, `Preparation`), 4 close, 5 meta,
6 meta_text, 7 sections, 8 block (header, text parts, runs, `OffsetMap` checkpoints; cut into
fragments between records with `more`/`next`/generation), 19 progress. Ops 1 and 9-18 answer
answer as `document.md` says a format with none does: op 1 no rows (not a sniffer), ops 9, 14 and 16 an empty list (`fmt.end`, total 0), ops 10-13, 15, 17 and 18 `NotFound`; anything else `Unsupported`. The book
is root 3 (path `""`), the copy root 6; `HostIo` maps both. The parser (`TxtDocument`) is the
one the interface-6 plugin had, so the blocks are the same; `tests/provider.rs` compares the provider with the
document's own reading of the same files (UTF-8, Windows-1252, UTF-16 both orders, Gutenberg header),
fragments, refusals and random bytes; `tests/encodings.rs`, `positions.rs` and `robustness.rs` are the golden
coverage of the parser (UTF-16 from the copy `Preparation` makes).

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

## Removal step (done)

Status: **complete**. Gone with interface 6: `TxtFormat` and its `Plugin` impl (`FORMAT`, `add_book_cache`,
`add_format`, the `HOW` table, the manifest and its `uses`), `TxtDocument::open` and `utf8_copy` with the
plugin-folder UTF-8 cache and its `.source` stamp (`.paperpaw/plugins/format-txt/<hash>.txt`; its row in
`docs/data-files.md` goes too: the copy is the host's `text.utf8` in the document cache, made by `Preparation`
and stamped by the host) and `push_utf16` (the converter is `utf16.rs`). `TxtDocument::open_converted` is the one
way to open a book. The app no longer registers `TxtFormat` (`paperpaw-plugin-format-txt` is an optional
dependency of the app, enabled by `native-linked`), `NativeTxt` is in `NATIVE_PLUGINS` and `complete.txt`.
`fuzz/fuzz_targets/txt.rs` converts a UTF-16 input with `Preparation` and opens it with `open_converted`.
The inline test of the plugin-folder copy (a replaced file of the same size is copied anew) went with the
stamp; the host owns that now (`cache stamps`).

The reader's tests that read a `.txt` book (`plugins/reader/tests`) are ignored with
`needs native activation (format removal)` and keep their assertions; they run again when the shells
activate native plugins (see the PR notes for the list). The `how/*.txt` texts stay in the package; the
registry cannot read them yet (`plugin-how-text-read`).

## Still blocked on the host

- Host adapter `fmt.call` (Host #417) and activation (Core #944): the declaration is refused as `Unsupported`
  today (`tests/hosted_construction.rs`).
- The reader's tests of `.txt` books through the kernel need the host to open a format through a loaded plugin.
- Title fallback (root 3 has no file name), `doc_meta` series bit and `fmt.option` rows: as above.

X4 `.ppn` size: see the PR notes (built with `tools/plugin-files7/build.sh x4 <out> --only format-txt`).

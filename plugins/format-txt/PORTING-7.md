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

## Not wired yet (`tests/provider_pending.rs`, ignored, each names its owner)

- `format.spec` declaration builder in the SDK (`Declarations` has none): no `NATIVE_ENTRY`,
  no metadata, no `.ppn`.
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

## Left in place (intermediate)

The interface-6 `TxtFormat` registration, `TxtDocument` use of `utf8_copy` and its plugin-folder
cache stay until the host activates native plugins; they go together then (no shim).
X4 `.ppn` size: not measurable yet (built into Beta 3, no artifact; no entry point yet).

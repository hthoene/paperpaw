# Interface-7 port

Version 1.0.2. Slice 1 of the format ports (#228, assignment #1344).

## Ported

`src/provider.rs`: `TxtProvider::call(io, input, out)` answers one `core.format@1` call
with the generated `core_format_v1` codecs (no hand-encoded records): op 2 open, 3 prepare
(UTF-16 book to the UTF-8 copy `text.utf8` in root 6, `Preparation`), 4 close, 5 meta,
6 meta_text, 7 sections, 8 block (header, text parts, runs, `OffsetMap` checkpoints; cut into
fragments between records with `more`/`next`/generation), 19 progress. Ops 1 and 9-18 answer
`Unsupported` (a text has no sniffing, table of contents, links, fonts or pictures). The book
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

## Left in place (intermediate)

The interface-6 `TxtFormat` registration, `TxtDocument` use of `utf8_copy` and its plugin-folder
cache stay until the host activates native plugins; they go together then (no shim).
X4 `.ppn` size: not measurable yet (built into Beta 3, no artifact; no entry point yet).

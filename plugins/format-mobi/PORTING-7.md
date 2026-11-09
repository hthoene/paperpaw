# Interface-7 port

Version 1.0.3 (the interface-7 bump of this cycle; no further bump). Slice 1 of
the Mobipocket port (#228, assignment #1715, after format-txt #427 and
format-xtc/images #434).

## Ported

`src/provider.rs`: `MobiProvider::call(io, input, out)` answers one
`core.format@1` call with the generated `core_format_v1` codecs: op 1 sniff
(`BOOKMOBI` at 60, as the interface-6 sniff), 2 open, 3 prepare, 4 close,
5 meta, 6 meta_text, 7 sections (one, the text's length), 8 block (header, text
parts, runs, `OffsetMap` checkpoints, a picture's `fmt.image`; cut into
fragments between records with `more`/`next`/generation), 9 toc (paged, 63 rows
and a closing `fmt.end`), 10 toc_target, 14 links (the book's own, as `#<place>`,
paged), 15 link_target, 17 cover, 18 image. Ops 11-13 answer `NotFound`, 16 an
empty list, 19 and anything else `Unsupported`. The book is root 3 (path `""`),
the cache root 6.

- **The shared code is the interface-6 code.** The converter (`text.rs`),
  `cache.rs`, `header.rs` and `images.rs` serve both; the interface-6 opener runs
  the same `Conversion` in a loop, so blocks are the same by construction and
  `tests/provider.rs` compares them (text, kind, offsets, runs, checkpoints,
  contents, links, pictures' pixels) against `MobiDocument` on the same books.
  The code was made fallible on the way: every allocation of the header, the
  converter, the cache writer and reader and the paths is `try_reserve`d and a
  refusal is `OutOfMemory`/`NoMemory`, never an abort (`src/mem.rs`); a card
  that fails in the middle of a book is an error (before, it ended the book as
  if it were whole).
- **Preparing is op 3.** An open of a book whose cache has no `done` answers
  `state` 1 and each `prepare` converts one text record (4 KB of text, then the
  blocks it made are appended to `blocks.bin`/`index.bin`/`links.bin`/`toc.*`),
  answering `progress_ppm`; the last record writes `done` and the state is 0
  with bit 6 (cache made now) and bit 1 (has contents). A cache with `done` opens
  ready; the host owns the stamp (`.complete`), so the interface-6 `done` stamp
  of (version, fingerprint, fit, colour) is not written natively. A refused
  allocation in a step answers `NoMemory`, drops what was made and starts again
  from the first record on the next call (a half-converted record cannot be
  resumed).
- **Pictures** are decoded when the host asks: a block's `fmt.image` has flags
  bit 0 (not decoded yet) until `img-<n>.gray` exists, op 18 decodes picture `n`
  at the open's `image_side` (a larger `image_side` in the call is a zoom file
  `zoom-<n>-<side>.gray`), op 17 the cover at `cover_side` (0: `COVER_FIT`). A
  picture that cannot be decoded is marked (`img-<n>.fail`) and is `NotFound`,
  not tried twice. A failure of memory or the card is passed on, only a damaged
  picture is named.
- Where the interface-6 reader showed a message (DRM, Huffman, KF8), the
  provider's failed document has only the status `Invalid` (see pending).

## Check of `input_rows` (the `fmt.option` need of #385)

None. The plugin reads no setting of its own (`OpenOptions::settings` is never
read); what interface 6 took from `OpenOptions` is `image_fit`, `cover_fit` and
`color`, which are the open's `image_side`, `cover_side` and `flags` bit 1. So
the plugin is not blocked on #385.

## Gaps for the later declaration and host work

- `Declarations::format` (extensions `mobi`, `azw`, `azw3`, a sniff, `prepared` (the cache is made by
  op 3), not whole pages) and the
  `NATIVE_ENTRY`/`.ppn` are not written here (Plugin Engineer's declarations PR);
  no `.ppn` size is measurable yet.
- The host's `fmt.call` adapter and the activation of format providers (#417, #944)
  are not wired: until they are, nothing calls this provider and the
  `provider_pending` cases stay ignored.
- Direction, writing mode and language per block (draft 29 `dir`, `writing_mode`,
  `lang`): Mobipocket has none; the records carry 0 and "". Embedded fonts: none.
- Title fallback: root 3 has no file name, so a book with no title has none (the
  library names it by its file).
- `fmt.state` carries a status, not a message: the reader can no longer say "this
  book has DRM" (interface 6 did). Ask Kernel for a status per reason, or keep a
  message the host shows.
- Two documents of one conversion open at once would share a cache that one is
  still making (a second open sees no `done` and starts again); the host's pinning
  rule should keep them apart.
- The time of a `prepare` step on the X4 is not measured (one 4 KB record plus
  the card writes; budget 50 ms), nor the peak heap of a block read.
- Decoding a picture (ops 17 and 18) reaches the SDK image writers whose first
  allocation is infallible (`GrayWriter::start`, Kernel: write the header from a
  stack array); it is not in the refusal sweep (`tests/provider_allocation.rs`
  sweeps everything else). `ImageFile::open` also builds its path infallibly.

## Refusals

`tests/provider_allocation.rs` refuses every allocation of open, prepare (every
step), meta, meta_text, sections, contents, links and blocks in turn: each
refusal is `NoMemory` and the call made again answers what a call without a
refusal answers. `tests/provider.rs` has a hostile-book loop (random damage and
cuts through open, prepare and reads: no panic), failed books (DRM, Huffman,
KF8, too short, empty) and bad calls (unknown document, id opened twice, too many
documents, bad input).

## Left in place (intermediate)

The interface-6 `MobiFormat` registration and `MobiDocument::open` stay until the
host activates native plugins; they run the same converter.

# Interface-7 port

Version 1.0.4. Native plugin: `src/native_plugin.rs` (`NativeEndOfBook`),
built-in descriptor `NATIVE_ENTRY` (feature `linked`), declarations in
`declarations.rs` (built into the image by `build.rs`), metadata in `native/`
(`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, and the same
for `metadata.host.*`; services `core.library` and `core.ui`, features
`uses.library` and `uses.storage`, the latter for the card-presence check). The screen uses `paperpaw-ui-kit`. The interface-6 plugin, its
screen and Kernel tests are gone; the app and the store-file build do not carry
it until Core publishes native plugins (the `tools/plugin-files` row is
commented out in `plugins.txt`).

## Ported

- The two switches (`end-of-book.show`, `end-of-book.mark`), the book events
  (`BookFinished`, `BookClosed`), the screen with its three rows (next book of
  the series, rating, finished) and the finished mark kept while the book is
  open and said again at its closing.
- The book is looked up in the library (`NotFound`: no
  screen) and its file on the card must exist. The next book comes from LIB op 6 (`next_in_series`), stepping over
  finished ones; opening it is a handover with `path` into `core.reader`
  (`command.open`, mode 1). No next book: `core.library`.
- Fallible allocation throughout the callbacks (kit `mem`).

## Tests

- Unit: the rating cycle, the field value, the star text, the rows.
- `tests/abi7_declarations.rs`: the image equals what the builder makes; the
  host admits the settings and the screen; the events are the two book edges.
- `tests/linked_entry.rs`: descriptor and metadata.

## Acceptance pending (explicit, never faked)

Pending cases are `#[ignore]`d refusals in `tests/acceptance_pending.rs`; the
eight interface-6 pictures stay in `shells/desktop/tests/snapshots` and their
cases left the live list of `snapshots.rs`.

| Need | Affects |
| --- | --- |
| H1 screen commands, H3 composition, H4 input | the screen's pictures |
| SDK: `Library::fields` (LIB op 22) | reading the rating back; until then the rating row shows nothing and refuses a press (`Unsupported`) rather than overwrite a rating unseen |
| SDK: book field and book badge declarations (`book_field.spec` 231, `book_badge.spec` 232) | `end-of-book.rating` as a field and `4/5` as a badge; not declared yet |
| Host stage `core.book_badge` codec (no `abi/stages/core.book_badge@1.def`) | the badge |
| Host: book events and library through the loader | the finished mark, the next book |

## Not carried

- "On the card" is `storage::size(Files, path)` as before (`NotFound`: gone, no screen
  and no finished mark said again; other refusals propagate); the library having the book
  is not proof (Services). It runs where interface 6 checked: at the end and at the closing.
- The next-book rule is the host's op 6 plus stepping over finished books; the
  interface-6 plugin walked the series itself.

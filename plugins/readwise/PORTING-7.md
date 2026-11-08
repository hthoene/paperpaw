# Interface-7 port (slice 1: reading the notes)

Version 1.0.7. Partial: what the plugin decides without the host. The plugin declares no screen,
setting or action (it only hears events), so there is no `declarations.rs`, and no `NATIVE_ENTRY`
yet; the interface-6 `Readwise` and everything of it stays until the host activates native plugins
(then it goes together, no shim).

## Ported

- `src/native_readwise.rs`: the Markdown reader (`Parser`: title, author, the highlights under the
  first heading, each with its place and note, later headings left out, every field cut to what
  Readwise takes) and the JSON of a highlight (`Parser::json`, byte for byte `paperpaw_sdk::json::string`
  escapes). Allocation is fallible and swept; a highlight that cannot be kept starts nothing, so the
  one before it is not lost. `src/native_parity_tests.rs` compares both with the interface-6 parser
  over notes of every shape (CRLF, no author, long and multi-byte texts cut at the limits, odd
  percents) and the JSON escapes.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The sending: waiting list, request files, the answer, retries (files, network, declared work).
- The token: the keyboard and the secrets (`core.secrets`, #367), a token Readwise refuses.
- The events it hears: `core.notes-exported`, `started`, its own retry and answer events.
- The retry as declared work (`Work::network(..).within(..)`): no native work declaration yet.

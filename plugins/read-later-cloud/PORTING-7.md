# Interface-7 port (slice 1: the decisions and the declarations)

Version 1.0.4. Partial: what the plugin decides and what it declares, without the host. No native
set-up screen and no `NATIVE_ENTRY` yet; the interface-6 `ReadLaterCloud` and everything of it stays
until the host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_read_later.rs`: which texts are article ids (`is_id`), where Reader keeps what is
  asked for (`location`), the number that tells accounts apart (`account`), the start and end of
  an article's book (`head`, `TAIL`), the lists it keeps (`articles`, `finished`, `articles_text`),
  when a page turn is the article read to the end (`is_read`, from the native reading event's page,
  pages and progress in millionths), and the copying of an article's text out of Readwise's answer
  (`Copier`: fed the answer in pieces, finds `html_content`, unescapes as it goes, holds only what
  the caller has not written yet, cut at 1 MiB). Allocation is fallible and swept; `Copier::feed`
  refuses (`NoMemory`) where the text taken cannot be kept, so the caller writes and clears it
  often. `src/native_parity_tests.rs` compares each with the interface-6 code, the copier over
  every kind of escape, broken or missing key and value, fed in pieces of 1 to 100,000 bytes.
- `declarations.rs`: the setting for where articles come from (*Later* or *Inbox*, kept on reset,
  library group), the set-up screen (one at a time), the actions `open` and `fetch`, the place in the
  tools (order 49) and the icons. `tests/abi7_declarations.rs` decodes the batches by the host's rules
  and compares each with the interface-6 registration, including the screen's instance policy.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The fetch: the lists and the answers about an article (`answers.rs`, read with
  `paperpaw_sdk::json::stream`, whose events own their strings and allocate without a refusal path):
  to be read with the SDK parser's fallible `try_feed`/`try_finish` in slice 2, as wallabag's.
- The set-up screen (H1, H2), the token (secrets, #367), the keyboard slot and the set-up's row.
- The archive: the page-turn event, annotations (a book with the reader's marks stays), the account
  check that lets go of another account's lists (secrets), and the book's free name (files).
- The fetch started by a rule as declared work (no native work declaration yet).

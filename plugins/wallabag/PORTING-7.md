# Interface-7 port (slice 1: the decisions and the declarations)

Version 1.0.7. Partial: what the plugin decides and what it declares, without the host. No native
set-up screen and no `NATIVE_ENTRY` yet; the interface-6 `Wallabag` and everything of it stays until
the host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_wallabag.rs`: which texts are article ids (`is_id`), the set-up's rows (`Row`: title,
  keyboard field, and the row asked for by a field), the lists it keeps (`articles` and `finished`
  over the file's text, `articles_text` writing the first back), and when a page turn is the article
  read to the end (`is_read`, from the native reading event's page, pages and progress in millionths:
  95 % where pages are not counted). Allocation is fallible and swept. `src/native_parity_tests.rs`
  compares each with the interface-6 code, including lines of another shape and CRLF lists.
- `declarations.rs`: the three settings (server; user name and client ID as advanced ones; all kept on
  reset, in the library group, edited on the set-up screen), the screen (one at a time), the actions
  `open` and `fetch`, the place in the tools (order 48) and the icons. `tests/abi7_declarations.rs`
  decodes the batches by the host's rules and compares each with the interface-6 registration,
  including the screen's instance policy from the decoded flags.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The fetch: the token, the list, each article's check and book (network, files). The list of unread
  articles and an article's `is_archived` are read with `paperpaw_sdk::json::stream`, whose events
  own their strings and allocate without a refusal path; a fallible stream reader (kit) is needed
  first, so `unread` and `is_archived` are not ported yet.
- The set-up screen (H1, H2), the password and client secret (secrets, #367), the keyboard slot.
- The archive: the page-turn event, annotations (a book with the reader's marks stays), the account
  check that lets go of another account's lists, and the book's free name (`book_path`, files).
- The fetch started by a rule as declared work (no native work declaration yet).

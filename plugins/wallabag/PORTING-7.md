# Interface-7 port (slices 1 and 2: the decisions, the declarations and the readers)

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
- `src/native_fetch.rs` (slice 2): the list of unread articles (`Unread`: the first 30 with an id that
  is a number, `None` for a list that never ends) and an article's archived flag (`Archived`). Both
  read `paperpaw_sdk::json::stream::Parser` with `try_feed`/`try_finish`, so a refused allocation
  of the parser's path or text ends in `NoMemory` and nothing is returned in part; what the plugin
  keeps (items, ids, titles) is allocated with `try_reserve`. A title is kept as the book's name
  (`clean_name(title, NAME_UNITS)`, 80 UTF-16 units, the only use the plugin has for it; cleaning is
  idempotent, so the same book comes of it): 30 titles of 4 KiB do not stay in memory. The caller feeds the file in pieces as
  it reads it (`feed`, then `finish`); a list of any length is read in the parser's bounded memory,
  and only the first 30 items with an id are kept. `src/native_fetch_tests.rs` compares each with
  the interface-6 `unread` and `is_archived` at piece sizes from one byte to 2 KiB and sweeps
  every allocation.
- Read caps (slice 2): the native caller keeps interface 6's caps. `articles` accepts a file of at
  most `ARTICLES_CAP` (64 KiB) and `finished` one of at most `FINISHED_CAP` (16 KiB), counted with
  a byte order mark, which is left out of the text as in interface 6; over the cap they say
  `NoSpace`, never an empty list, so the caller does not write the file over what it holds. The
  streamed answers (`entries.json`, `entry.json`) have no cap, as in interface 6, as they are read
  in pieces.
- `declarations.rs`: the three settings (server; user name and client ID as advanced ones; all kept on
  reset, in the library group, edited on the set-up screen), the screen (one at a time), the actions
  `open` and `fetch`, the place in the tools (order 48) and the icons. `tests/abi7_declarations.rs`
  decodes the batches by the host's rules and compares each with the interface-6 registration,
  including the screen's instance policy from the decoded flags.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The fetch: the token, the list, each article's check and book (network, files). The readers of the
  list and the flag are ported (`native_fetch`); the calls that feed them are the host's.
- The set-up screen (H1, H2), the password and client secret (secrets, #367), the keyboard slot.
- The archive: the page-turn event, annotations (a book with the reader's marks stays), the account
  check that lets go of another account's lists, and the book's free name (`book_path`, files).
- The fetch started by a rule as declared work (no native work declaration yet).

# Interface-7 port (slice 2: the decisions, the declarations and the feed converter)

Version 1.0.10. Partial: what the plugin decides and what it declares, without the host. No native
list screen and no `NATIVE_ENTRY` yet; the interface-6 `News` and everything of it stays until the
host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_news.rs`: the mark of a feed's address (`Mark`), a book's name without it
  (`without_mark`), a feed's short title, the feeds the plugin keeps (`feeds` reads `feeds.txt`,
  `feeds_text` writes it; `Feed::shown` is what the list shows), the headlines it keeps
  (`headlines`: the newest first, the old ones of the same book dropped, 40 at most), the dates
  feeds give (`read_date`: ISO 8601 and RFC 822), and the text helpers the conversion needs
  (`cut`, `one_line`, `escape`, `is_block`). Allocation is fallible and swept.
  `src/native_parity_tests.rs` compares each with the interface-6 code (`read_date`, `cut`,
  `one_line`, `is_block` and `escape` of `convert.rs` became `pub(crate)` for it, nothing else
  changed).
- Read caps of interface 6, enforced in code: `feeds` takes a file of at most `FEEDS_CAP` (64 KiB)
  and says `NoSpace` over it, never an empty list, so the caller does not write the file over what
  it holds (a byte order mark is left out). `headlines` takes the old file only up to
  `HEADLINES_CAP` (32 KiB); a larger one counts as none and is written over, as in interface 6.
  `MAX_FEED_FILE` (4 MiB, a feed larger than it is not taken), `MAX_TEXT_SMALL` (8 KiB) and
  `MAX_TEXT` (64 KiB, the text kept of an article) and `MAX_FIELD` (400 bytes) are the same
  constants as before.
- `declarations.rs`: the setting for the articles a feed keeps (5 to 200, 30 by default, in steps of
  five, advanced, in the library group and in the part named after the plugin), the list of feeds
  (one screen at a time), the actions `open` and `fetch`, the place in the tools (order 44) and the
  icons. `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each
  with the interface-6 registration, including the screen's instance policy from the decoded flags.

- `src/native_convert.rs`: a feed (RSS 2.0, RSS 1.0, Atom) made into a page (`Converter`: `feed` in
  pieces, `taken`/`clear` to write the page as it comes, `finish` for the count and the headlines).
  The XML is read by the SDK's `Tokenizer::try_feed`; everything kept is reserved first; one article
  is in memory at a time. After a refused allocation the converter is spent (every later `feed` and
  `finish` say the same; the caller ends the fetch and throws the page away, so a feed that fails
  keeps the old book). Caps as in interface 6: `max_text` per article (64 KiB, or 8 KiB on small
  devices, given by the caller), 400 bytes a title/date/link, five headlines named at the top; the
  4 MiB limit on the feed file (`MAX_FEED_FILE`) is the fetch's, not the converter's.
  `src/native_convert_tests.rs` compares the page and the result byte for byte with `convert.rs` at
  piece sizes from 1 byte up, and sweeps refusals. (`convert::paragraphs` became `pub(crate)` for
  that; `native_news` gained `escape_len` and `HEADLINES_SHOWN`.)

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The book's name (`Feed::file_name`/`book`, which need a fallible `clean_name`: a gap
  for Kernel, the SDK's allocates and the reserved-name list is in `paperpaw-platform`). Next slice.
- The fetch itself (one feed after the other, the network and files, the page's rename, the
  retirement of version 1's book) and its notices.
- The list screen (H1, H2: *Fetch the news*, the feeds, *Add a feed*, the choices on a feed) and the
  keyboard slot for the address.
- The headlines widget (its group and 14 settings): with the widget pack's kit (H1, H3).
- The fetch started by a rule as declared work (no native work declaration yet).

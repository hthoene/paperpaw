# Interface-7 port (slice 4: the decisions, the declarations, the converter, the page and the fetch)

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

- `src/native_book.rs` (slice 3): a feed saved in the data folder made into its page there
  (`make_page` over `paperpaw_ui_kit::files::Volume`): the feed is read 2 KiB at a time into the
  converter, the page is appended in calls of at most 4 KiB (`MAX_DATA`) and synced, a feed over
  4 MiB or without an article is `Failure::Failed` and leaves no page, a refused allocation is
  `Failure::Memory`. The page is appended straight to `page`, with no staging file: the caller
  imports it only after `Ok`, and a power cut (or a failed write whose clean-up fails too) leaves a
  stale, partial `page` that the next call removes before it starts (`a_cut_leaves_a_stale_page_…`
  cuts after every append). `Failure::Failed` merges the causes (no feed, too large, no article, a
  card that does not read or write); the caller only needs "keep the book there is", and the
  counts of the run do not tell them apart. `src/native_book_tests.rs` compares the page and the result with
  `convert::convert` on the corpus of the converter's tests and sweeps refusals. Next: the
  fetch over `core.net` (saved with `save_to`, at most 4 MiB with `max_file`), the book put in the
  library with `core.library` `import` (name `Title (mark).html` in the folder `News`; needs
  `Library::import`, SDK PR #668), the headlines file; then the list screen and the widget.
- `src/native_fetch.rs`, `src/native_plugin.rs`, `src/native_entry.rs` (slice 4): the fetch. A
  `Fetcher` asks for one feed after the other (`GET`, the answer saved to `feed-<mark>.tmp` in the
  data folder with `max_file` 4 MiB, the plugin's `User-Agent`), and when a request ends reads the
  list again, makes the answer into its page and hands it to a `Shelf` (`feed`, `name`, `page`,
  `converted`), keeps the name a feed brought, asks for the next one (`all`) or ends with the counts;
  a feed removed meanwhile is left out, a feed that failed (no 2xx answer, no article, the shelf
  refused) does not stop the others, a refused allocation stops the fetch. It sees the world only
  through `Wire` (the data folder as a `Volume`, `get`, `cancel`, `declare_work`), so it is tested
  without a host (`src/native_fetch_tests.rs`: a volume in memory, a recording network, a recording
  shelf); `native_plugin.rs` is the glue (`HostWire` over `core.net`, `core.work` and the storage
  calls), the notices (`news.fetching`, `news.fetched` with its plural, ...) and the `date_text`.
  `tests/native_flow.rs` drives the linked plugin: what it registers (the setting, `HttpDone` and
  `WorkDue`), the work the action `news.fetch` declares (network, optional, at the wake and when
  online, an hour on: interface 6's `Work::when_cheap(.., 3600 s)`), a work that cannot be declared
  costs that fetch only, a due work where the list cannot be read says so.
  The image holds the setting and the two subscriptions only: the action, the screen, the menu
  entry and the icons are not in it (`declarations::navigation` and `screens` are built when the
  slices that implement them are), so nothing is offered that opens nothing.

### Differences from interface 6, on purpose, and gaps

- The action `news.fetch` always declares the background work: a native action cannot tell a
  rule's run from the reader's (interface 6's `env.reader()`); the reader's own fetch is the screen's
  *Fetch the news* (next slice).
- The shelf is `Unshelved` until `Library::import` is in the SDK (#668): no book is made, every
  feed counts as failed. The plugin is not listed as complete and is not in the app yet.
- An article's text is cut at 8 KiB (`MAX_TEXT_SMALL`) on every device: the native snapshot has no
  memory class, and interface 6 gave 64 KiB to the larger ones. An open question for Kernel (the
  memory class in the snapshot or `core.device`). The fetch passes `MAX_TEXT_SMALL` to `make_page`
  on the X4 because the peak is about 4 × `max_text` per article (the converter's field, the
  paragraphs, the escaped text and the page piece); 64 KiB would be ~256 KiB, the whole heap.
- A feed's name is not yet cleaned for a file name (`web::try_clean_name` is there; the library's
  import names the book, next slice).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The book's name (`Feed::file_name`/`book`, which need a fallible `clean_name`: a gap
  for Kernel, the SDK's allocates and the reserved-name list is in `paperpaw-platform`). Next slice.
- The fetch itself (one feed after the other, the network and files, the page's rename, the
  retirement of version 1's book) and its notices.
- The list screen (H1, H2: *Fetch the news*, the feeds, *Add a feed*, the choices on a feed) and the
  keyboard slot for the address.
- The headlines widget (its group and 14 settings): with the widget pack's kit (H1, H3).
- The fetch started by a rule as declared work (no native work declaration yet).

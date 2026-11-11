# Interface-7 port (the whole plugin; the interface-6 code is removed, HELD until native activation)

Version 1.0.10. Status: **complete, native only** (ticket #244). The native plugin (`native_plugin`, `NATIVE_ENTRY`,
feature `linked`) is the whole plugin: the list of feeds, the fetch, the library import and the headlines widget.
The interface-6 plugin is gone: `News` with its registration (`lib.rs`), its list screen and widget (`widget.rs`),
its converter (`convert.rs`), the interface-6 tests (`tests/fetch.rs`, `fetching.rs`, `widgets.rs`, `src/feeds_tests.rs`
and the tests of `lib.rs`), its place in `paperpaw-app` (`install_with`, `STORE_PLUGINS`, `NETWORK_SETTINGS`) and its
store-file builder in `tools/plugin-files` (`plugins.txt` keeps a comment line). Goldens changed by this (the Tools list lost its *News* row): `crates/paperpaw-app/tests/snapshots/tools.txt` and the emulator images `boox-all-tools.png`, `x4-tools.png`, `x4-drawer-de.png`; no others. **HELD until native activation
(#243):** the interface-6 app would lose News before; this PR must not merge before. `tools/plugin-files7` builds and
packs the native file for the X4 (`news-1.0.10-x4.ppn`, 196,112 bytes before this change: 157,540 code, 33,408 data;
the removal changes no native code, so the size is the same; the packer accepts it, no frame over 2,048 B). The page's
read buffer is on the heap (a 2 KiB array made the frame 2,368 bytes, over the packer's limit of 2,048).

## What replaced the interface-6 tests

The interface-6 code is gone, so what it gave is kept as data and the native code is tested against it:
`src/native_golden.rs` holds what the interface-6 converter, `mark`, `short_title`, `without_mark`, `load`/`save` of
`feeds.txt`, `headlines`, `read_date`, `cut`, `one_line`, `escape` and `is_block` gave for fixed inputs (pages as
their length and FNV-1a hash; made with the interface-6 code before its removal), used by
`native_convert_tests`, `native_book_tests` and `native_rules_tests` (the same cases as the former parity tests, at
the same piece sizes). `native_convert_hostile_tests` runs the former hostile-feed tests (whole pages, no markup of
the feed's, limits; proptest) through the native converter. `tests/abi7_declarations.rs` compares the declarations with
the interface-6 registration kept as data (`mod interface_6`). The feeds file of version 1 is still read and written
alike (`feeds_of_version_1_are_written_alike`, `testdata/feeds.txt.v1`). `HOW` (the help texts) stays, published by the
host at admission as in the other native plugins.

The behaviour the removed integration tests pinned on a real kernel (`fetching.rs`, `fetch.rs`, `widgets.rs`) is
covered by the native tests that exist (`native_fetch_tests`: one feed after the other, a failed one does not stop the
rest, a feed removed meanwhile, ends not under way ignored, the counts; `native_book_tests`: page and cut power;
`native_books`: the book of a feed by its mark; `native_screen`: the rows and an address is a web address;
`native_widgets`) and by the cases in `tests/acceptance_pending.rs`, which stay ignored, each `BLOCKED` on the host,
with their assertions: the integration with the real host (hostile titles through the library import, the widget's
draw and tap) is not run until native activation.

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
- Slice 5 (`src/native_shelf.rs`, `src/native_books.rs`, `src/native_screen.rs`): the shelf is the
  library (`Library::import`, SDK #668): the page goes in as `Name (mark).html` in the folder `News` with
  *replace*; a book that is not the plugin's any more (moved, or a file of the reader's own at that name)
  stays and the new one takes a free name. The library says where the book went, and the plugin keeps
  that path next to the feed's mark in `books.txt` (`mark<TAB>path`, at most 300) and the newest headlines
  in `headlines.txt` (as interface 6). The list of feeds is the screen `news.list` (one at a time):
  *Fetch the news*, the feeds, *Add a feed*, and what feeds are when there is none; a tap opens the
  feed's book in `core.reader` (or fetches a feed with no book), a hold offers *Open / Fetch / Remove*,
  *Add a feed* asks for the address on the keyboard (from `https://`). The action `news.open`, the place
  in the tools (order 44) and the icons are declared as in interface 6.
- Differences from interface 6, on purpose: no *go online* offer in Offline mode (the network service
  refuses and the notice *needs a network* says so, as in joplin and calibre); the book of a feed is found
  by the path the library gave (`books.txt`), not by looking for a file name: a book the reader moved is
  opened where it is kept only while `books.txt` knows it, else the feed is fetched again; the books of
  version 1 (by name alone) are not retired. After an update from Beta 3 `books.txt` is empty, so a tap
  fetches the feed once, and the library may refuse to replace the book interface 6 made (it was not
  imported by this plugin): the new one then takes a free name beside it (not measured on a real card).
- Not measured: the heap peak of the screen on the X4 with many feeds, the time of `books.txt` and
  `headlines.txt` reads on each opened feed, the library import of a 64 KiB page.
- The headlines widget (`src/native_widgets.rs`, `declarations::widgets`): the group `news.widget.headlines`
  and its fourteen settings in the order of Beta 3 (compared with the interface-6 registration by
  `tests/abi7_declarations.rs`), no `.previews`. The host makes an instance per placement; the plugin reads
  `headlines.txt` when the widget is measured and drawn (a headline whose book is gone is left out), a tap
  on a headline opens its book in `core.reader`, a tap on the empty widget opens the list. A fetch's end and
  a changed `news.widget.*` setting (`event.setting_changed`, token 21; a host that does not admit it yet
  answers `Unsupported` and the widget shows the change at its next measure) tell the instances to ask again.
  Differences: the rule between headlines is a plain light gray line (interface 6 dithered it); the kit lays
  the lines out (`label`, `wrap`, `caption`), so a line may break at another place than the old `Wrapped`.
  Not measured: the widget's frames on the X4 and its draw time with eight headlines of two lines. X4 timing
  gate: each measure, targets, draw and tap reads `headlines.txt` once and calls `storage::size` for up to
  eight books (the library's `missing` bit would save these).
- A new feed whose list cannot be saved is not added: the notice *news.unsaved* says so (a fetch would ask
  for a feed the re-read list no longer holds).
- The plugin's metadata asks for what the code calls: `core.library`, `core.ui` next to the earlier three
  services, the features `uses.library` and `uses.storage` (`Root::Files` sizes), and the drawing host calls
  of the widget (`canvas.*`, `text.*`, `font.acquire`).
- The plugin is not listed as complete and is not in the app yet: the removal of interface 6 (slice C,
  held until native activation) follows.
- An article's text is cut by the device's memory class (`core.device` op 1 `dev_device.class`,
  read once at the first finished request, `native_plugin::text_cap`): 64 KiB (`MAX_TEXT`) on a
  medium or large device as in interface 6, 8 KiB (`MAX_TEXT_SMALL`) on a small one (the X4: the
  peak is about 4 × `max_text` per article, the converter's field, the paragraphs, the escaped text
  and the page piece; 64 KiB would be ~256 KiB, the whole heap) and wherever the class cannot be read.
- A user-bound `news.fetch` action (a menu entry, once `navigation` is built into the image) would
  declare the background work and so wait an hour (`BACKGROUND_AFTER_MS`) before it fetches; the
  reader's own fetch is the screen's *Fetch the news* (next slice) and starts at once (`start`).
- A refused next request ends the fetch (`Ended::NoNetwork`, the notice *needs a network*) with the
  counts kept; the end of the run (`Reason::WorkEnded`, 10 minutes) stops it and asks for no more;
  a start replaces a fetch under way and ends its request first.
- Unmeasured: the peak while the list (at most 64 KiB, `FEEDS_CAP`) is alive during `book`, on top of
  the ~4 × `max_text` of the conversion. To be measured on the X4 with the screen.
- A feed's name is not yet cleaned for a file name (`web::try_clean_name` is there; the library's
  import names the book, next slice).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

Nothing of the plugin is missing natively (the screen, the fetch, the book's name, the widget and the declared work
are in; see the slices above). What waits is the host: the four acceptance cases (`news-feeds`, `news-fetch`,
`news-headlines-widget`, `news-fetch-due`) stay `#[ignore = "BLOCKED …"]` until the host qualifies H1/H2/H3, the
network, the files and declared work for native plugins.

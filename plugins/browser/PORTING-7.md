# Interface-7 port (native only; the blocked cases wait for host pieces)

Version 1.0.8 (Beta 3: 1.0.6, so no further bump). Status: complete in code. Native only: the interface-6
`Browser`, its `store.rs` and `web.rs` and `tests/browsing.rs` are gone (no shim, todo 90); the checks
that compared the native rules with them are pinned values taken from that code before it went
(`src/native_tests.rs`, `src/native_format_tests.rs`, `tests/abi7_declarations.rs`). `HOW` and `CATALOGS`
(en/de) are the statics the host publishes at admission. Not linked into the app (it is no longer in
`paperpaw_app::STORE_PLUGINS`) and listed in `tools/plugin-files7/complete.txt`; what the host must still
give is the list of blocked cases at the end.

## Slice 3: downloads (`src/native_save.rs`)

- A book (EPUB, text) goes to the library with `library.import` from the download file (root 0),
  the server's file name, **free name** (`name (2).epub`): the host moves it and catalogues it. The
  plugin then says "in the library" and asks *Read now* / *OK*; *Read now* opens it with
  `command.open` on slot `core.reader` and the hand-over `path`.
- A font goes to the card's `fonts` folder, any other file to `Downloads`, by `core.files` move from
  root 0 to root 1 (`uses.storage`), each with the first free name of ninety-nine (`State` for a taken
  name, never a replace). `Unsupported` for a font says `browser.no-fonts` (a host without a card
  root has no fonts folder to give).
- `uses.library` and `uses.storage` are not required: without them (`Denied`) a download says
  `browser.no-permission`; ninety-nine taken names say `browser.name-taken`; pages and bookmarks still work.
- Differences from interface 6: (1) a book keeps the server's file name; interface 6 renamed it after
  its author and title. That needs the document service's metadata (`doc_meta`, op 3): the SDK has no
  wrapper yet and the host piece is **DOC-metadata**, so it is an ignored case, not faked. (2) The
  *Read now* offer is for every book (EPUB or text: the kinds the browser takes as books), where
  interface 6 asked the format registry first. (3) The fonts folder is the card's `fonts` folder;
  interface 6 used the device's first font folder (`Env::font_dirs`), which native has no call for.
  (4) A request sets no `max_file` (0 = the host's 64 MiB) for pages and downloads alike, as Kernel
  ruled (todo 319): Beta 3 saved every answer with one `save_to` and no cap; a full card ends with
  `too_large` or a card error. The page is read in bounded steps (`keep_page`, 4 KiB pieces).

## Slice 2 (`src/native_plugin.rs`, `NATIVE_ENTRY` with `linked`)

- The start screen (`paperpaw_ui_kit` item list): *Search or enter address*, the bookmarks and the
  pages read lately, a line for messages below. Holding an entry offers open / bookmark / delete
  (bookmarks: open / delete) as a choice dialog; the address is typed in a text dialog (URL mode).
- Loading: `core.net` GET with `save_to` (data folder, `download.tmp`), `User-Agent` and `Accept`
  as in interface 6; the answer is read from `http_done` (status, final address, content type). An
  error answer's page comes in memory (`Net::body`) and is kept to be shown; an error without a page
  is told (`browser.error`). No network, offline, saving or the radio off is `browser.no-network`.
- A page is kept with its address as `<base href>`, titled, put in the history and opened in the
  reader with a hand-over of `root=0`, `path=pages/<hash>.html` (`command.open` on slot `core.reader`).
  Back from the reader goes through the pages of this time; a page whose file is gone is loaded again.
- Closing the last browser screen cancels the request (`Net::cancel`); an answer that still comes
  with no screen open is discarded (its file removed), so the reader is never opened over a book.
  - The screen takes the address as the hand-over parameter `url` (interface 6: `params::URL`).
- Memory is refused, never aborted: every allocation of the plugin is fallible. Frames stay below
  2,048 B (the kit's list). The texts (en/de) are the ones of interface 6.
- `tests/hosted_construction.rs`: construction in the host's registered scope: `make` runs to its end and the host admits
  all its declarations (the registry views are wired on integration).

## Ported

- `src/native_web.rs`: the address for what was typed (`address`: an `http(s)` address as it is, a
  name with a dot or a port with `https://`, else a search), what an answer is (`kind`: content type,
  then the address's file name, then the first bytes), the title of a kept page (`title`, from the
  first 16 KiB), the page kept with its address as `<base href>` (`keep_page`, in 4 KiB pieces) and
  the page's stable name (`page_name`). The same answers as `web.rs`: `src/native_tests.rs` compares
  both on fixed corpora and at random (proptest), and refuses every allocation in turn.
- `src/native_store.rs`: bookmarks and the history (`title<TAB>address` lines, version 1 files stay
  readable, with or without a byte order mark): `load`, `save`, `bookmarks` (the three first places,
  in the device's language), `remember` (30 pages, the dropped ones' files removed). Paths are
  relative to the data folder (`DataStorage`, root 0): `bookmarks.txt`, `history.txt`,
  `pages/<hash>.html`, `download.tmp`.
- `declarations.rs`: the screen, the `core.browser` offer, the action *browser.open* with its place in
  the tools menu (order 43) and its icon, and the `http_done` event. `tests/abi7_declarations.rs`
  pins each field to what the interface-6 registration said.

## Differences from interface 6 (deliberate, bounds in code)

- An address is at most 1,024 bytes (`net::MAX_URL`, the longest `core.net` takes). Typed text that
  would give a longer address is `Error::NoSpace`; a stored address over it is dropped when a list is
  read. Interface 6 passed it on and the request failed.
- A list file is read up to 64 KiB (interface 6: 256 KiB, more than the X4's heap), at most 400 entries,
  a title cut at 256 bytes on a character. A larger or longer list is `TooLarge`: it is left as it is,
  never cut and written back (`bookmarks` then shows none, `remember` changes nothing).
- Memory is short: `title` says `Error::NoMemory` where interface 6 said "no title"; the screen falls
  back to the host name in the same case, explicitly.
- Saving a list reports the storage's refusal (`StoreError`); interface 6 ignored it.
- `paperpaw_sdk::xml::try_decode` is now exported (a one-line SDK change); the SDK's `web::try_file_name`
  (PR #506) is used for the address's file name. This PR is stacked on #506 until it merges.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The screen (H1, H2, H4): the start screen and its three lists, the offer to go online in Offline
  mode (`network.md` 3.2: a request that ends with reason `offline` and `later`), the choices on a
  held entry, the address dialog and the messages.
- Loading: `core.net` GET with `save_to` (data folder) and the `Accept` and `User-Agent` headers, the
  answer from `http_done`, an error page kept to be shown; no host for requests on integration
  (`core-net-host`).
- Opening a kept page in the reader (`command.open` mode 1 or 3 with `page`, `ui.md`: the table of interface-6 commands): the page is a
  file in the data folder; the host must open a path of the plugin's own folder (`DOC-open-page`).
- Downloads: the import, the moves and the offer to read need the library and files hosts (`LIB-import`,
  `FS-core-files`) and the dialog (H2); naming after author and title needs `DOC-metadata` and an SDK
  wrapper for `doc_meta`.
- The slot `core.browser` is offered, but the screen takes the address as a parameter (`params::URL`)
  in interface 6 and the native hand-over of a parameter to a slot's screen is **to settle** with
  Kernel (`ui.md` 1.3).
- The `ui::offline::OfflineOffer` and the chooser in the screen are interface-6 widgets; the native
  start screen is lists of `paperpaw_ui_kit`.

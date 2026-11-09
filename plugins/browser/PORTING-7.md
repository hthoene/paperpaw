# Interface-7 port (slice 2: the screen, loading, pages in the reader)

Version 1.0.8. Partial: the native screen and the loading of pages are in place; downloads of books,
fonts and other files are the next slice. The interface-6 `Browser` and everything of it stays until
the host activates native plugins (then it goes together, no shim: todo 90).

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
  Until downloads, a request saves at most 4 MiB (`max_file`): the cap for downloads is **to settle**
  with Kernel (see Not yet).
- The screen takes the address as the hand-over parameter `url` (interface 6: `params::URL`).
- Memory is refused, never aborted: every allocation of the plugin is fallible. Frames stay below
  2,048 B (the kit's list). The texts (en/de) are the ones of interface 6.
- `tests/hosted_construction.rs`: construction in the host's registered scope, `make` runs to its end; the host's commit is refused
  (`DeclarationViewsUnavailable`) until it wires the registry views (`BLOCKED #944`).

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
  compares each field with the interface-6 registration.

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
  (`core-net-host`). `network.md` 8 gives the browser no `max_file`: **to settle** with Kernel
  whether a download without a cap is allowed, and what the host's default is.
- Opening a kept page in the reader (`command.open` mode 1 or 3 with `page`, `ui.md`: the table of interface-6 commands): the page is a
  file in the data folder; the host must open a path of the plugin's own folder (`DOC-open-page`).
- Downloads: books go to `library.import` (`library.md` 2.3, from root 0, free name; it needs no
  `uses.storage`), fonts to the fonts folder and other files to `Downloads` (root 1, `core.files`
  move between roots 0 and 1, `uses.storage`); a book is named after its author and title with the
  document service's metadata (`DOC-metadata`) and `try_clean_name`. None of these hosts is on
  integration.
- The slot `core.browser` is offered, but the screen takes the address as a parameter (`params::URL`)
  in interface 6 and the native hand-over of a parameter to a slot's screen is **to settle** with
  Kernel (`ui.md` 1.3).
- Plugin metadata (`uses.network`, `uses.library`, `uses.storage`, `core.files` with the downloads)
  is made with the native entry, with the screens.
- The `ui::offline::OfflineOffer` and the chooser in the screen are interface-6 widgets; the native
  start screen is lists of `paperpaw_ui_kit`.

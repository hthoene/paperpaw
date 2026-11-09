# Interface-7 port (slice 1: the rules, the lists and the declarations)

Version 1.0.8. Partial: what the browser decides and keeps, and what it declares, without the host.
No native screen and no `NATIVE_ENTRY` yet; the interface-6 `Browser` and everything of it stays
until the host activates native plugins (then it goes together, no shim).

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

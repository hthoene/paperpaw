# Wikipedia lookup on interface 7 (ported, with named gaps)

Status: **native only.** The interface-6 plugin (its summary screen, provider and slot binding),
its kernel tests and its registration in the app are removed. Version 1.0.3 (Beta 3 is 1.0.2:
no further bump).

Native plugin: `src/native_plugin.rs` (`NativeWikipedia`), the screen in `src/native_view.rs`, the
wrapped text and what an input means in `src/native_page.rs`, the host-free answers in
`src/native_model.rs`, declarations in `declarations.rs` built into the image by `build.rs`,
metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`).

## What it does

- Declarations: the overlay screen `wikipedia.lookup` (title `wikipedia.title`), offered for the slot
  `core.lookup-online` (`slot_offer`; another plugin can take its place), and the subscription to
  `event.http_done` (token 3). The metadata has `uses.network` and the services `core.net`, `core.ui`; the feature `uses.wifi` is for *Go online*.
- The screen takes `word` and `language` from its hand-over (a word over 256 bytes and a language over
  35 are cut or dropped; a hand-over that cannot be read is a screen for no word, "no article"). The
  language is the interface's where none is given. It asks `core.net` for the summary of the article
  (`GET`, `User-Agent` and `Accept` headers, `max_body` 16 KiB) as soon as it opens; leaving the screen
  cancels the request. *Loading* shows meanwhile.
- The answer is read by `native_model::outcome` (status 200 to 299 with a non-blank `extract` is a summary,
  with the article's `title` as the heading; 404 or no extract is "no article"). The body is read in
  parts into one text (at most 16 KiB; a longer or not-UTF-8 body is "did not answer as expected").
  What the end of a request says: the offline power mode, a hold or network off say "needs a network
  connection"; no network, DNS, TLS, timeout or refusal say "could not be reached"; any other end says
  "did not answer as expected".
- The summary shows with its source and licence below it (CC BY-SA asks for both), wrapped by words
  into at most 600 lines over the lower 55 % of the screen. Buttons: next/previous page, up/down,
  left/right scroll a page; confirm and back close. Touch: the right part scrolls on, the left goes back
  (or closes on the first page), outside closes. A scroll key at the ends is taken (it must not reach
  the page below).
- Memory: every allocation is fallible (`NoMemory` is returned, never an abort; sweep in
  `tests/native_alloc_refusal.rs`); the font is released on every draw path.

## Not ported (each is an ignored case in `tests/acceptance_pending.rs` or a named gap)

- **The offer to go online** (done): a request the user's input started goes through in the offline mode
  (`network.md` 3.2, 2,000 ms). When it ends with the reason `offline` (the window was missed), the screen
  says so and offers it: confirm or a tap sends `command.go_online` (120 s, `uses.wifi`) and asks again,
  back closes. A plugin hold or *network off* still says "needs a network connection".
- **The provider** (declared, not submitted): the host answers a lookup provider with `ERR_UNSUPPORTED`
  until the stage `core.lookup` is live, and a provider that must fetch needs the asynchronous stage
  reply (todo 98). Until then the dictionary panel has no *Online ›* source; the summary is reached
  through the slot only. `tests/native_declarations.rs` pins the refusal; when the host admits it, the
  test fails and `make` submits `image::PROVIDER`.
- **The summary given by the dictionary's panel** (`text`): the panel does not offer one without the
  provider; the screen always fetches.
- **The line in *About*** naming the source and the licence (the stage `about.lines`).
- **Goldens**: the host draws no native screen yet (H1/H3/H4); no snapshot is blessed.
- The screen shows the page the same way the dictionary's panel does but has no "page n of m" note.

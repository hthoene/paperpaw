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
- **The provider** (done, `src/native_provider.rs`): `make` submits it; the host calls it only while
  connected. An ask is answered *pending* (title *Encyclopedia*) and the summary is fetched through
  `core.net` (one ask in flight, a new ask replaces it). When the request ends, the answer is kept
  (the whole summary, at most 16 KiB, or why there is none) and `lookup_ready` is staged with the
  ask's text and language; the asker's next ask gets the summary (cut to an entry's 2,048 bytes) with
  `opens` = `wikipedia.lookup` (*Open Wikipedia ›*). As in Beta 3, no article, a network not reached
  (every request that did not get through, or no network service) or an unexpected answer is an
  entry saying so. Scripted-host flow: `tests/native_provider_flow.rs`. Acceptance through the
  dictionary panel waits for the asker (Port A). The host does not stage `command.lookup_ready` yet
  (`ERR_UNSUPPORTED`, a step after #1387): the provider keeps its answer all the same, so the
  asker's next ask gets it; the ignored case `the_end_of_the_request_tells_the_askers` waits for it.
- **The summary given by the dictionary's panel** (done): the screen an entry opens shows the
  summary the provider kept for the word (whole), else the hand-over's `text` (at most 1,024 bytes),
  and fetches only without either.
- **The line in *About*** (done): the plugin subscribes to `about.lines` (token 4) and answers one
  `lines.line` with `wikipedia.about` (en/de, at most 512 bytes: the source and CC BY-SA 4.0). The codec
  is generated (`tools/stage-codec.py plugins/wikipedia/stages/subscribed plugins/wikipedia/src/stages.rs`,
  checked in CI) from a copy of the owner's `about.lines@1.def`; a test holds the copy to the owner's file
  (a published `@1` never changes; Kernel's convention for subscribers). Host acceptance (About listing
  it) stays with the About page's own cases.
- **Goldens**: the host draws no native screen yet (H1/H3/H4); no snapshot is blessed.
- The screen shows the page the same way the dictionary's panel does but has no "page n of m" note.

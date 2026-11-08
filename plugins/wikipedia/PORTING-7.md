# Wikipedia lookup on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (the summary screen, the provider, the slot
binding, the line in *About*) is still in place beside the native one. It is removed in the PR
that finishes the port.

Version 1.0.3. Native plugin: `src/native_plugin.rs` (`NativeWikipedia`), declarations in
`declarations.rs` built into the image by `build.rs`, the host-free parts in
`src/native_model.rs`, metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).

## Ported in slice 1

- Declarations: the screen `wikipedia.lookup` (an overlay, as the interface-6 screen drew in a
  `region`) with the interface-6 id and the title key `wikipedia.title`; the en/de texts are the
  existing ones. The lookup provider `wikipedia.summary` (online only, any language) is declared
  and built into the image too, but **not submitted**: the host answers a lookup provider with
  `ERR_UNSUPPORTED` until the stage `core.lookup` is live, and a `make` that fails would keep the
  whole plugin from being admitted. `tests/native_declarations.rs` pins that refusal; when the host
  admits providers it fails, and `make` then submits `image::PROVIDER`.
- Model (host-free, each part the interface-6 code made fallible and tested to give the same
  answers): the wiki of a language, the address of a summary and of its article (spaces as `_`,
  other bytes percent-encoded), and the summary read from an answer: a status of 200 to 299 with a
  non-blank `extract` is a summary (with the article's `title` when there is one), 404 or no
  extract is "not found", status 0 "unreachable", any other status "failed".
- The answer is read like interface 6 reads it (the members `title` and `extract` of its object),
  the texts unescaped into a fallible buffer (`paperpaw_ui_kit::json`): kept whole, refused memory
  is `NoMemory`, not an abort. A body over `MAX_BODY` (16 KiB, the request's `max_body`) is refused
  with `NoSpace`. A member that is not a string (`null`, a number) is no summary or title, as in
  interface 6.
- Tests: parity with interface 6 (ids and titles of the screen and the provider, addresses for
  eight languages and seven words, the summary for nine bodies, the statuses), the declarations
  against the host's validation, the texts in both languages, the linked entry, and a refusal
  sweep of every allocation (`tests/native_alloc_refusal.rs`).

## What the slice loses (each returns in a later slice or with its host piece)

- **Fetching and showing the summary** (the screen's draw, scrolling, the offer to go online in
  offline mode): needs the network host (`core.net`), the screen hosts (H1, H3, H4).
- **The provider** (declared, not submitted, see above) and its answer: a provider that must
  fetch is answered `ERR_UNSUPPORTED` by the host until the asynchronous stage reply exists
  (`document.md` 7). Until then Wikipedia is not listed in the user's order of providers.
- **The slot** `core.lookup-online` (a `command.bind_slot` in the plugin's first event) and the
  line in *About* naming the source and the licence (the stage `about.lines`).

Each loss is an ignored case in `tests/acceptance_pending.rs`.

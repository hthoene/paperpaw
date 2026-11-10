# Plugin store on interface 7 (slices 1 to 4, partly ported)

Status: **not complete.** The interface-6 plugin (the library's list and plugin
pages, the index kept on the card, the downloads, the install with its checks
and keys, updates, requests from other plugins, the first-run offer, the
screens) is still in place beside the native one. It is removed in the PR that
finishes the port; until then nothing is dropped.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`)

- Declarations: the two settings, the library's address (developer group) and
  *Community plugins* (system group, guarded). `build.rs` and `native/metadata*`
  are generated with `tools/abi-metadata.py`. En/de texts are the existing ones.
- The rules (`native_model.rs`, host-free): the interface-6 code made fallible,
  tested to give the same answers (`src/native_parity_tests.rs`): the groups and
  sections and the limits read from the library; the line the index keeps of a
  plugin (written and read back, tabs and line ends of the list made spaces); what
  a build needs of the firmware and whether it fits (`Needs`, `Provides`, `Fit`);
  the address of the list (`list_url`, `encode`); hexadecimal keys and signatures
  read without allocating (`paperpaw_digest::hex::decode_array`, #507); the revocations a library names, kept only if
  they could hold (of a plugin here, naming its author's key, not stopped yet),
  with a bound on the entries read (4,096) and on those kept (256).
  `tests/native_alloc_refusal.rs` refuses each allocation of these. A library is
  untrusted (community plugins too): every text is cut or refused at a limit (a line, name or summary past its cap is not read; runs and sections stop growing) named in the
  file, and nothing indexes text or bytes in a way that can panic.
- Declarations the host refuses for now are left out of the batch (none here).

## Ported in slice 2 (`src/native_item.rs`, host-free)

- The reading of a plugin of the library's JSON into an `Entry`: the keys of the
  plugin and of its `latest` version that make the index line and say whether this
  device lists it (`Item::read`: id, name, summary, author, licence, group, kind,
  waits, built_in, official, unreviewed, added, `devices`, `latest.version`, `sdk`,
  `formats`, `size`, `date`), with `cut` and `date`. Tested to give what the
  interface-6 `Item::read` gives for any path, value and device
  (`src/catalog/native_parity.rs`), with a refusal sweep and long, multi-byte and
  odd-path inputs (`tests/native_item.rs`, `tests/native_alloc_refusal.rs`).
- **Long values are cut, not refused.** As in interface 6, a text of the JSON is
  cut at its field's cap (80 bytes; the summary 300), at a character's end, and
  the plugin stays listed. Refusing at a cap applies to **index lines only**
  (`Entry::parse`: a line over 1,024 bytes, a name over 80 or a summary over 300 is
  not the store's line). The fetch must keep this split: JSON values are cut when
  read, and the lines written from them are then always within the caps.

## Ported in slice 3 (`src/native_item.rs`, `src/native_list.rs`, host-free)

- The rest of the plugin reading, in `Item`: the files of a plugin's page
  (`File`, `Digest`: SHA-256 wins over SHA-1, lower case), the build for this
  device and PaperPaw (`build_done`: device, fingerprint, firmware fit with
  `Needs`/`Provides`, families, `image_len`), the author's keys and the key
  rotations (`rotation_done`, at most 6 steps: the install's batch of 8 records; a longer chain ends with reason 6), the screenshots (`shot_done`: png
  or jpeg only, at most 4, none past 2,560 px), the plugin page's extras (`More`)
  and the fit of a plugin to this device (`Item::fits`, with what the device has
  as `Have`: a built-in or a plugin of the other kind is not replaced) and
  `finish`/`finish_parts` (the code's size counted in, PaperPaw's own mark only for
  its first-party key; the code file is moved out, not copied).
- The streaming reading with the fallible parser (`Parser::try_feed`, #414):
  `ListReader` (list: plugins, sections, revoked keys) and `PageReader` (a plugin's
  page), fed pieces of any size, memory refused is `NoMemory`. Tested to give what
  interface 6 gives for any list and page a library could send, in any piece size
  (`src/catalog/native_list_parity.rs`, `native_parity.rs`), with a refusal sweep
  (`tests/native_alloc_refusal.rs`; it found a copy of the code file, now a move)
  and hostile input (`tests/native_list.rs`).
- Caps in code: texts cut at 80 bytes (the summary 300, the description 4,000,
  an address 200, a picture's address 300); 64 files (one more kept, so that it
  shows), 6 rotation steps, 4 screenshots, 20 plugins a section, 8 + 1 families,
  4,096 revocation entries read, 256 kept. Values over 4,096 bytes are cut by the
  parser. Cut values stay (see slice 2).
- **The other keys interface 6 also reads**, each cut as it did: `author_key` and
  `recovery_key` (80 bytes), `latest.heap` (a number above 0), `latest.verified`,
  `latest.source.url` and `source` (`https://` only, 200 bytes; the version's
  source goes before the home), `repository` (`https://`, 200), `author_since`
  (date), `description` (4,000), `author_plugins` (number) and `rotations.*`
  (`to` 80, `signature` 160).
- Format-5 builds (#188, Kernel todo 169): a build's `abi` is either the
  format-4 fingerprint (16 hex digits) or `7.<needs_minor>` (`Abi::parse`:
  decimal, no sign, no leading zeros, 16 bits each, nothing else). The order is
  target (`device`) → major (below 7: built for an older PaperPaw and not offered;
  above: needs a newer one) → `needs_minor` at most the host's minor (else a newer
  PaperPaw); no draft check. **Pending Kernel draft 29:** `Native::minor` is not in
  the SDK yet, so `Provides::minor` is `None` and the minor is not checked; switch
  `Provides::of` to the field when it lands.
- **A difference from the old reading, on purpose:** `Entry::parse` refuses a
  line whose id, version, SDK, author, licence or name is over 80 bytes, or whose
  summary is over 300 (and a line over 1,024); the interface-6 `Entry::parse`
  took them. The caps are the old code's own `cut`s (`MAX_NAME`, `MAX_SUMMARY`,
  made when the library is read), so every line the store has ever written is
  within them: a **Beta 3 index holds no such name**, and a hand-made or damaged
  line is skipped (the index is a cache, rebuilt from the library). The parity
  tests (`native_parity_tests.rs`) keep the generator wide and assert "old takes
  it, native refuses" for it.

## Ported in slice 4 (`src/native_index.rs`, `src/native_sort.rs`, host-free)

- **The index on the card**, with the interface-6 files and format (`index.txt`,
  `index.bin`, `index.head` `store-index 5`, `sections.txt`, `search.bin`; a Beta 3
  index is read as it is): `build`/`take` (the list in a file of the data folder is
  read in pieces of 1 KiB through `ListReader`, written as it is read, built beside
  the kept index as `*.part` and moved into place, the head last; a file over 4 MiB,
  a text that is no list or a card that refuses leaves no index and the kept one
  stays where the new one is not whole), `Index::{kept, runs, fair, section,
  entry, search, scan, pick, orders, order}` and `hit`, `name_key` (parity-tested
  against the interface-6 key for any name). Over `paperpaw_sdk::Storage`, which
  `DataStorage` is, so the bytes need no new facade.
- `pick` keeps the plugins a view takes in an order (list, A to Z, newest, last
  updated, rank) with little held at once: written as read, the best `most` kept
  while reading (at most 512), or runs sorted on the card and merged (`native_sort`).
- Fallible throughout (`NoMemory`; an allocation sweep in
  `tests/native_index_refusal.rs`), a card that does not answer is "no index" or an
  end of the merge, no `&s[a..b]`. Search folds case with `mem::lowercase` per
  entry (a name or summary, at most 300 bytes), not one lowercase copy of a list.
- **Differences on purpose:** a card write that fails ends `build` with `Io` (the
  interface-6 `build` said only "no index"); `Index::order` falls back to the first
  order of the list's own; `Output` and the writer of the index fail with `NoMemory`
  rather than abort.
- Tests: `tests/native_index.rs` (round trip, sections, runs, search, every order
  also for 1,300 plugins across runs and the merge, refusing cards),
  `tests/native_index_refusal.rs`.

## Not yet (the install and the fetch)

- Whether a page's files make a package the install accepts (safe paths, each once,
  a `plugin.ini`, valid digests, at most 64) and that a code plugin names its
  author's key: the install's (**Kernel todo 145**).
- The addresses of files and pictures relative to the page's: the fetch joins them
  (`paperpaw_sdk::web::join_url` allocates without a refusal path; a fallible
  form is needed on the host side).
- Where the bytes come from: `core.net` (the list and the pages into files of the
  data folder, then `take` and `ListReader`); the author file and the stopped keys
  behind `Hooks`; the hand-over of the revocations to the kernel
  (`trust_revoked`, command 1582).

## Pending host (come back before activation, keeping parity)

- **The install** of plugins, data and code (the check of a code plugin whole, the
  author's key remembered at the first install and followed through signed
  rotations, the digests, the room for code, the revocations handed to the kernel,
  updates waiting for a restart). The contract is in `docs/abi/device.md` 5.1 to 5.5 (host job B2c pending: `BLOCKED 5.2-install`).
  The SDK has no typed methods yet for commands 1580 (`install_author`), 1581
  (`install_rotation`) and 1582 (`trust_revoked`); asked of Services. Owner decisions that
  apply: a plugin-triggered install or update needs the host's confirmation (132),
  and a change of the firmware always confirms (134).
- Fetching the list and the plugins' pages, the downloads, the index and its
  sorting on the card: `core.net` and the files facade (#384).
- The screens (list, plugin page, search, shots), the actions, the menu entry and
  icon, the queue and the notices, "how it works": H1/H3/H4.
- Requests from other plugins (`Command::request_plugins`) and the first-run offer
  (`core.registry` op 22, Host todo 99).
- **No downgrade**: the install must refuse a version older than the installed
  one (the interface-6 rule); it belongs with the install, so it comes with Kernel
  todo 145's design.
- The bytes of the library's JSON: the fetch (`core.net`, files facade) feeds
  `ListReader` and `PageReader`; the index writing and sorting on the card.

Each is an ignored case in `tests/acceptance_pending.rs`. The interface-6 tests
(`tests/*.rs`) stay as the references.

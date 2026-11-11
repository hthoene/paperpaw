# Plugin store on interface 7 (slices 1 to 8, partly ported)

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

## Ported in slice 6 (`src/native_page.rs`, `src/native_refusal.rs`, host-free)

- **The rows of a plugin's page** (`rows`, `actions`): install, stop, update, remove,
  restart, understood/cancel (the notice before the first code plugin), use the
  store's version, switch on, no room, needs a newer PaperPaw, not available yet, and
  what there is to read (how it works, pictures, source), as the interface-6 page
  decided them: a job or the notice above the rest, a built-in only switched on, one
  kind never over the other (`may_install`), an unbuilt build keeps what is installed
  (`unbuilt`), an update only over a plugin this store installed (`newer`, `local`,
  `offers`: betas where the device takes them, the release as the store named it) and
  a same-version build that repairs one built for another interface (`repairs`).
  Parity: `offers` and `is_pre_release` against the interface-6 functions for any
  versions (`native_parity_tests.rs`), the rest as a table (`tests/native_page.rs`).
- **The words for how an install ended** (`native_refusal::outcome`): the host's
  reasons of `device.md` 5.5 (0 to 21; unknown reads as 1) to the store's en/de texts,
  silent for the reader's own word (2, 3) and the host's stop (20), the stock-flash
  wording for *no room* (13). Nine texts are new for reasons interface 6 had no
  word for (revoked, unknown key, needs a newer PaperPaw, same version, card did not
  answer, card full, changed while installing, firmware restart first, other); every
  reason has both languages (`tests/native_page.rs`).
- **Differences on purpose (interface 7):** the room for a plugin is the host's
  check (`check_file`, reason 13), no longer worked out from the card and the flash
  here: *no room* shows once a check said it (`PageInput::no_room`); until then
  *Install* is offered, as where the room could not be told. The pre-install
  messages for a code plugin's key and signature (`trust.rs`'s `Refusal`) are the
  host's checks now, said through `outcome`. **Pending (`BLOCKED 5.2-install`):**
  who installed a version (`Origin`) and as which release (`Installed::release`): the
  host does not say, so the store keeps its own record of what it installed after an
  `install_done` of reason 0 (the `store-installs` file of `paperpaw_sdk::install`);
  until that is wired a plugin counts as put in place another way.
  **Activation blocker:** until the record is wired no update is ever offered
  (every installed plugin counts as put in place another way) and *use the store's
  version* shows for plugins the store itself installed; the store must not be
  activated before `Origin`/`release` come from the record.
- **The screen runs `check_file` first** and passes `flash_hint` from the device
  layout: `no_room` is only set after a check, so without it *Install* shows and the
  install ends in reason 13 instead of saying *no room* beforehand.
- Fallible (`NoMemory`; sweep in `tests/native_page_refusal.rs`); nothing indexes
  text; no host call.

## Ported in slice 5 (`src/native_queue.rs`, `src/native_request.rs`, host-free)

- **The queue** (*Update all*, the plugins another plugin asked for, the first-run
  setup's): `queue.txt` in the interface-6 format (`> ` the turn, `~` quiet, `&`
  asked for by a plugin; a Beta 3 queue is read and written back as it was) as
  `Queue` (`read`, `keep`, `add` each id once with a `Mark`, `interrupted`,
  `advance`: the one before goes, what is no plugin's name goes, the next gets its
  turn, `None` once empty; `requested`, `requested_waiting`, `quiet`, `has_quiet`)
  and the notes of the quiet plugins (`note`, `report`: told once, when none waits).
  What to fetch is the answer of `advance`; the page's request, the install and the
  notices are the plugin's, with their host pieces.
- **The requests of other plugins**: `request.txt` / `request-result.txt` (same
  format) as `Request` (`open`, `start`, `again`, `close`), `outcome`, `finished`
  (who asked, what is installed, what is missing; the request closes), `wanted` (each
  id once, none on the device, 32 at most, four times as many looked at), `entries`
  and `names` (the dialog names plugins by the store's listing, never by the ask),
  and `decide`: the interface-6 choices as a verdict (nothing missing, the same ask
  the reader declined within a minute, the same one still open, busy with another's
  request, or show the dialog, closing a request an unanswered dialog left).
- Fallible throughout (`NoMemory`, an allocation sweep in
  `tests/native_queue_refusal.rs`); a queue or request that cannot be read (too
  big, not text, no file) is none, as before. Differences on purpose: at most 512
  lines are read of a queue and at most 32 ids of a request (`MAX_WANTED`, as the ask is
  capped; interface 6 read them all); a card that refuses a write is `Io` for the caller
  to say (interface 6 ignored it), and no directory is made (the data folder exists).
- **The fetch must check every id** it reads from `queue.txt` or `request.txt` with
  `is_plugin_name` before it builds an address from it (`Queue::advance` drops those
  that are none, and `Request::open` keeps the ids as written: a damaged or planted
  file is not trusted).
- Still the plugin's (host pieces): the event that asks (77) and its answer (78),
  the dialog (`Ui::ask`), the fetch of a page and the download, the install, the
  first-run offer's names (`styles.rs`'s hint) and what the fetch screen shows.
- Tests: `tests/native_queue.rs`, `native_queue_refusal.rs`.

## Ported in slice 8 (`src/native_download.rs`, `src/native_install.rs`, host-free)

- **The download** (`Download::start`, `ended`, `cancel`, `take_ready`): the files of a
  page one after the other over `core.net` into `stage/<plugin>/` in the data folder,
  as they come (nothing is held in memory; the request's most is the size the library
  lists), each checked against that size and its SHA-1 or SHA-256 before the next is
  asked for, the job kept in `install.txt` / `install.done` (read in pieces of 256
  bytes, written a line at a time, hashing in pieces of 512). The rules are those of
  the interface-6 `install::Installer`: a package is valid with safe distinct paths, a
  `plugin.ini`, valid digests and at most 64 files; a start stops the download going
  on and forgets what it fetched; a file that fails ends it as `Failure::Network`,
  `Damaged` or `Card` and the files go. The plugin owns the network: it implements
  `Wire` (`get`, `cancel`) and passes each `http_done` as a `Done`.
- **The install** (`Installer::finish`, `done`): the host's job (`install_plugin`,
  commands 1576 and 1580 to 1582) with the library's word of a code plugin (`Proof`:
  author key, recovery key, the last six rotations) and `update` / `allow_older`
  from `Have`; the end comes as `install_done` (`End`) and is said by
  `native_refusal::outcome`. The files are removed at the end whatever it was. The
  library's revocations are handed to the host (`hand_over`, at most 16).
- **What the store installed** (`Installs`): `.paperpaw/plugin-origin.txt`, the
  interface-6 file and format (`<id>\t<release>`, at most four releases of a plugin,
  the newest last, the same release once), so what Beta 3 kept is read as it was;
  `adopt` keeps interface 6's rule (the plugins installed before the record count as
  the store's, once). The no-downgrade rule is the host's (reason 11 "older", 14 "the same version"; `device.md` 5.5).
  Kernel #1091's "installed by store" registry flag is to be used once it lands; the
  file stays the record until then.
- **The queue** (`native_install::next`): goes on with `Queue::advance` and keeps the
  queue on the card; `Next::FetchPage(id)` is what the plugin's loader fetches next.
- **Differences on purpose:** an install the plugin cannot start (`Full` while four
  wait, no `uses.install`) is `Failed`, not retried; a record that cannot be kept does
  not turn an installed plugin into a failure.
- Tests: the unit tests of both modules and `tests/native_install_refusal.rs`
  (every allocation refused in turn).

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

## Ported in slice 7 (`src/native_device.rs`, `src/native_load.rs`, `src/native_plugin.rs`)

- The device's facts (`native_device.rs`): its name, the code file a build gets, the
  plugins installed (sorted by id, read from the registry with a 256-byte stack
  request), the library's address and the *Community plugins* setting, read from the
  settings service. `Facts::read` returns `None` where plugin libraries do not know the
  device.
- The fetch (`native_load.rs`): a state machine over the `Wire` trait (the host's
  storage, the network request, final address and revocations), host-free and tested
  with a scripted wire. The list is saved to `list.part` and indexed in one pass; a
  plugin's page is asked and kept for one screen; a failed or offline answer is
  `Trouble::Failed`/`Offline`, never a panic. Small devices keep fewer pictures.
- `native_plugin.rs`: the entry and the `Wire` over the host. Still open (other
  slices): the screens, the downloads and installs, the requests of other plugins, the
  first-run offer.

## Ported in slice 3: the screens (`src/native_screens.rs`, `src/native_screen_*.rs`, `src/native_ask.rs`; ticket #244, assignment #2286)

Status of the plugin is unchanged: **not complete** (the interface-6 plugin stays
in place; its removal comes with the PR that finishes the port).

- The contract (`native_screens.rs`): `StoreView` (`List`, `View`, `Letters`, `Source`,
  `Plugin`, `Shots`, `Fetch`, `Offer`) with `open`, `shown`, `closed`, `resume`,
  `hidden`, `draw`, `input`, `event`, `started`, `answered`, `returned`. Each answers
  with a `Next` (open a screen, close, fetch the list or a page, install, update,
  remove, stop, *Update all*, say a text, ask, restart) that `native_plugin.rs` carries
  out. What a screen reads comes through `Ctx` (host, data folder, `Loader`,
  `Installs`); `LoaderView` makes the slice-1 `native_load::Loader` one.
- Screens, as the interface-6 ones (`list.rs`, `view.rs`, `letters.rs`, `source.rs`,
  `page.rs`, `shots.rs`, `fetch.rs`): the start with the views and things to do, a
  view (section, order, search, the letters to jump to), where the library is read
  from, a plugin's page with its pictures, the progress of a fetch (list, *Update
  all*, plugins another plugin asked for), the first-run offer.
- `native_ask.rs`: the dialog of plugins other plugins ask for (events 77 and 78),
  the first-run offer, and the offer of Literata's bold and italic, as pure functions
  of what the plugin read; the answer to the one who asked is one `PluginOutcome` for
  each id (`answer_plugin`) where interface 6 sent one event with lists.
- Declarations (`declarations::screens`, `declarations::setup`): `store.list` and
  `store.fetch` single, `store.plugin`, `store.view`, `store.letters`, `store.source`,
  `store.shots`, `store.offer` as before; the setup step `store.setup`. The test
  `native_declarations` compares the ids and titles with the interface-6
  registration.
- Memory (#248): nothing is built at the start. A screen builds its rows when it
  opens or comes on top, lists are read from the index on the card a page at a time,
  a plugin's page is held for the one screen and dropped (`forget_page`), no state is
  kept between screens. Texts are copied once; every allocation is fallible. Frames
  stay under 2,048 bytes.
- **Not run on a host.** Pure functions and the screens' layout are tested; no screen
  has run against the host yet. It runs when `native_plugin.rs` (slice 1) routes the
  screen tokens to `StoreView`, when the downloads and installs (Port E, slice 2) give
  `Installs`, and when native plugins are activated (#243). The store's start heap
  against Beta 3's 99.9 KiB is measured then (`--heap-report`).
- Open: the `Installs` adapter over Port E's steps and queue; the metadata `used` list
  (`native/metadata.json`) for any host call a screen makes that slice 1 did not
  (Port A regenerates it); the lazy `ItemList` (ui-kit has none: rows of a page of the
  index are built eagerly, as Wi-Fi's).

## Wiring of the screens and the install (`src/native_plugin.rs`, `native_jobs.rs`, `native_wire.rs`, `native_installs.rs`, `native_glue.rs`; ticket #244, assignment #2310)

- `native_plugin.rs` routes the screen tokens (20 to 27) to `StoreView`: `open`, `shown`,
  `closed`, `resume`/`hidden` (when another screen comes on top), `draw`, `input`,
  `event`, `started`, `answered` (the reader's answer to `Ui::ask`) and `returned`.
  Every `Next` a screen returns is carried out in `native_jobs.rs` (fetch of the list
  and of a page, download then install, remove, *Update all*, the queue, the notices,
  the progress screen, the restart).
- `Ctx` is made fresh for each call: `LoaderView` (the loader, its generation, small
  memory), `HostInstalls` (what the device has, read once and let go when no screen is
  open, and the activity: step, queue, waiting) and the data folder.
- The author's key travels with the page: the install keeps the page's `author_key`,
  `recovery_key` and rotations (`lean`) and `Proof::of` hands them to the host in the
  evidence of `install_plugin` (interface 6 wrote `author.txt` with `trust::expect`;
  nothing is written now). Test: `native_jobs::tests`.
- A picture the host prepared for a pictures' screen that is gone (`image_ready` with
  nobody to take it) is given back with `image_release`; the page held for the screens
  is let go (`Loader::forget_page`) when the last page screen is left.
- Metadata: `canvas.draw_image` (the pictures), the service `core.ui` (`Ui::open`,
  `Ui::ask`, the opening of screens and of the reader). `image_prepare` and
  `image_release` are commands through `commands.submit`, already listed. The `.bin`
  and inspected files are regenerated.
- **Plugins other plugins ask for** (`src/native_asks.rs`; #1132 has the host side:
  event 77, `command.answer_plugin`, event 78): the store subscribes to event 77
  (token 8) and declares `uses.install` (optional, like the other installers). The ask
  is decided by `native_ask::asked`; ids the host named and the store does not take
  are answered at once (on the device: installed, no plugin's name: failed), those the
  library lacks *not found*, a repeated or busy ask *declined*. The reader is asked
  once (`Ui::ask`, a confirmation); *Download* queues the plugins (`Mark::Requested`),
  opens the fetch screen and starts the queue; *Not now* answers *declined*; a dialog
  left without an answer tells nothing (the next ask closes it). When the queue
  ends each id is answered installed or failed (`finished_answers`), when the
  download cannot start (no network) failed. What is held between ask and answers is
  the host's request numbers with their ids, in memory: a restart drops them and the
  host fails the open ids. The network is not asked before the dialog (no call for it
  on the plugin side): a download the connection refuses ends as *failed*.
- **Review of #1141 (Port Reviewer 2, seven must-fixes), what changed:**
  - A step is synced from every end (`install_done`, `download_failed`, a stop): a
    finished install no longer leaves the *Stop* row, a failed download is no longer
    *Downloading*. The release of the page is kept for the install that outlives the page
    screen (`installing_version`).
  - The queue never waits for a page that is not coming: a page request the host
    refuses, or a page screen that took the slot for another plugin (asked again once)
    ends as *not installed* and the queue goes on (`want_failed`).
  - `finish` abandons what it staged (the folder, the busy mark, what was kept) on
    every early error (`abandon`).
  - A removal says *removed* and lets go of the record only when the host says it
    was done (reason 0); declined or dismissed nothing changes; a refusal says its reason.
  - Memory (#248): the start reads only the rows of the plugins the offer names
    (`Snapshot::read_some`), and an ask for plugins only the rows of the plugins it names;
    nothing is held. The queue the screens read is read while they are open and dropped
    with the snapshot when the last screen closes and no install goes on
    (`release_idle`); a page held for the queue or the offer is let go when the install
    ends and no page screen is open; what an install keeps of its page is the entry's
    name, version and build flags and the library's word of its author, none of the files.
  - The plugin's facts (`can_install`, `takes_betas`, the stock flash layout, the
    actions, the plugins that wait for a restart) are read with the snapshot
    (`Activity::read_facts`).
- **Not run on a host**, and the store start heap (Beta 3: 99.9 KiB) is measured only
  when native plugins are activated (#243).

### Review of #1136: what was fixed, and what the wiring must do

- The ask dialog of requested plugins names every plugin it lets through (no cut
  text); plugins that do not fit are answered *declined*, not fetched (consent).
  *Remove* on a plugin's page asks keep or remove again (`store.remove-question`).
  Every id of a request gets an outcome on every path (partial list: not found;
  closed dialog, stale request, refused queue: declined or failed; no memory:
  `Reply::AnswerAll`).
- The shots' `closed` cannot be refused: the image id and the net handle are released
  first with stack buffers. Download progress redraws one row of the page
  (`ItemList::set_value`, an additive ui-kit change) instead of rebuilding all rows.
  The offer's choice and its rows never disagree (`toggled`, `chosen_of`).
- Shots and fetch hold nothing between frames: their texts come through stack
  buffers (`copy_message`/`format_message`, 512 B message + 256 B arguments in the fetch;
  the shots read their three texts once at open). A text too long for them (a long
  list of names) is said through a `Messages` made for that one text and dropped. The
  page counter of the shots is said by `header::pager` through two stack buffers
  (96 + 128 B; an additive change in ui-kit, owner: Plugin Engineer). Strings and
  paint ops of a frame are allocated as in the other screens.
- **For the wiring (`native_plugin.rs`, Port A):** answer `Dialog.not_found` and
  `Dialog.declined` at once, handle `Reply::AnswerAll` and `Asked::stale`; route the
  Remove question's answer (`answered`, tag `REMOVE`); release the image id of an
  `ImageReady` that arrives for a closed shots screen; call `forget_page` when a page
  screen closes; the install's trust check needs the page's author key
  (`trust::expect`). The store stays out of `NATIVE_PLUGINS` until the screens are
  routed.
- Differences from Beta 3, on purpose or open: the screens read what the plugin
  passes in (`Installs`, `LoaderView`) instead of `env`; the free-space/room notes
  and the offline offer depend on the facts the wiring provides (`flash_hint`,
  `no_room`, `can_install`); the setup step's applicability and badge are pushed by
  the plugin (`set_item_state`) because they cannot be declared.

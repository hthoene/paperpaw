# Interface-7 port (slices 1 to 4: the decisions and declarations, the native entry and the card page, the file check, the online flow)

Version 1.0.7 (already above its Beta 3 version; no further bump). Status: **not complete**: the native entry
(`NativeUpdate`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin `update`) has the whole card flow
(checks, states, install, offer at start) and, since slice 4, the online flow (search, offer, download, the fetch of
code plugin builds, the weekly look and the check a rule asks for). What is still named below (the plugins an
update leaves out, an update that waits for plugins, the dual-boot notice, the badge, a built-in plugin that comes
back as a file) keeps the plugin **gated**: it is not in `NATIVE_PLUGINS` and not in
`tools/plugin-files7/complete.txt` (its native file is not the whole plugin). The interface-6 `Update` and
everything of it stays until the host activates native plugins (then it goes together, no shim; see the removal
step).

## Ported

- `src/native_update.rs`: the rest of the week before the weekly look (`wait`), the periods of which a
  check is the first for the server's counting of devices (`first`, ISO weeks across the turn of the
  year; the clock set back is the first of none) and the age of a device (`age`), and the texts that
  read as versions (`is_version`, `is_pre_release`). No allocation. `src/native_parity_tests.rs`
  compares each with the interface-6 code over every pair of days across two years, every pair of
  times, and the shapes of versions (the interface-6 `wait` and `is_pre_release` became `pub(crate)`
  for it, nothing else changed).
- `declarations.rs`: the group (order 94), the three settings (the server's address in the developers'
  group, looking by itself, betas), the page (one at a time), the actions `open` and `check`, the
  page's row first in the group, the icons, the `Firmware` need of the group and four items, and the
  first-run step (order 57, page `update.look`, only where PaperPaw updates the firmware). **One change:**
  the interface-6 step left its title and help empty, to take the setting's; the native step names them
  (`update.setting.look`, `update.setting.look.help`), the same texts. `tests/abi7_declarations.rs`
  compares the decoded step field by field (name, page and kind, order, title and help as the setting's,
  no condition, the `Firmware` need standing for the interface-6 closure).
  `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each with the
  interface-6 registration.

## Native entry and the card page (slices 2 and 3)

- `src/native_plugin.rs`: `make` submits the settings, the page, the navigation and three event
  subscriptions (`started`, `install_done`, `firmware_changed`; `declarations::events`). *Update* (the
  settings row, `update.open`) opens the page. The page reads `core.device` `firmware` (state and progress;
  not found means the device has no firmware updates: *This update cannot be installed*, the device does not
  install from here) and the files of the card's folder `updates` through `core.files` (root 1, `uses.storage`:
  `.bin`, not hidden, a name up to 64 bytes, at most 8, natural order: `0.9.0` before `0.10.0`).
- **The check (slice 3)**: for each file the plugin sends `check_file(request 2 + index, Files, "updates/<name>",
  Firmware)`, one at a time (the next goes out when `install_done` kind 3 of the last arrives; the device
  runs one check at a time and holds four requests), and reads each answer with `Device::check` (same
  request) at once, as the evidence stays only until the next check. The page shows *Checking the update
  files* meanwhile (with *Not now*); the checks go on with the page closed. `src/native_check.rs` (no host,
  tested) reads the evidence: **who signed it** (the PaperPaw flag, else the key: *own build* with its
  fingerprint, four groups kept on one line as interface 6 showed it), **newer, older or the same**
  (`Version` order of the SDK, else file-name order for a tag it does not read; the host's reason 14 wins:
  never newer then), **another firmware's image** (the foreign flag; its verdict being unsigned, unknown
  key or not newer is no failure here, any other reason is), or **unfit** with the host's reason (damaged,
  other chip, no room, revoked, unsigned, newer interface; the reason texts of interface 6). The page
  offers the newest image that fits (the first listed of equal versions, as `found::find`), else the first
  other firmware, else the last failure, else *No update on the card*. States and buttons: newer PaperPaw
  `Install` (`update.ready.*`); older *Install older version* (`update.older.*`) and the same *Install
  again* (`update.same.*`), each with `allow_older()` only because the reader pressed that button, told what
  it is; an own key *Install with this key* (`update.unknown.*`, fingerprint shown, `.key(..)` with the
  checked key, plus `allow_older()` when not newer, and the note that it is not newer); another firmware
  *Not now* first, then *Install anyway* (`update.foreign.*`, `.foreign()`).
- *Install* sends `install_firmware(request 1, Files, "updates/<name>", options)` as above; the **device checks
  the file again, always asks the reader and shows version and signer** (decisions 132 and 134), so the page
  shows *Installing the update* with the percent of `firmware_changed` (a hint read again through
  `dev_firmware`, redrawn only when the percent moves). The install belongs to the plugin, not to the page:
  closing the page does not stop it (interface 6 held *Back* while installing), and the end is handled with no
  page open.
- `install_done` (kind 1, request 1) ends it: OK writes `installing.txt` (path and **the checked version**;
  none for another firmware, whose start the plugin does not see) and sends `firmware_restart`, the page says
  *Update installed, Restarting …*; declined or dismissed (reasons 2, 3) shows the file as it was offered
  (no new check); not newer (14, which the check and the reader's choice should have made impossible) and
  every other reason is *Update not installed* with the interface-6 reason text (`native_card::reason`;
  unsigned, other, unknown or revoked keys are *signed with a key this device does not trust*); restart
  pending (21) shows *Restart now*. A page opened while an install runs or waits shows *Installing*, one
  opened with an installed image waiting shows *Restart now*.
- At start (`started`): `installing.txt` (read in 4 KiB at most, removed) says the update went in; if the
  firmware did not undo it and the named version (where there is one) is the running one, the plugin announces
  *Updated to PaperPaw {version}* (`notify` with `announce`, no screen needed) and removes the file of
  `updates` (only a plain name in `updates`; the data file is not trusted to name another path). A start that
  undid the update announces *The update did not start: PaperPaw {version} is back*. Otherwise the files of
  `updates` are checked (the firmware idle) and **a newer image, whoever signed it, is offered once**:
  `offered.txt` (path and version, the interface-6 file) is compared and written, and the page opens on what
  the check found (no second check). Formats of `installing.txt` and `offered.txt` are unchanged
  (`testdata/installing.txt.v1` is read by `native_card::read_pair`).
- `src/native_card.rs`: the decisions without the host, tested: which names are update files, the order, how
  an `install_done` reads (`ended`), the reason texts, the percent, `installing.txt`.
- Page layout: the rows (buttons) are on top and the large line and the words under them are the list's
  *empty footer* (title on one line, text up to six lines of small type), not buttons at the bottom as in
  interface 6; the beta version reads *0.5.0 Beta 1* as before. New texts (en and de):
  `update.card.*`, `update.installing.percent`, `update.installed.restart-help`, `update.restart`,
  `update.reason.unsupported`; the interface-6 texts of the states above are used as they are.
- The `uses.firmware` and `uses.storage` features are declared in `native/metadata*.json` (required): without a
  card (`uses.storage`) there is no install from `updates`. `uses.network` and `uses.install` (the online flow) are
  declared optional.
- **Left out of the card flow, not blocked:** interface 6 also looked for another firmware's image at the
  card's top level (where CrossPoint's installer looks); the native page looks in `updates` only (one more
  listing of root 1 and the same check). **To confirm with Firmware:** the check of a foreign image answers
  with reason 0, 8, 9 or 14 (the page reads these as "another firmware"); a check that names another reason
  is shown as *This update cannot be installed* with it.

## The online flow (slice 4)

Files: `src/native_online.rs` (no host, tested: the question, the streamed answer, the kept offer, the counting,
the dates, the server's address), `src/native_plugin/online.rs` (the page's search and download, declared work),
`src/native_plugin/fetch.rs` (the code plugin builds). Services: `core.net`, `core.work`, `core.registry`
(`native/metadata*.json`), features `uses.network` and `uses.install` (optional: the card flow works without them).

- **Search** (*Search online*, shown where the device has a network): a `POST` to `<server>/api/v1/updates` (setting
  `update.server`, trailing slash dropped, `https://paperpaw.org` where empty) with the interface-6 members
  `project`, `chip`, `version`, `plugins` (a crate and its version each, named `paperpaw-plugin-<id>` from
  `core.device` op 4's ids), `code` (`id version` of the code plugins installed, from `core.registry`), `betas`,
  `trigger`, `first` and `age` (the counting, `last-check.txt` and `first-check.txt` as before, same rules as
  `native_update`). **Changed from interface 6:** `abi` and `compiler` are gone (`dev_native` has no fingerprint
  or toolchain, `device.md` section 7); the question says `abi_major` and `abi_draft` of `dev_native` instead.
  **To confirm with Web:** the server's reading of those two members (it selects builds by them, `device.md`
  section 7) and that `dev_builtin.id`, a plugin id, becomes the crate `paperpaw-plugin-<id>` (a crate with
  another name, such as a third party's, is not named right). The question goes in a file (`question.json`, root
  0, sent with `send_file`: it is over the 2,048 bytes a request carries inline). The answer is read in pieces of
  3,840 bytes as the streaming parser takes them (at most 40 KiB, below what a memory body is on the X4); a
  version that is no version, not newer, a pre-release where the reader did not ask for betas and the device does
  not run one, or no address is no offer (the interface-6 rules, `native_online::Reader::finish`). The offer is
  kept in `offer.txt` (the interface-6 file; its `native` line is read and dropped).
- **Offer and download**: the page offers it with its size (`update.size.kb`, `.mb`) and *Download*, which saves
  the file into `updates/<project>-<version>.bin` with `core.net` `save_to` (root 1), `progress` (the percent on the
  page) and `max_file` the size the server named. The host writes `<path>.pp-staged` and renames it when whole,
  so a cut download never shows as an update file (interface 6's `updates/.download` is not used). The plugin
  also checks the size (`core.files` `stat`). The update's own signature is checked by the device at the card flow.
  Closing the page does not stop the download (as before); a restart cuts it off and the offer is shown again.
  There is no cancel button, as in interface 6. A failed one says so (`update.download-failed.title`) and keeps the
  offer. When it is whole the card flow takes over: the files of `updates` are checked again and the page offers
  the update.
- **Code plugins for the update** (`fetch.rs`): of the builds the answer names, those of **code plugins installed
  here** are downloaded one after another into `staging/<id>/` (root 0, `save_to`), checked against their size and
  SHA-1 or SHA-256 (read in 4 KiB pieces), and the host is asked to install each folder
  (`command.install_plugin`, replace, request 100 and up): **the host checks it and asks the reader** (decision 132;
  a trusted-installer grant skips the question). One plugin at a time; one that fails is counted and said
  (`update.fetch-failed`), never stops the update. Interface 6's queue in `fetch.txt` and its retries at later
  checks are **not** carried: a restart loses what was left, which the store gets later. `fetch::comes_back`
  has nothing to filter until `leaves_out` is native (below).
- **Weekly look and the check a rule asks for** (declared work, `core.work`): at each start, and when the setting
  `update.look` changes, `update.look-due` is declared (optional network work, after what is left of the week since
  `looked.txt`, every 7 days, at the moments online and charging) or cancelled where the setting is off; the work
  writes `looked.txt` and asks quietly (`trigger` `auto`). The action `update.check` declares `update.check-due`
  (network work within an hour, at the moment online). **Changed from interface 6:** an action cannot tell the
  reader from a rule (`ActionRun` has no such flag), so the action always waits for a cheap moment and
  nothing opens the page when it finds an update (interface 6 searched at once and opened the page where the reader
  ran it). A quiet check keeps the offer; the page shows it next time. In a run of declared work nothing is said
  (`work.md` 2.3: no notice).
- **Not needed natively:** interface 6's offer to go online before the network is used in the offline mode: a
  request started within 2,000 ms of the reader's input goes out in the offline mode (`network.md` 3.2), and the
  page's buttons are such input. Held requests end with a reason the page says (`update.net.held`).
- Page: *Search online* in the card-less state, below an older or the same PaperPaw image (as before) and after
  *Install anyway*; *Wi-Fi transfer* where transfer's action `transfer.open` exists (`core.registry` `actions`, run
  with `run_action`; back on the page after it the card is looked at again); the states *Searching*, *Up to
  date*, *No answer* (the reason, or the server's HTTP status, with *Try again*), the offer, *Downloading* (the
  percent) and the same layout as the card flow (the rows on top, the large line and the words under them).
- New texts (en and de): `update.downloading.percent`, `update.size.kb`, `update.size.mb`, `update.net.*`; the
  interface-6 texts of the states are used as they are. The interface-6 `update.offer.*` / `update.waiting.*`
  keys stay for the states still to port.

## Before activation

- `update.check` is implemented (declared work, above).
- A refused `firmware_restart` (after an install, or the *Restart now* row) shows `update.restart-failed` and keeps
  the *Restart now* row. *Installed* ("Restarting …") has an *OK* row to leave the page where the restart does not
  come.

## Blocked and next steps

1. ~~`CommandSender::check_file`~~ **Done in slice 3** (SDK #738): the card flow's found, newer, older, same,
   own-key and foreign states, the offer at start (`offered.txt`) and the version line of `installing.txt`.
   ~~The online flow, the weekly look, the Wi-Fi transfer button~~ **Done in slice 4** (above). Not yet: the update
   *file list* ordering by version (the page offers the newest only, as interface 6 did) and everything that needs
   **the plugins an update leaves out** (2).
2. **Plugins an update leaves out** (`update.leaves*`, `update.install-without`, `update.missing.*`): the contract is
   `docs/abi/device.md` 5.1 (Kernel, ships with #732): `check_file` (kind 1) on the downloaded image,
   `dev_check.flags` bit 5 says the update leaves out plugins built in now, `core.device` op 16 `leaves_out`
   pages them (`dev_builtin` rows: id and the running version); a foreign image answers `Unsupported`. The plugin
   keeps what is its own: it filters out what comes back as fetched files (`fetch::comes_back`) and words the
   choice from the server's `missing` reasons (the `missing` lines kept in `offer.txt`). **Blocked on the SDK
   wrapper:** `crates/paperpaw-sdk/src/native` has only the wire constant `OP_LEAVES_OUT` (no `Device::leaves_out`
   and no `FileCheck` flag for bit 5; checked on integration 804d7415f). Then the *Update without them* choice
   on the card page and the offer. Until then an update is offered without saying which plugins it leaves out
   (acceptance case `LEAVES-OUT`, ignored, note: op 16).
3. **The dual-boot notice and its button** (`dual.rs`, `not_dual.rs`): the firmware's note is
   `.paperpaw/boot.txt`, which root 1 refuses; `paperpaw.ini` is a card file the button rewrites before a
   restart. Needs a flag for it in `dev_firmware` or `dev_device` (Kernel/Firmware). Until then the device's own
   answer applies: no firmware updates here (`dev_firmware` not found, or an install ended with reason 19) is said
   as *This device does not install updates from here*, and the online flow does not ask where there is none.
   **Firmware to confirm** that a dual-boot setup answers so; if it does not, a dual-boot device would be offered
   an install the host must refuse.
4. **An update that waits for plugins** (`waiting.rs`, the offer's `waiting` lines and `missing ... not-ready`, the
   *Keep waiting*, *Update anyway* and *Check again* choices, the daily question at a cheap moment, the page that
   opens once after a week unseen, `waiting.txt`). **Not blocked by the host; not done in slice 4** (it needs the
   code plugins in use with their versions and switch, which `core.registry` `plugins` gives, and a page state
   with three buttons; about 250 lines). Until it is done an update the server says waits for plugins is offered
   as any update. The offer keeps the `waiting` and `missing` lines (`offer.txt`), so nothing is lost.
5. **A built-in plugin an update leaves out that comes back as a file** (the builds of plugins built in now, with
   the author key the server names for a plugin new here): the running firmware has the id built in, so
   `install_plugin` of its file is not the way, and the host has no way to put a plugin in place for the *next*
   firmware's first start (interface 6's `fetched_folder`). `install_author` (command 1580) has no SDK sender
   either. **Needs Kernel/Firmware** (a staging for the next start, or the guarantee that the next firmware
   takes plugin files installed now). Until then only builds of installed code plugins are fetched.
6. **The badge** on the page's entry (`set_item_state`) when an update waits: `docs/abi/screens-input.md` 3.1
   names `CommandSender::set_item_state` but the SDK has none (checked on integration 804d7415f), and a plugin
   cannot submit the command itself. **Blocked on the SDK wrapper (Services).** Then the badge `update.badge`
   when `offer.txt` exists, kept by the host across restarts (`ITEM_KEEP`), set when the answer is kept and cleared
   when the offer goes. Without it a quiet check's result is seen only on the page.
7. The *Not now / OK* focus rules of interface 6 (touch focus shown only after a button was used).
8. The page's **layout** as interface 6 (buttons at the bottom, the text above, a bar, a help scrolled where
   it does not fit): the list's empty footer cuts the help at six lines (the card flow's longest text is
   three; the offer's is four). A page of this kind belongs in `paperpaw-ui-kit`.

Ignored acceptance cases for these are in `tests/acceptance_pending.rs` (they name what they wait for).

## Removal step (one sweep, with the reader port and Core #944)

1. `Update` and its `Plugin` impl, `page.rs`, `found.rs`, `waiting.rs`, `online.rs`, `fetch.rs`, `count.rs`,
   `look.rs`, `dual.rs`, `not_dual.rs` (and the interface-6 tests `tests/states.rs`, `tests/page/`,
   `tests/not_dual/`), the `HOW` table kept for the manifest; `dual_boot_declared`, which
   `crates/paperpaw-x4/tests/dual_boot.rs` uses, moves to the firmware's side or stays as a function;
2. the app's registration of `Update` (`crates/paperpaw-app/src/lib.rs`);
3. list `NativeUpdate` in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` only when blocked steps 2 to 6
   are native (every user-visible function works natively).

**Part B (the removal) was not done in slice 4**, because the gaps left are not all named host pieces: an update
that waits for plugins (step 4) is not ported. If the interface-6 `Update` were removed with the gaps as they are,
these user-visible functions would **disappear** (the owner decides):

- the *Update without them* choice and the naming of the plugins an update leaves out (step 2);
- the dual-boot notice with its *This is not a dual-boot setup* and *Detect dual boot again* buttons (step 3);
- the states of an update that waits for plugins, its daily question and the page that opens once after a week
  unseen (step 4);
- plugins built in now that come back as files, the retry at later checks of a plugin that failed, the counting of
  room for code plugins (`update.no-room`) and the plugin fetch that goes on after a restart (steps 5 and the
  `fetch.txt` queue);
- the badge on the update entry (step 6);
- a check the reader runs from an action opening the page at once, and the offer to go online before searching in
  the offline mode (see the online flow);
- the other firmware's image looked for at the card's top level (left out of the card flow already).


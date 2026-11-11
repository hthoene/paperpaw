# Interface-7 port (interface 6 is removed, not yet complete; the functions that disappeared are named below)

Version 1.0.7 (already above its Beta 3 version; no further bump). Status: **native, not yet complete (the dual-boot notice is missing)**; with the functions
named under *What disappeared with interface 6* that wait for the host or are owner decisions. The plugin is
native only: the native entry (`NativeUpdate`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin
`update`) is the whole plugin. It has the card flow (checks, states, install, offer at start), the online flow
(search, offer, download, the fetch of code plugin builds, the weekly look and the check a rule asks for) and the
update that waits for plugins, the choice to update without the plugins the new firmware leaves out and the
*New* mark on the update entry. It is in `NATIVE_PLUGINS`, not yet in `tools/plugin-files7/complete.txt` (Manager:
not before the dual-boot notice is native), and the app links it and no interface-6 form.

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
- The `uses.storage` feature is declared in `native/metadata*.json` as required: without a card there is no
  install from `updates`. `uses.firmware`, `uses.network` and `uses.install` are declared optional: Beta 3 loaded
  this plugin on a desktop too, where it stayed silent (the `Firmware` need hides its menu items and setup step, and
  `started` does nothing without a firmware), so a host that has no firmware (the desktop) must still start it; the
  page, if reached, answers *This device does not install updates from here* (`update.reason.unsupported`).
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
  (`update.missing`: get it from the store after the update), never stops the update. Interface 6's queue in `fetch.txt` and its retries at later
  checks are **not** carried: a restart loses what was left, which the store gets later. interface 6's `fetch::comes_back` (the crates that come back as fetched files, filtered out of what an update
  leaves out) is gone with it.
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
  interface-6 texts of the states are used as they are.

## Update that waits for plugins (slice 5)

Files: `src/native_waiting.rs` (no host, tested: the record `waiting.txt`, what an answer says the update waits
for, when to ask again and when the page opens), `src/native_plugin/waiting.rs` (the host's part: the registry,
the work, the page's two states). Services: `core.registry`, `core.device`, `core.work` (already declared; no
change to `native/metadata*.json`). The file `waiting.txt` is interface 6's, unchanged (`docs/data-files.md`,
version 1; `testdata/waiting.txt.v1` is read and written back byte for byte).

- **What waits.** An answer naming a newer PaperPaw whose `waiting` lines name code plugins of the question it has
  no build for, or whose `missing` lines say `not-ready` for crates built in now, **waits**; each of them is kept
  only while it still holds: a code plugin installed, switched on (`core.registry` `plugins` flag bit 0) and at
  the version asked about, a crate still built in (`core.device` `builtins`, the crate `paperpaw-plugin-<id>`
  against the plugin's id). The offer stays in `offer.txt` as before (its `waiting` and `missing` lines are what
  is read); `waiting.txt` keeps the version, `since`, `checked`, the flags `asked`, `seen`, `told`, and the
  `plugin`/`crate` lines.
- **Asking again.** A start and each answer that waits declare the question as declared work (key
  `update.check-due`, network work at a cheap moment: at a start within a day, after an answer not before a day,
  within two days); a question that could not be asked or answered (a refused request, an HTTP error) is
  declared again in a day. A question asked is counted in `checked` (without a clock: `asked`, once). The
  answer is read as any answer: still waiting is kept, ready is offered as any update, none removes the record.
  Interface 6 declared the same work; it runs the quiet check (`search(quiet)`), so nothing is said.
- **The page.** *PaperPaw 0.5.0 waits for plugins* names the plugins (their names from `core.registry`, a
  first-party crate by the name of its plugin) and the crates that never come (the server says they are not in
  its library and the update does not name them); rows: *Keep waiting* (leaves; the way on), *Update anyway*,
  *Check again*. *Update anyway* shows the offer with *Update later* first and *Download anyway* second, and
  words what stops working (`update.stopping`, the code plugins) and what is gone after the update
  (`update.leaves.anyway`: every plugin built in now that the answer's crate list lacks, as *Plugins an update
  leaves out* below says; where the answer lists no crates, the crates it names as not ready and those that never
  come). *Download anyway*
  downloads as any download. Where nothing holds any more the offer is shown as any offer.
- **Seen, and a week unseen.** Showing the waiting state records `seen` (nothing opens it by itself after).
  At a start, an update that waits and has not been seen or told for a week (`since`, counted from the first
  answer; the clock set back counts from now) opens the page once and records `told`; never again for that
  version. A start that finds an offer with no `waiting.txt` (kept before the file was written) records it. A
  start that finds the update installed or no longer newer removes `offer.txt` and `waiting.txt`; one that finds
  nothing that holds any more removes `waiting.txt`.
- **Changed from interface 6:** a finished download (the update is in `updates`) removes `waiting.txt` and
  cancels the declared question, so that an update the reader chose to take anyway is not asked about again (in
  interface 6 the record stayed until the next start). The check the action `update.check` asks for never opens
  the page (no flag tells a rule from the reader, above), so an update that waits shows only on the page, as one
  that does not.

## Plugins an update leaves out and the New mark (slice 6)

Files: `src/native_leaves.rs` (no host, tested, refused allocations too: which built-ins the answer's crate list
lacks, the server's reasons), `src/native_plugin/leaves.rs` (the host's part: the device's list, the names from
`core.registry`, the mark). Services and features are as before (`core.device`, `core.registry`;
`command.set_item_state` is a command of `commands.submit`): no change to `native/metadata*.json`. New texts: none;
the interface-6 ones are used as they are (`update.leaves`, `update.leaves.download`, `update.leaves.anyway`,
`update.install-without`, `update.download-without`, `update.missing.unknown`, `update.missing.not-ready`,
`update.badge`).

- **An update on the card.** After each `check_file` of an update file that is a PaperPaw image the plugin reads
  `Device::leaves_out` at once (the list is the last check's only, as the evidence is, `device.md` 5.1; each file is
  checked in turn, so each file's list is read before the next check goes out) and keeps the names (the registry's
  name of the plugin, else its id) in the `Image`. The page then says *These plugins cannot come along: ... Update
  without them, or wait until they are ready* (`update.leaves`, after the state's words) and the button reads *Update
  without them* (`update.install-without`; going back, the same again and an own build keep their words, and the
  sentence names the plugins); *Not now* is the wait. A foreign image is not asked (`Unsupported`). Errors of the
  op-16 read, as Kernel ruled on `device.md` 5.1 (`native_leaves::read_all`, tested without the host): op 6 (`check_file` result) and op 16 answer ONLY for the plugin's most recent firmware check, a newer
  check replaces both results, and while a check runs op 16 answers `ERR_BUSY`. So one file at a time: `check_file`,
  wait for its `install_done` (kind 3), read op 6, then op 16. If op 16 answers `ERR_BUSY` a check is running: the file stays in the scan (the page shows *Checking*), the plugin
  waits for the running check to end and then SUBMITS `check_file` AGAIN for that file and reads again. If it answers
  `ERR_NOT_FOUND` a newer check replaced the result and, since the plugin checks one file at a time, no check of the
  plugin runs and no end event will come: the plugin SUBMITS `check_file` AGAIN AT ONCE (same request id) and waits for
  nothing else, as waiting would hang the file. The old request is never read again. The file is "cannot be
  installed" only when the re-submitted check itself refuses the image, or after 3 re-submits of either kind (then
  unfit; `native_leaves::Recheck`, tested without the host). `ERR_STATE` (a cursor of an earlier
  check) starts the read again at page 0 with `generation` 0, at most twice, then the file is unfit.
  `ERR_UNSUPPORTED` (a PaperPaw image should not get it) and any
  other error: the file is *not offered* (*This update cannot be installed*) rather than offered with plugins
  dropped without a word. `ERR_NO_MEMORY` keeps its retry path.
  **To confirm with Kernel:** the plugin calls op
  16 for every PaperPaw image, not only where `dev_check.flags` bit 5 is set, because the SDK's `FileCheck` has no
  accessor for bit 5 (an image that leaves nothing out answers no rows); one more service call per file.
  TODO: call op 16 only when `FileCheck::leaves_out()` (dev_check.flags bit 5, draft-36 carrier) says so, once it is on integration
  Settled (was Open, Port Reviewer 2): the rule above, that op 6 and op 16 answer only for the plugin's most recent
  check and a stale read is answered by a re-submit, is Kernel's ruling; Kernel will add it to `device.md` 5.1
  (todo 444).
- **An update the server named, not yet downloaded.** The answer lists the crates it has built in (`plugins`); what
  is built in now (`core.device` `builtins`) and not among them is left out. The page names them, each with the
  server's reason where it says one (*not in the plugin library*, *not ready yet*; the `missing` lines),
  *Download without them* (`update.download-without`) replaces *Download*, and the sentence
  (`update.leaves.download`) comes first. An answer that lists no crates says nothing (the device names them
  after the download). Interface 6 also left out of the list the plugins that come back as fetched files: not
  carried (above, 5), so every plugin the update lacks is named. Once downloaded the card flow's list above is
  the one the reader sees.
- **Waiting, then anyway.** *Update anyway* names every plugin the answer's crate list lacks (the server's own
  `not-ready` and `unknown` lines were the whole list before), so a plugin without a reason is not dropped silently.
- **The New mark.** `CommandSender::set_item_state("open", keep + badge update.badge)` on the page's entry in the
  settings (the action `update.open`, first row of the group) while `offer.txt` exists; the default state when it
  does not. Set when an answer is kept, removed when the offer goes (an answer with no newer update, a finished
  download) and checked at each start (the host keeps the last state across restarts). A refusal (no host that
  shows item states) is no failure of the update.
- Not done: **the dual-boot notice** (*Blocked and next steps* 3). `tests/acceptance_pending.rs` keeps `DUAL-BOOT-FLAG` and
  `FILE-FOR-NEXT-FIRMWARE` only; `LEAVES-OUT` and `ITEM-STATE` are gone.

## Before activation

- Not measured: the heap peak of the op-16 (`leaves_out`) reads, which run for every PaperPaw image on the card and
  hold one page of rows each (X4 gate stays open until the device test).
- `update.check` is implemented (declared work, above).
- A refused `firmware_restart` (after an install, or the *Restart now* row) shows `update.restart-failed` and keeps
  the *Restart now* row. *Installed* ("Restarting …") has an *OK* row to leave the page where the restart does not
  come.

## Blocked and next steps

1. ~~`CommandSender::check_file`~~ **Done in slice 3** (SDK #738): the card flow's found, newer, older, same,
   own-key and foreign states, the offer at start (`offered.txt`) and the version line of `installing.txt`.
   ~~The online flow, the weekly look, the Wi-Fi transfer button~~ **Done in slice 4** (above). Not yet: the update
   *file list* ordering by version (the page offers the newest only, as interface 6 did).
2. ~~**Plugins an update leaves out**~~ **Done in slice 6** (*Plugins an update leaves out*, above).
3. **The dual-boot notice and its button** (`dual.rs`, `not_dual.rs`): the firmware's note is
   `.paperpaw/boot.txt`, which root 1 refuses; `paperpaw.ini` is a card file the button rewrites before a
   restart. Needs a flag for it in `dev_firmware` or `dev_device` (Kernel/Firmware). Until then the device's own
   answer applies: no firmware updates here (`dev_firmware` not found, or an install ended with reason 19) is said
   as *This device does not install updates from here*, and the online flow does not ask where there is none.
   **Firmware to confirm** that a dual-boot setup answers so; if it does not, a dual-boot device would be offered
   an install the host must refuse.
4. ~~**An update that waits for plugins**~~ **Done in slice 5** (above): the offer's `waiting` lines and
   `missing ... not-ready`, *Keep waiting*, *Update anyway*, *Check again*, the daily question at a cheap moment,
   the page that opens once after a week unseen, `waiting.txt`.
5. **A built-in plugin an update leaves out that comes back as a file** (the builds of plugins built in now, with
   the author key the server names for a plugin new here): the running firmware has the id built in, so
   `install_plugin` of its file is not the way, and the host has no way to put a plugin in place for the *next*
   firmware's first start (interface 6's `fetched_folder`). `install_author` (command 1580) has no SDK sender
   either. **Needs Kernel/Firmware** (a staging for the next start, or the guarantee that the next firmware
   takes plugin files installed now). Until then only builds of installed code plugins are fetched.
6. ~~**The badge** on the page's entry~~ **Done in slice 6** (*The New mark* in the same section, above).
7. The *Not now / OK* focus rules of interface 6 (touch focus shown only after a button was used).
8. The page's **layout** as interface 6 (buttons at the bottom, the text above, a bar, a help scrolled where
   it does not fit): the list's empty footer cuts the help at six lines (the card flow's longest text is
   three; the offer's is four, and with the plugins it leaves out up to seven: their sentence comes first there, so
   the size is what goes). A page of this kind belongs in `paperpaw-ui-kit`.

Ignored acceptance cases for these are in `tests/acceptance_pending.rs` (they name what they wait for).

## Removed (interface 6)

`Update` and its `Plugin` impl (manifest, registration, the action handlers, the `HOW` table; the catalogs carry
the name, the description and `update.plugin.how`, the text of `how/`, which stays in the crate for the store's
page), the interface-6 page (`page.rs`, the offline-mode offer to go online), and `online.rs`, `fetch.rs`,
`count.rs`, `look.rs`, `waiting.rs`, `dual.rs`, `not_dual.rs` and `found.rs`, which the native side never used (it
has `native_online`, `native_update`, `native_waiting`, `native_check` and `native_plugin/`): the kernel's
`HttpRequest` flow, the queue `fetch.txt` and its retries, `found::find` over a `Storage`, `dual::declared`, the
dual-boot question. The texts only they used are gone from the catalogs (`update.fetch-failed`, `update.no-room`,
`update.gave-up`, `update.downloaded.short`, `update.nothing.help`, two `update.reason.*`); those of the functions that
wait for the host (the dual-boot notice) stay. The interface-6 test suites that drove them on a kernel
(`tests/states.rs`, `tests/page/`, `tests/not_dual/`) are gone, and so is the parity of the declarations with the
interface-6 registration: `tests/abi7_declarations.rs` pins the declarations by what they say and by a reviewed
golden (`testdata/declarations.bin.v1`, `PAPERPAW_BLESS=1` writes it anew). The pure rules those tests also pinned
are `native_update` (the weekly wait, the periods of the counting over years, the age, the versions: now tests of
their own), `native_online` (the answer, including hostile answers), `native_card`, `native_check` and
`native_waiting`; `installing.txt`, `offered.txt` and `waiting.txt` of version 1 are read
(`testdata/*.v1`). The data files interface 6 wrote and no one reads now are in `docs/data-files.md`.

Outside the crate: the app no longer registers `Update` (`crates/paperpaw-app`; `NativeUpdate` is the 14th of
`NATIVE_PLUGINS`, the crate's `linked` feature is in `native-linked`, and `update` leaves `NETWORK_SETTINGS`, the
interface-6 networked plugins); `paperpaw-x4`'s tests no longer register it (`tests/update_path.rs` drives the
firmware's install through `Updates` as the plugin did, `tests/dual_boot.rs` no longer reads the note with the
plugin: `paperpaw_x4::dual_boot::noted` is the one reader); the desktop snapshot `x4-update` is gone and the
settings snapshots follow (no *Updates* row in the interface-6 programs, which do not run native plugins).

## What disappeared with interface 6 (the owner decides)

If these are wanted back they need the host pieces named in *Blocked and next steps* (3 and 5) or a decision.

1. **The dual-boot notice and its buttons** (*This is not a dual-boot setup*, *Detect dual boot again*; no firmware
   flag in `dev_firmware`/`dev_device`): the plugin no longer reads `.paperpaw/boot.txt` or rewrites `paperpaw.ini`;
   `dual::declared` is gone (the firmware's `dual_boot::noted` is the reader). The device's own answer applies: no
   firmware updates here is said as *This device does not install updates from here*, and the install of the
   firmware itself refuses beside another firmware (`Updates::begin`).
2. **A built-in plugin an update leaves out that comes back as a file** (the builds of plugins built in now, with the
   author key the server names for a plugin new here; `install_author` has no SDK sender and the host has no staging
   for the next firmware's first start), and with it `plugins/.<id>.next/`, `fetch-native.txt`.
3. **The queue of code plugins after a restart**: `fetch.txt`, the retries at later checks (`fetch-retry.txt`,
   three tries, `update.gave-up.*`), the counting of room (`update.no-room`) and the fetch that went on after a
   restart: a restart now loses what was left, which the store gets later (`update.missing`).
4. **A check the reader runs from an action opens the page at once** and the **offer to go online** before the
   network is used in the offline mode (the page's buttons are the reader's input, which the network rules take
   as such, `network.md` 3.2): the action `update.check` always asks at a cheap moment and nothing opens the page.
5. **Another firmware's image at the card's top level** (CrossPoint's installer folder): the native page looks in
   `updates` only.
6. **The firmware install's host side (named B2)** is not part of this crate and no test here proves it with the
   native plugin: the plugin sends `install_firmware` and `firmware_restart` and reads the device's answer
   (decisions 132 and 134: the device checks again and asks the reader), but what ran the whole path on a kernel
   with the interface-6 page was `crates/paperpaw-x4/tests/update_path.rs`, which now drives `Updates` (the
   firmware's install, trial and undo) directly, as the page did. Also changed: the interface-6 page held *Back*
   while installing; the native page does not, closing it does not stop the install.
7. **The interface-6 page's layout and focus rules** (buttons at the bottom, a bar, a scrolled help, the touch
   focus shown after a button was used): the native page is a list with the large line and the words under it
   (*Blocked and next steps* 7 and 8).


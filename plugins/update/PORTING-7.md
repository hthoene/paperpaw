# Interface-7 port (slices 1 to 3: the decisions and declarations, the native entry and the card page, the file check)

Version 1.0.7 (already above its Beta 3 version; no further bump). Status: **not complete**: the native entry
(`NativeUpdate`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin `update`) has the whole card flow
(checks, states, install, offer at start), but the plugins an update leaves out, the online flow and the
dual-boot notice are named blocked steps below, so the plugin is **gated**: it is not in `NATIVE_PLUGINS` and not in
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
  card (`uses.storage`) there is no install from `updates`. The plugin does not use the network yet, so it does
  not declare `uses.network`.
- **Left out of the card flow, not blocked:** interface 6 also looked for another firmware's image at the
  card's top level (where CrossPoint's installer looks); the native page looks in `updates` only (one more
  listing of root 1 and the same check). **To confirm with Firmware:** the check of a foreign image answers
  with reason 0, 8, 9 or 14 (the page reads these as "another firmware"); a check that names another reason
  is shown as *This update cannot be installed* with it.

## Blocked and next steps

1. ~~`CommandSender::check_file`~~ **Done in slice 3** (SDK #738): the card flow's found, newer, older, same,
   own-key and foreign states, the offer at start (`offered.txt`) and the version line of `installing.txt`.
   Not yet: the update *file list* ordering by version (the page offers the newest only, as interface 6
   did) and everything that needs **the plugins an update leaves out** (2).
2. **Plugins an update leaves out** (`update.leaves*`, `update.install-without`): the contract is
   `docs/abi/device.md` 5.1 (Kernel, ships with #732): `check_file` (kind 1) on the downloaded image,
   `dev_check.flags` bit 5 says the update leaves out plugins built in now, `core.device` op 16 `leaves_out`
   pages them (`dev_builtin` rows: id and the running version); a foreign image answers `Unsupported`. The plugin
   keeps what is its own: it filters out what comes back as fetched files (`fetch::comes_back`) and words the
   choice from the server's `missing` reasons. **Blocked until** Kernel adds op 16 to the schema and Services
   writes the SDK wrapper; then the *Update without them* choice. Until then an update is offered without saying
   which plugins it leaves out (acceptance case `LEAVES-OUT`, ignored, note: op 16).
3. **The dual-boot notice and its button** (`dual.rs`, `not_dual.rs`): the firmware's note is
   `.paperpaw/boot.txt`, which root 1 refuses; `paperpaw.ini` is a card file the button rewrites before a
   restart. Needs a flag for it in `dev_firmware` or `dev_device` (Kernel/Firmware). Until then the device's own
   answer applies: no firmware updates here (`dev_firmware` not found, or an install ended with reason 19) is said
   as *This device does not install updates from here*. **Firmware to confirm** that a dual-boot setup answers so;
   if it does not, a dual-boot device would be offered an install the host must refuse.
4. **The online flow**: *Search online* (`update.check`; the action answers `Unsupported`), the offer, the
   download into `updates`, the plugin fetch (`fetch.rs`), waiting for plugins (`waiting.rs`), the periods
   count (`count.rs`, rules in `native_update.rs`) and the answers while the page is closed. Needs `core.net`
   (SDK has `Net::request`), `core.files`, work declarations and `uses.network`; blocked on the check for the
   plugins an update leaves out (2).
5. **The weekly look** as declared work (`Work::network(..).optional()..at_moments(..)`, `update.look-due`,
   `update.check-due`, `setting.changed`): no native work declaration yet.
6. **The badge** on the page's entry (`set_item_state`) and the offline-mode offer before the network.
7. **The *Wi-Fi transfer* button** (running another plugin's action) and the *Not now / OK* focus rules of
   interface 6 (touch focus shown only after a button was used).
8. The page's **layout** as interface 6 (buttons at the bottom, the text above, a bar, a help scrolled where
   it does not fit): the list's empty footer cuts the help at six lines (the card flow's longest text is
   three). A page of this kind belongs in `paperpaw-ui-kit`.

Ignored acceptance cases for these are in `tests/acceptance_pending.rs` (they name what they wait for).

## Removal step (one sweep, with the reader port and Core #944)

1. `Update` and its `Plugin` impl, `page.rs`, `found.rs`, `waiting.rs`, `online.rs`, `fetch.rs`, `count.rs`,
   `look.rs`, `dual.rs`, `not_dual.rs` (and the interface-6 tests `tests/states.rs`, `tests/page/`,
   `tests/not_dual/`), the `HOW` table kept for the manifest; `dual_boot_declared`, which
   `crates/paperpaw-x4/tests/dual_boot.rs` uses, moves to the firmware's side or stays as a function;
2. the app's registration of `Update` (`crates/paperpaw-app/src/lib.rs`);
3. list `NativeUpdate` in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` only when blocked steps 1 to 7
   are native (every user-visible function works natively).


# Device plan: direct update and rollback from Beta 1, 2 and 3 (#229)

For the Device Tester. **Not run, not authorised by this file**: it needs the
owner's go for each device (the X4 on esp-host:4001; the X4 Pro on 4000 only in
the one run before Beta 4 and only with the owner's explicit OK) and a signed
Beta 4 candidate from the owner (nothing is signed or published by QA). Read
`docs/testing/update-rollback.md` first: it says what the host already proved
and so what is left for the device.

Device rules that hold here (project notes): every USB device is a test
device, **no backups**; read the chip id before every flash; never switch a
device off; the origin images are the released ones (`sleep` and switch-off
left as released: the released firmware has no `remote` feature, see below).

## Why this is not a `.pps` script

`tools/x4-remote.py` and the `.pps` steps need the `remote` feature, which
released firmware leaves out. The origin (Beta N, as released) and the signed
Beta 4 candidate have no remote. So: the card is read on a computer
(`card-manifest.py`), the screen is read by eye (photo of each message in the
table), the log by the serial monitor (`espflash monitor`, read only), the
images are flashed with `esptool`/`espflash` or the web flasher. Once a
**developer** Beta 4 build with `remote` runs (after the update, never as the
update's source: its signature is not the release key), `tools/pps/beta4/*`
can be used as usual for the rest.

## Material (one set per row)

| Item | Where |
| --- | --- |
| Origin full-flash, install, update images, X4 and Pro, Beta 1/2/3 | the releases `v0.6.0-beta.N` (`paperpaw-<device>-0.6.0-beta.N.bin`, `-install.bin`, `-update.bin`, with `.sha256`); `python3 tools/qualify-release-origins.py --download` fetches and hash-checks them |
| Target | the signed Beta 4 candidate: `-update.bin` (plain), `-update-packed.bin`, full flash (owner provides; record its commit and SHA-256) |
| Card state | `crates/paperpaw-core/testdata/beta-upgrade/` (settings, scoped settings, last book, old progress) plus the books of the owner's test card, copied onto an expendable SD card; add by hand: a highlight and a bookmark in a book, a Wi-Fi password (Settings, Passwords and keys), one store plugin with code |
| Before/after | `python3 tools/pps/update-rollback/card-manifest.py snapshot /mnt/card > before.json` with the card in a computer, and `... diff before.json after.json` |

## Rows

Run each row for X4 and Pro separately (Pro: only with the owner's OK), origins
Beta 1, 2, 3. Record per row: origin and target SHA-256, chip id, running slot,
operator, date, photos, log, manifests, verdict. A failed step stops the row:
keep the logs and the card as they are (no reflash loop).

### U1 Flash Beta N to Beta 4 (flashing: the only way on the small CrossPoint layout)

1. Flash the origin's full image. Boot, set up as in "Card state". Power-cycle once (not off).
2. `snapshot` the card: `before.json`. Note the screen of Home, Settings > Updates, Plugins.
3. Read the chip id, flash the Beta 4 candidate full image (writes the new partition table: app slots 7.9 MB). Boot.
4. Expect: it starts; Home shows the last book (Continue); the book opens at the place left; the highlight is shown; the language and settings are those set; the Wi-Fi network is listed.
5. Plugins: the store plugin with code (interface 6) is **listed as needing an update or a newer build**, not deleted; its folder is intact on the card; the store offers a compatible build when online (or says none: then the row records "clean recovery without a store build").
6. Take the card out, `snapshot` again, `diff`: nothing LOST. The expected moves are listed by the tool.
7. Update again to a higher test version (the owner's second candidate) from the Beta 4 running slot: online or by SD `update.bin`; trial, kept, restart (that is `candidate -> higher-test-version`).

### U2 In-app update from Beta N (must refuse safely)

1. On the origin, point `update.server` at the controlled server (`paperpaw-notes/scripts/serve-hardware-ota.py`, adapted: its arming guards name beta.8/9 and must not be bypassed unreviewed) that answers as paperpaw.org does (the real server, Decision 272: a device that sends no `abi_major` is offered the interface-7 release's plain file, never the packed one, with no `waiting`, `missing` or `code`; `paperpaw.org/src/updates.rs`).
2. Update page: Search online. Expect: PaperPaw 0.6.0-beta.4 is offered with its size (plain file), maybe with a note of plugins that cannot come along (hyphenation, Wikipedia: no longer built in); no wait for plugins. Download, install, restart into Beta 4 on trial, kept.
3. Put the signed `-update.bin` of Beta 4 on the card by hand and open the update page. What the host says (#402, `docs/testing/update-rollback.md`): Beta 3's own installer **installs** the plain interface-7 update (CI `update-path` runs Beta 3's tests on this build's file). So on the device expect: the update page names the file, installs, the device restarts into Beta 4 on trial and keeps it. Record exactly what is seen, then continue as U1 steps 4 to 6. A refusal with the old firmware still running is a finding (host and device differ), not a pass.

### R1 Rollback by flashing Beta N back over Beta 4

1. After U1 step 7 use the Beta 4 card a while (open the book, add a highlight, change a setting, connect Wi-Fi: this makes Beta 4's files).
2. `snapshot` (`b4.json`). Read the chip id. Flash the origin's full Beta N image (its own table: app slots 0x7f0000). Boot.
3. Expect (host-proved, now seen): starts; the book opens at the place left (the position files are shared); settings are read, a save keeps Beta 4's unknown sections; **degrades, expected**: the highlight made or converted in Beta 4 is not shown (annotations version 3), the Wi-Fi password written by Beta 4 (sealed) must be entered again, a plugin of interface 7 is not listed (a warning says it is for a later PaperPaw).
4. Card out, `snapshot`, `diff b4.json after.json`: nothing LOST (annotations and secrets files unchanged byte for byte).
5. Update to Beta 4 again (U1 step 3): the highlight is back, the plugin is back, the Wi-Fi password is still sealed and usable (if the same device), settings as left.

### R2 Trial failure rolls back (OTA slots)

Only where Beta 4 itself runs from an OTA slot (after U1 step 7). A failing
candidate on trial (the owner's test image that panics at start: "failed trial")
returns to the previous slot by itself; the previous slot is the Beta 4 first
candidate. Then another update still works. Also the **durable mark** failure
(card removed during the keep): record by the owner's separate session, per
`paperpaw-notes/ops/update-soak-qualification-2026-11-05.md`.

### R3 Interrupted download / card out

Cut the Wi-Fi or take the card out during the download of the higher test
version: the old firmware still runs, no partial file is taken as an update
(`updates/.download` is let go), the next try works.

## What the host already showed (do not spend device time on it)

Current kernel starts on the Beta 1/2/3 fixtures and keeps them; Beta 1/2/3
code on a Beta 4 card loses nothing (`tools/qualify-update-paths.sh`); the
released images' layout, signature and OTA transitions parse with the current
code (`tools/qualify-release-origins.py`); signed SD update, trial, kept and
failed trial on the model of the updater (`cargo test -p paperpaw-x4 --test update_path`).
What no host check can show: the **released old bootloader and installer
running** the Beta 4 image or a flash over it, real flash timing, the card
swap, the screens' wording, the plugin folder of a real store install, and the
Pro. Those are the rows above.

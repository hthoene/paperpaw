# Interface-7 port (slices 1 to 6, linked into the app)

Version 1.0.4. **Status: complete, native only** (ticket #244): the native plugin (`native_plugin::NativeWifi`,
`NATIVE_ENTRY`, feature `linked`) is the whole Wi-Fi plugin and is linked into the app
(`paperpaw_app::NATIVE_PLUGINS`, under `native-linked`). The interface-6 `Wifi` is **removed** (`src/lib.rs`'s
`Plugin` impl with its settings, screens, status item and registrations, `networks.rs`, `page.rs`, `phone.rs`,
`policy.rs`, the HOW texts `how/{en,de}.txt`, the registration of `Wifi` in `paperpaw_app::install_with`, and the
tests that ran it on a kernel: `tests/{radio,networks,phone,native_phone_parity}.rs`, the old-vs-new comparisons in
`native_policy/tests.rs`, `native_ini` and `native_phone/tests.rs`). **This PR is HELD until native activation
(#243):** until the host activates native plugins the interface-6 app (without `native-linked`) has no Wi-Fi page and
no radio policy. The plugin joins `tools/plugin-files7/complete.txt`. `WHILE_CHARGING` stays as the key the X4's
firmware sets the default of (`shells/xteink`).

What was pinned by the removed tests is now held by: `native_policy/tests.rs` (the policy's own cases),
`native_radio/tests.rs` (the five scenarios of the old `tests/radio.rs` on a simulated host), `native_phone/tests.rs`
(the four scenarios of the old `tests/phone.rs`; the form and the `wifi.ini` readings run on the old code's samples,
written out), `tests/scripted_*.rs` (the screens and the radio on the scripted host), `tests/abi7_declarations.rs`
(every declaration, as the interface-6 registration declared it, and admitted by the host's rules). The byte-for-byte
comparison of the served pages with the interface-6 screen (`tests/native_phone_parity.rs`) was made while that code
existed and is not repeated; the random old-vs-new policy runs (200 runs of 40 events) likewise.

## Ported

- `src/native_policy.rs`: when the radio is on and which network is joined (the module
  documentation of `policy.rs` says how), on inputs of the plugin's own (`Radio`, `Net`, `Known`)
  and outputs `Step::{On, Scan, Join}` that the native screen turns into `CommandSender::wifi_on`,
  `wifi_scan` and `wifi_join` (`uses.wifi`, a `request` number, flag 0 or "remember"). A test drives
  this and the interface-6 policy with the same random events (200 runs of 40 events) and
  compares what they answer and the state they keep. Allocation is fallible and swept; a password
  is wiped when dropped and never printed.
- `src/native_radio.rs`: what the plugin does with the radio's events, on facts the native screen reads
  first (`Facts`: the radio, busy, whether work waits, the networks in reach, the networks joined
  before, the clock, charging, home or a book shown, the settings) and answering `Action`s (a radio
  `Step`, off, a `Timer` to set (idle off, retry, at the policy's generation), a notice, no network,
  the picked network to remember, the password to keep): `network_wanted`, `wifi_changed`,
  `wifi_scanned`, `wifi_done` (a failure; a join another request superseded changes and says
  nothing), the typed password, `network_idle`, the power mode, the screen shown, charging, and the
  two timers. The five scenarios of the interface-6 `tests/radio.rs` run on a simulated host (radio
  doing as told, timers each second) with the same results; allocation is swept.
  `Failure::from(WifiFailure)` maps the SDK's reasons.
- Differences: the clock is the host's milliseconds; a join that another request superseded
  (`wifi_done` reason 4) changes nothing, the replacement decides (interface 6 had no such
  failure); `wifi_done` reasons 5, 6 and unknown count as other.
- `src/native_phone.rs` (slice 2): the phone setup's decisions without the host (`Phone`: searching,
  the own network opening and open, the page served, the network chosen on the phone and left for,
  joining, joined, failed; the two steps and the one-time move to the page when a phone joins; the
  ten minutes of idle time; what the radio keeps for the user) as a state fed what happened that
  answers a list of `Effect`s (radio on, scan, host, serve, respond with a page, choose a network,
  wake, back), and `native_phone/page.rs` (the form read fallibly, the passwords typed wiped, and
  the choose, joining and not-found pages). The four scenarios of `tests/phone.rs` run on it, the
  form is compared with the old functions on the same text, and `tests/native_phone_parity.rs`
  compares every page byte for byte with what the interface-6 screen serves on the real kernel.
  Allocation is swept; a refused one leaves the stage as it was (the name is copied before the
  password moves on). The form is at most 1,024 bytes (a longer body counts as none, as before).
  **Gate for the screen (`NET-serve`):** the body of a request can hold the password, so the screen
  wipes it (the body file or buffer, and the text read from it) once `Phone::request` returns; the
  module wipes only what it copies.
  Not ported: the own network's password (`core.secrets` `random`, PR 367) and
  the code to join it, which the native screen draws.
- `declarations.rs`: the group (order 85, its page), the four settings, the three screens (single
  instance), the action and the status item that open the page, the icons, the `Wifi` needs of the
  settings (hidden where PaperPaw does not manage the radio) and the first-run step (applies only
  with `Wifi`). `tests/abi7_declarations.rs` compares each with the interface-6 registration.

- `src/native_own.rs` (slice 3): the device's own network for the phone: the readable password
  (`readable`: 15 letters of the 18, each as likely, bytes that would favour one are drawn again), the
  page's token (`token`: 5 bits a letter), the groups of five shown (`grouped`), the QR text that
  joins it (`join_code`, the signs `\ ; , : "` escaped) and the name it opens (`ssid`, 32 bytes, `PaperPaw`
  where none is left). Same rules and answers as the SDK's `Random`, `wifi::join_code` and
  `device_name::ssid` (compared on the same byte streams and at random). The bytes come from a `draw`
  the screen backs with `core.secrets` `random` (up to 256 bytes a call, `strong` when the source was
  fully random; **no weaker draw is substituted**; a weak draw (`strong == false`) never becomes the own network's
  password or the page's token: the screen takes the text only through `Drawn::into_strong`, which
  wipes a weak one and gives `None`; the own network's password is best drawn once the radio is on,
  `secrets.md`); the bytes and the
  intermediate text are wiped, the result is a `Password` (wiped when dropped, never printed).
- `src/native_known.rs` (slice 3): the networks joined before as secrets of the plugin's own: one for
  each, named by the network's name (interface 6: `wifi.<name>`; the own name has no prefix,
  `secrets.md` 7, and `SecretName` takes tabs and line breaks), labelled `Wi-Fi <name>`, the value the
  password (empty for an open network), changeable in the list of passwords and keys.
  `Remember::new` checks what a network can be (name 1 to 32 bytes, password up to 63) before anything
  is sent; `ssids_of` and `known_with` make the policy's list from the names and a password asked for
  each (a secret gone since the list is skipped); `remember`, `forget`, `password_of` and `names` are
  the calls on the facade with caller-owned buffers (`INPUT` 1,280, `REPLY` 4,096), wiped after every
  call, at most 64 networks. **Moving the interface-6 `wifi.<name>` secrets to the plugin's own names is
  the secret store's job at the first start (decision 138), not the plugin's.**
  The calls that use the service are not run in a test (no native entry to load in a scripted
  host yet); the parts around them are compared and swept.

- `src/native_plugin.rs`, `src/native_model.rs` (slice 4): the screens, on `paperpaw-ui-kit` and the
  typed senders. The Wi-Fi page (the radio's switch with where it is, the setup with a phone first
  while no network is kept, the networks, the three settings; the same rows and words as the
  interface-6 page), the networks in reach (a kept network joins at once with its password from
  `core.secrets`, an open one once, a secured one after its password is asked for with the host's
  masked text question; the row that looks again; long press or menu button forgets a kept one and
  leaves it if joined) and the setup with a phone up to what is possible now (below). The password
  typed is read from the host text with buffers that are wiped, goes to `Policy::choose` and is kept
  (`native_known::remember`) when the device is in that network (`WifiChanged`, state online), as the
  old page kept it after `wifi_done` (a wrong password is not kept: the join fails and nothing is
  online). **`core.secrets` may refuse to save with `ERR_UNSUPPORTED` (owner decision 141: the X4
  Classic's table has no room for passwords), or the service may be absent: the network is joined all
  the same and the user is told with the shared `core.secrets-unsupported` (core catalogue, en and de).**
  **Todo 179:** the own network's password and the page's token are taken only through
  `Drawn::into_strong` (`Own::draw`); the fields of `Drawn` are private. Where the draw is weak (or
  too few usable bytes came), the setup screen shows the existing `wifi.phone.no-random` and
  nothing else: no weaker draw is substituted. The screen shows the network's name and the password
  in groups of five; it does **not** open the network or serve the page (`NET-serve`: the SDK has no
  `serve`/`respond`, and the QR code needs the canvas drawing of the old screen, to be ported with it),
  so the drawn password belongs to a network that is not opened yet. The text of the password in the
  list is wiped when the screen closes (`ItemList::wipe`).
  A password typed longer than 63 bytes is not joined: the user is told (`wifi.password.too-long`),
  the callback does not fail. The row that looks again scans only while no scan runs.
  Tests: `native_model::tests` (rows, idle choices, bars against the old ones, notes, the choice of
  a network), `tests/native_alloc_refusal.rs` (every allocation of the rows refused),
  `tests/translations.rs` (every text asked for exists in en and de), `tests/linked_entry.rs`
  (the entry carries the checked metadata) and `tests/scripted_networks.rs` (the networks screen
  over a scripted `core.device` and `core.secrets`: the radio, the networks, the names kept; no
  refusal of the host aborts). Metadata: `core.device`, `core.secrets`, `core.ui`; feature
  `uses.wifi`, not required (the host ignores the radio commands where there is none).
- Differences from the old page: the radio's on/off and the settings show as words (On/Off), not as
  switch rows (the kit has none yet); the page shows nothing where the device has no radio; the
  own network's name is the device's model name.

- `src/native_ini.rs` (slice 5): the `wifi.ini` in the plugin's data folder (`Root::Data`, not
  `core.files`: it is the plugin's own storage, as `env.storage()` was), read once at a start
  (`Started`, where there is a radio): at most 16 KiB (a larger file is removed unread and said,
  `wifi.file-too-large`), lines `ssid = …` and `password = …` read as the old plugin read them
  (compared on samples with the old function). The networks are kept among the secrets (a network
  without a password does not take the password kept; a name over 32 bytes or a password over 63
  cannot be a network and is skipped at parse time, with its network, before anything is copied: a 16 KiB file costs no large copies), then the file is removed. Where the device keeps no passwords
  (`Unsupported`) the file stays for the next start and the user is told (`core.secrets-unsupported`);
  after any other failure it stays and nothing is said. Differences: a line that is not UTF-8 is skipped
  (the old reading replaced the bad bytes); at most 64 networks are taken (no more can be kept); the
  old plugin removed the file after reading the secrets back, the native one after every `keep`
  answered. The buffer with the passwords is wiped. Metadata gains `storage.remove`. The host calls
  are not run in a scripted test (the scripted host has no storage yet); the parsing is, and
  every allocation of it is refused in turn.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The scripted host answers `i18n.copy`/`i18n.format` now: `tests/scripted_networks.rs` draws the networks page
  from the plugin's own catalogs and the callback ends well (the case `wifi-pages` is gone). The answers to the
  password question (`SCRIPTED-typed-password`), the status item's value (H1) and the first-run row's badge are
  still not run on a host. Kernel's audit (#2269) reads the remaining markers H3, H4, H1 and `NET-serve-event` as
  stale for the host (compose, staged Wi-Fi commands, the status value and `serve` from the radio's event are
  there); the cases stay ignored until a run on the host shows each one, not on a reading.
- **Differences to Beta 3 found with the headless run on the flip (#243):** the radio row of the Wi-Fi page has no
  value text and is told as a switch (`RowItem::on`), so it reads `Wi-Fi, Connected to <network> (<address>)` or
  `Wi-Fi, Off` as in Beta 3 (the headless test `online_from_the_start_with_wifi_online` pins it; the plugin has no
  unit test of the row, it is made from the host's texts). The status item answers the host's `core.status` ask
  (`native_plugin/events.rs`, `status`) with the item's label (*Wi-Fi*) while a network is joined, not the network's
  name and not the icon Beta 3 drew; a drawn value (`StatusAnswer::drawn`) needs the host's status draw.
- **Host admission of event subscriptions (found with `tests/hosted_construction.rs`, which runs now that
  Services' #1111 admits them):** the host refused tags 92 (`wifi_scanned`), 94 (`network_wanted`) and 95 (`network_idle`) at `make`
  (`Factory(-1)`; 71, 93, 101, 102 and the others are admitted), so the native Wi-Fi is not registered in the
  emulator under `native-linked` ("not registered: Factory(-1)"). Their admission is Services' (#2282, with
  the tag 91 one in #1095); the case is switched on (#1112, measured on the flip with #1123). Tag 38 (`screen_shown`) is refused by the contract
  (`screens-input.md` 2: it goes to the token of the declared screen, never to `registration.event`): the
  subscription is **gone** from the plugin; `event.stack_changed` (81, a hint the host admits) takes its place:
  any screen coming on top is matched again, as in interface 6 (back home or in a book, the radio goes off at
  once). Its cost: one read of the radio per stack change, and the stack (its first row) only while the radio is
  on; nothing is read while it is off.
  **Heap:** the interface-6 Wi-Fi flow (open the page, back, home; X4 profile, headless, `--heap-report`) peaks
  at 100.0 KiB (67.7 KiB at idle, so +32 KiB). Native, on the flip with #1123 and #1130 (X4 profile, headless, the
  same flow, `uses.wifi` offered by a local scratch patch; no radio, so no scan list and no password entry):
  142.9 KiB peak with the radio online, 148.8 KiB with it off, from a native idle of 128.4 KiB (all plugins
  native): the flow adds 14.5 and 20.4 KiB, less than Beta 3's +32 KiB; the gap to 100.0 KiB is the native
  idle baseline (#248). Top Wi-Fi sites at the peak: the radio read (`with_wifi`), the secrets list (`any`).
  Their input buffers are cut to the size the op takes (256 B; `max_input`), their reply buffers stay at the
  op's `max_reply` (the host refuses a smaller buffer: `device` ops 4,096 B, secrets `get`/`keep`/`remove`
  1,280 B, `list` 4,096 B).
- **No real radio on the desktop** (`uses.wifi` is not offered by `shells/desktop/src/linked.rs`; the X4's
  radio is Firmware's wiring, #250): the plugin runs there only on the scripted host.
- The phone setup screen drawn with its join and page codes (`Footer::Code` of the ui kit) is the
  host's compose (H3); the rows and the answers are run in `tests/scripted_phone.rs`.
- Wi-Fi commands cannot be staged by the scripted host (it has no `core.device` context): a callback that
  sends one ends with `ERR_UNSUPPORTED` there, so `tests/scripted_radio.rs` reads what was sent from the
  bytes of the batches (`commands_decode`) and asserts it exactly; staging them and the radio really
  doing them is the host's (H4).

## The radio's events, wired (`src/native_plugin/events.rs`)

- `network_wanted` (94): the radio on / a scan / the join of the network joined before, by
  `Policy::on_wanted`; with none known, `network_none` (what waits fails now, not after the host's 30 s).
  `network_idle` (95): the idle-off timer. `wifi_scanned` (92): `Policy::on_scanned` (the next network
  joined before, another scan, or "No saved Wi-Fi network in reach" and the retry timer). `wifi_done` (93):
  a failure goes to `Policy::on_failed`, so a wrong password stops being pending and the next network is
  tried, or the failure is told; a success decides nothing (the radio's change says it). `wifi_changed`
  keeps the password of the network just joined (`Action::KeepSecret`) and the network the user picked
  last (`picked.txt`, joined first next time, as *Join first* says), and may join one when charging keeps
  the radio on. Also the power mode, the stack changing (81: back home or in a book, radio off at once; see below) and the status
  changing (charging).
- The facts are read per event and only as far as the decision needs them (`Need` in `events.rs`, each fact a
  flag; `tests/scripted_radio.rs` counts the host calls): the radio and its flags (`dev_wifi`, wanted and
  busy; its name and address copied only where the changed event compares them); the networks in reach
  (scanned, failed); the networks joined before: none, **whether there are any** (the names listed, no
  password read: wanted, the retry timer, charging, the radio's change while charging) or with their passwords
  (scanned, failed only); the screen on top (the stack's first row, home or reader; the stack changed, while
  the radio is on); charging and whether the device may use the network (snapshot, `dev_power`; once per
  event, and not where the stack says nothing is at rest); the settings (on demand, idle time) and
  `picked.txt` (scanned, failed, and the radio's change while the user's choice is joined).
- The views keep Beta 3's memory: the Wi-Fi page counts the networks in reach and asks only whether one is
  kept; the networks page holds each name once (in its row, with the signal and openness beside it); a view is
  read again with the old rows freed first; the frame makes no catalog buffers of its own.
- The timers are work (`core.work`, `WorkSpec::at`, keys `wifi.idle-off` and `wifi.retry`, the work event
  subscribed with token 15), at the event's `now_ms` plus the idle time or 10 s. Each is set with the
  policy's generation; one that comes due after the network was wanted again does nothing. A work is lost
  at the next start (the radio then goes off when home or a book shows); where the device has no work
  service, the timers are not set and the same holds.
- The decisions are `native_radio` (the five interface-6 radio scenarios on a simulated host, with the
  same results, and allocation swept); `tests/scripted_radio.rs` runs the wiring through the production
  loader with explicit facts: wanted brings the radio on, none known says so, idle sets the work at
  `now + 60 s` and the due work switches off, a timer from before the network was wanted again and one while
  work waits do nothing, back home switches off and another screen does not, scanned joins the network
  joined before (not the open one in reach, the password shows nowhere), a failed join is told and a success
  is not.
- Not run on a host: the typed password kept after the join (the dialog's answers over the scripted host,
  `wifi-passwords`), the charging path (snapshot with a battery), the retry timer coming due, and what the
  radio really does (H4).

## Review fixes

- Passwords are wiped by `paperpaw_ui_kit::mem::wipe_text`, the helper the keyboard uses too (one
  volatile wipe, not a copy per plugin). `Step::Join` carries the password in a `Password`, so the
  buffer is wiped when the shell has built the `wifi_join` command from it and drops the step.

## Memory parity, open items (review of #1112)

- **Eager Networks rows.** ui-kit's `ItemList` has no lazy mode, so the Networks
  rows are built from the scan when the view opens (about 8 KiB at 64 networks,
  estimated from the code, not measured); Beta 3 built rows lazily. Follow-up: a lazy
  row source for `ItemList` in ui-kit (owner: Plugin Engineer, who owns ui-kit),
  then Wi-Fi switches to it. Measure at 64 networks with the emulator
  `--heap-report` once native delivery of the radio events (Services #1123) is in a
  train.
- **A refused rebuild** in `reread` leaves a hidden Page or Networks view empty until
  the next event (it never panics; the view is rebuilt on the next scan or status
  event).
- **Per-event reads.** `event.stack_changed` reads the radio (two 4 KiB buffers) even
  when the radio is off; status changes read the idle setting at each battery percent.
  Both are short-lived; to narrow after the heap measurement if they show.

## The phone setup served (slice 6, `src/native_plugin/phone.rs`)

- The screen `wifi.phone` opens the own network (`Own::draw` from a fully random `core.secrets`
  `random` draw, none otherwise: "cannot make a safe password"), asks the radio to scan and to host,
  and once it is open serves the page on port 80 with `Net::serve` (`core.net`, `uses.network`):
  every request is answered with `Net::respond` through `native_phone::Phone::request`, a form is
  read from the request's body file (at most 1,024 bytes) and that file is removed at once (the
  plugin cannot overwrite the host's folder), and the password typed on the page is wiped.
- The server cap: the X4 has one server. A refused `serve` (`ERR_BUSY`) is shown on the screen
  ("another page is already served") and is not a failure of the callback. Closing the screen
  (`ScreenClosed`) stops serving (`stop_serving`) and leaves the own network; so does the idle time
  (ten minutes, a `core.work` wake).
- The codes: the join code and the page address are QR codes drawn under the rows by the ui kit's
  new `Footer::Code`; the codes are made with the new fallible `QrCode::try_new` (`paperpaw-qr`), a
  short heap is an error, never an abort; where a code cannot be made the name, the password and the
  address are on the screen to be typed.
- Every buffer of a host call is at least the op's reply bound (secrets `random` 1,280, `core.net` 1,536).
- Tests: `tests/scripted_phone.rs` (seven cases on the production loader: scan and host asked, the page
  served at port 80, the page lists the networks and carries the token, a form without it chooses nothing,
  404, a busy server, a weak draw makes no network, closing stops serving). A callback that sends a
  command ends refused there (`DEV-staging`), so what was sent is read from the batches. Version stays
  1.0.4 (above Beta 3).
- The page the phone gets is at most 4,096 bytes (`choose_page_fit`: the weakest networks are left out,
  the host lists them strongest first), the one write of the data folder it is sent from when it is over
  2,048 bytes (`MOST_PAGE`; unit test with 64 networks). The address is a row of words whenever the
  step shows it (no code where one could not be made or the rows take pages); a refused code is no
  failure of the callback, which would drop the redraw. The join code is wiped (`QrCode::wipe`) when the
  screen closes. A setup that cannot be made (no memory) says so and closes the screen. A body file is
  removed even where no buffer is had for it.
- `serve` is called from the radio's event (the own network opens asynchronously); network.md 5.1 is
  relaxed to any callback while one of the plugin's screens is showing and not covered (Kernel). Until the
  host does so (`net_service/serving.rs`) it answers `ERR_STATE`, which the plugin takes as "not on
  screen" and does not fail on; the end-to-end case is the ignored `wifi_phone_serve_from_event`
  (`BLOCKED NET-serve-event`).

# Interface-7 port (slices 1 to 5: the radio policy, the declarations, the phone setup, the own network, the kept networks, the screens and the file on the card)

Version 1.0.4. Partial: the decisions of the plugin, what it declares, the screens and the file on the card, up to what the
host cannot do yet. The native plugin (`native_plugin::NativeWifi`) has a `NATIVE_ENTRY` (feature
`linked`) and metadata in `native/`; the interface-6 `Wifi` and everything of it stays registered
until the host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_policy.rs`: when the radio is on and which network is joined (the module
  documentation of `policy.rs` says how), on inputs of the plugin's own (`Radio`, `Net`, `Known`)
  and outputs `Step::{On, Scan, Join}` that the native screen turns into `CommandSender::wifi_on`,
  `wifi_scan` and `wifi_join` (`uses.wifi`, a `request` number, flag 0 or "remember"). A test drives
  this and the interface-6 policy with the same random events (200 runs of 40 events) and
  compares what they answer and the state they keep. Allocation is fallible and swept; a password
  is wiped when dropped and never printed.
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

- The scripted host does not answer `i18n.copy`/`i18n.format` (`SCRIPTED-i18n`), so the rows' texts,
  the drawing and the answers to the password question are not run; the status item's value (H1) and
  the first-run row's badge.
- The radio's timers (idle off, retry) and the policy's `scanned`/`failed` entries run on events 92-95
  (below); the screen drives `Policy::choose` and `Policy::changed` only.
- The radio's events: `wifi_scanned`, `wifi_done`, `network_wanted`, `network_idle` (92-95) have
  no SDK subscription or decoder, so the declarations subscribe to the events that exist (started,
  status, screen shown, Wi-Fi and power changed) only. The timers (idle off, retry) need the work
  family; the policy takes the clock as an argument.
- The phone setup serves a page on port 80 of the device's own network: the SDK has no `serve` or
  `respond` yet (`NET-serve`); the decisions and pages are ported (`native_phone`).
- Without `wifi_done` (event 93) a wrong password stays "pending" (it is offered again as the initial
  text when the network is chosen again, and kept only if the device is online in that network); the
  policy's `failed` entry, which lets it go, runs when the event is there.

## Review fixes

- Passwords are wiped by `paperpaw_ui_kit::mem::wipe_text`, the helper the keyboard uses too (one
  volatile wipe, not a copy per plugin). `Step::Join` carries the password in a `Password`, so the
  buffer is wiped when the shell has built the `wifi_join` command from it and drops the step.

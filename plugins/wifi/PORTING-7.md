# Interface-7 port (slices 1 and 2: the radio policy, the declarations and the phone setup)

Version 1.0.4. Partial: the decisions of the plugin and what it declares, without the host. No
native screen and no `NATIVE_ENTRY` yet; the interface-6 `Wifi` and everything of it stays until the
host activates native plugins (then it goes together, no shim).

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

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The screens (H1, H2, H4), the status item's value, and the first-run row's badge.
- The radio's events: `wifi_scanned`, `wifi_done`, `network_wanted`, `network_idle` (92-95) have
  no SDK subscription or decoder, so the declarations subscribe to the events that exist (started,
  status, screen shown, Wi-Fi and power changed) only. The timers (idle off, retry) need the work
  family; the policy takes the clock as an argument.
- The phone setup serves a page on port 80 of the device's own network: the SDK has no `serve` or
  `respond` yet (`NET-serve`); the decisions and pages are ported (`native_phone`).
- Passwords and the `wifi.ini` on the card need `core.secrets` (PR 367) and `core.files`.

## Review fixes

- Passwords are wiped by `paperpaw_ui_kit::mem::wipe_text`, the helper the keyboard uses too (one
  volatile wipe, not a copy per plugin). `Step::Join` carries the password in a `Password`, so the
  buffer is wiped when the shell has built the `wifi_join` command from it and drops the step.

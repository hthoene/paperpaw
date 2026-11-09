# Interface-7 port (slices 1 and 2: the radio policy, its events and the declarations)

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
- `declarations.rs`: the group (order 85, its page), the four settings, the three screens (single
  instance), the action and the status item that open the page, the icons, the `Wifi` needs of the
  settings (hidden where PaperPaw does not manage the radio) and the first-run step (applies only
  with `Wifi`). `tests/abi7_declarations.rs` compares each with the interface-6 registration.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The screens (H1, H2, H4), the status item's value, and the first-run row's badge.
- The radio's events 92-95 are subscribed (`events` in the declarations; decoded by the SDK) and
  decided (`native_radio`). Missing: the timers (idle off, retry) need the work family, and the
  callbacks that read the facts (`dev_wifi`, the networks, the secrets, charging, the stack) and send
  the commands need the screens' wiring (H4).
- The phone setup serves a page on port 80 of the device's own network (`core.net` `serve`).
- Passwords and the `wifi.ini` on the card need `core.secrets` (PR 367) and `core.files`.

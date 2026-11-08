# Interface-7 port (slice 1: the radio policy and the declarations)

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
- The phone setup serves a page on port 80 of the device's own network (`core.net` `serve`).
- Passwords and the `wifi.ini` on the card need `core.secrets` (PR 367) and `core.files`.

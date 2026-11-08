# Interface-7 port (slice 1: the decisions and the declarations)

Version 1.0.7. Partial: what the update decides and what the plugin declares, without the host. No
native screen and no `NATIVE_ENTRY` yet; the interface-6 `Update` and everything of it stays until
the host activates native plugins (then it goes together, no shim).

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
  first-run step (order 57, the `look` setting, only where PaperPaw updates the firmware).
  `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each with the
  interface-6 registration.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Everything that touches the firmware: finding an update on the card, its signer, the install by the
  shell, the check after the restart, the foreign image, the dual-boot declaration. The native SDK has
  no firmware service yet (`paperpaw_sdk::firmware` is the interface-6 environment's). **Needs a
  design from Kernel.**
- Asking the update server, the downloads and fetching the code plugins (network, files, work).
- The weekly look as declared work (`Work::network(..).optional()..at_moments(..)`): no native work
  declaration yet.
- The badge on the page's entry (`set_item_state`) and the first-run step's applicability: the step
  declares the `Firmware` need instead of a callback, as the host does for steps.
- The page (drawing, dialogs, the waiting and dual-boot states) and the events it hears
  (`update.check-due`, `update.look-due`, `setting.changed`, `started`).

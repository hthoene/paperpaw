# Interface-7 port (slice 1: the decisions and the declarations)

Version 1.0.4. Partial: what the transfer decides and what the plugin declares, without the host. No
native screen and no `NATIVE_ENTRY` yet; the interface-6 `Transfer` and everything of it stays until
the host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_transfer.rs`: the address shown and the one the QR code carries (`urls`, given the
  `WifiState::reach` text instead of the state), the names a book may be put under (`is_book_name`),
  the read-only settings file (`read_only`) and the cable offer (`Offer::parse` over the text, with
  `size`, `code`, `plugin_file`, `book` and the first named books): the limits (64 books, 64 plugin
  files, 320 bytes a line, `MAX_OFFER` bytes), the plugin-name and safe-path checks, the books kept by
  16 bytes of the name's SHA-256. Allocation is fallible and swept; a line that is not understood
  makes the whole offer unread, as before. `src/native_parity_tests.rs` compares each with the
  interface-6 code over every pair of grammar lines, good and bad, the limits, and the Wi-Fi states.
- `declarations.rs`: the three screens (the transfer and the cable open one at a time, the phone's by
  parameters), the action `open`, its place in the tools (order 46), its icon and the offers for
  `core.remote-keyboard` (phone) and `core.cable` (cable). `tests/abi7_declarations.rs` decodes the
  batches by the host's rules and compares each with the interface-6 registration.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Serving the page, its key and the lock after wrong keys (`core.sockets`, SOCK; the Wi-Fi state), the
  files (`core.files`, FS) and the web handlers in `web.rs`.
- The installs: the plugin staging and commit of `paperpaw_sdk::install::local` are kernel-only
  (feature `kernel`); a native plugin has no way to install a plugin yet. **Needs a design from
  Kernel** (the DEV family's `core.install`, `registry.md` D). Until then the cable and the Wi-Fi
  page cannot install plugins, and the offer's staged-files check (`staged_as_offered`) is not ported.
- The screens' drawing, the QR code, the offline offer, the own network (H1, H3, WIFI) and the cable
  stage (`core.cable` has no stage declaration yet).
- Events: `transfer.server-event`, `transfer.cable-events` and the answers to the questions come with
  sockets and dialogs (H2).
- The page text, `include_str!("../data/page.html")`, stays with the web handlers.

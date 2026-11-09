# Progress sync (kosync) on interface 7

Version 1.0.9. Native only: the interface-6 plugin (events, state, account and confirmation screens,
HTTP commands, tests that drove a kernel) is gone, with its store file (`tools/plugin-files`) and its
registration in `paperpaw-app`. Behaviour is the same where the host allows; what it cannot do yet is
named below and in `tests/acceptance_pending.rs`.

## What is native

- `declarations.rs` (built into the image by `build.rs`; `native/metadata*` by `tools/abi-metadata.py`):
  the settings group and its three settings (user, *sync by themselves*, server), the account screen
  (`kosync.account`, one instance), the action *Sync progress* and its entry in the reading menu's
  *More* (order 25), the two icons, and the events heard (started, setting changed, book opened and
  closed, page turned, suspend, resume, work due and allowed, a request ended, screen shown).
  Metadata: services `core.device`, `core.net`, `core.secrets`, `core.ui`, `core.work`; features
  `uses.network` and `uses.wifi`, neither required (without a network the plugin loads and says so).
- `src/native_sync.rs`: the sync as a host-free state machine (`Engine`): holds, offers, *declined* and
  *not now*, books left, the offline queue, the once-a-minute rules. After `NoMemory` the caller drops the
  actions and the next event starts again; a `Fetch` or `Send` it cannot make is reported with
  `not_fetched` and `not_sent`. Limits (interface 6 had none): 4 dismissed, 4 unsent, 5 left.
  `tests/native_sync.rs` runs the scenarios of interface 6's `tests/sync.rs`;
  `tests/native_sync_alloc.rs` refuses every allocation in turn.
- `src/native_model.rs`: the server address, the key (MD5, wiped), the body, the answer read (4,096 bytes
  at most), the decision on a fetched position, the percentage text.
- `src/native_flow.rs`: the requests (`Call`: address, headers, body as interface 6 sent them; the key and
  the body are wiped on drop) and the table of requests out (4 at most, the service's limit).
- `src/native_account.rs`: the account screen's rows and decisions (fields, *Sign in*, *Create account*,
  *Sign out*, a changed server signs out, the status of each answer, 402 means *taken* only for a new account).
- `src/native_store.rs`: the key among the secrets (`key`, not changeable; every buffer that held it is
  wiped), the device id in `device.ini`, the move of a key kept in `account.ini` by earlier versions.
- `src/native_plugin.rs`: the services (opened at the first callback; no network or no secrets is no
  error), the events, the account screen, the offer as a confirmation dialog (*Go there* / *Stay here*;
  closing it is *not now*), notices, the work *push* (network, within 30 minutes, moment online, declared
  once), the answers of the server.
- The shared text `core.secrets-unsupported` is shown when the key cannot be kept (the account is then
  not signed in).

## What waits (each an ignored case in `tests/acceptance_pending.rs`)

- **The book's fingerprint** (`core.library` op 25 `book_key`) and **a place's XPointer** (`core.document`
  `xpointer`) have no SDK facade yet (`src/native_backing.rs` answers "unavailable"): the plugin then
  makes no fetch and sends nothing, as interface 6 did for a book it could not read. Signing in, the key,
  the account screen, *Sync progress*'s account prompt and sign out all work.
- **Go there** needs `core.document` `locate`; without it the offer is treated as closed (*not now*).
  It opens the book through `core.reader` with `path`, `section` and `offset` (the reader reads them
  since #629).
- **Stored positions** (`core.library` ops 13 and 14, written with `sync` and, after *Go there*, `confirmed`)
  have no facade: the sync does not write the position it fetched into the library.
- Acceptance of the account screen on the scripted host and of the frames against the interface-6
  snapshots waits for the UI host (H1-H4) and the network host.

## Differences from interface 6

- The offer is a confirmation dialog of the user-interface service (the buttons *Go there* and *Stay
  here*), not a screen of the plugin's own; a message (*up to date*, a failure) is a notice, not an overlay
  with an OK button. The account screen is a list of rows with the line below the rows as its footer
  (the row *More about this* is gone: the footer shows the whole line).
- The progress is `progress_ppm` of the event (millionths) instead of a percentage with one decimal.
- Offline: the host holds the request (`later` on the answer) instead of the plugin queuing positions
  itself; the plugin keeps no list of books switched from while Offline except that the engine keeps the
  four last ones for the moment the power mode allows sending again (`work_allowed`).
- The server's JSON is read with `paperpaw_sdk::json` until Kernel's fallible reader (#414) is in; the
  answer is bounded to 4,096 bytes first (the request's `max_body`), and the reader itself is not in the
  refusal sweep.

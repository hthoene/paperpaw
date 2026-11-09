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

- **The book's fingerprint, a place's XPointer and the place of an XPointer** are asked through
  `src/native_backing.rs`: `Library::book_key` (op 25) for the KOReader fingerprint, and
  `Documents::open_book`, `xpointer` / `locate` and `close` (`core.document`, root 1 with `uses.storage`) for
  the XPointers. A service the device does not offer, a book the library does not know, a book still being
  prepared (`Busy`: tried again at the next chance) and a format without XPointers (the progress is sent
  alone, as interface 6 did) all answer "unavailable"; nothing aborts. The host's side of these calls is
  not wired (Host #417, #944): until it is, they answer `Unsupported` and the sync is a **silent no-op**:
  no fetch, no send, and the 30-minute push work (`Action::PushLater`) is not declared
  (`Backing::can_send` needs the library). Signing in, signing out, the key and the account screen work.
- **Go there** opens the book through `core.reader` with `path`, `section` and `offset` (the reader reads them
  since #629). Where `locate` cannot place the XPointer, interface 6 went by the percentage
  (`at_progress`, the section sizes); the document service gives no section sizes yet, so the offer is then
  treated as closed (*not now*). Ask Kernel/Services for a size read if it is wanted.
- **No library position is written.** Interface 6 did not write one either: *Go there* hands the place to the
  reader, which keeps its own. `Library::position` and `set_position` (`sync`, `confirmed`) are not used.
- The book is opened for its XPointer with the host's defaults, not with the reader's options
  (publisher styles, picture size): a place is a section and an offset in the source, which do not depend on them.
- **Not linked into the app yet** (`NATIVE_PLUGINS`; waits for device activation, #944/#417): the app's
  interface-6 install and its `NETWORK_SETTINGS` entry are gone, so the *Progress sync* row is missing from
  the settings snapshot until then (`tests/snapshots/settings.txt` updated, as for notes-overview).
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

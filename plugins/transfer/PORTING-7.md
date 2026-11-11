# Interface-7 port (slices 1 to 3: the decisions and declarations, the native entry and the transfer screen, installs, the phone's keyboard and the offers)

Version 1.0.4 (already above its Beta 3 version, 1.0.3; no further bump). Status: **not complete**: the native
entry (`NativeTransfer`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin `transfer`) serves the page
and shows its address, installs plugins sent from the computer, types for the phone's keyboard, offers to go online and
opens the reader's own network; the cable and moving files are named blocked steps below, so the plugin is **gated**:
it is not in `NATIVE_PLUGINS` and not in `tools/plugin-files7/complete.txt` (its native file is not the whole
plugin). The interface-6 `Transfer`, `web.rs`, `plugins.rs`, `cable.rs`, `phone.rs`, `link.rs` and everything of
them stay until the host activates native plugins (then they go together, no shim; see the removal step).

## Ported (slice 1)

- `src/native_transfer.rs`: the address shown and the one the QR code carries (`urls`, given the
  `WifiState::reach` text instead of the state), the names a book may be put under (`is_book_name`), the read-only
  settings file (`read_only`) and the cable offer (`Offer::parse`, limits, plugin-name and safe-path checks, books
  kept by 16 bytes of the name's SHA-256). `src/native_parity_tests.rs` compares each with the interface-6 code.
- `declarations.rs`: the three screens (the transfer and the cable open one at a time, the phone's by parameters),
  the action `open`, its place in the tools (order 46), its icon and the offers for `core.remote-keyboard` (phone)
  and `core.cable` (cable). `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each
  with the interface-6 registration.

## Ported (slice 2: the native entry and the transfer screen)

- `src/native_plugin.rs` (`NativeTransfer`): `make` submits the transfer screen, the navigation (action `open`,
  menu entry, icon) and the event subscriptions (`event.server`, `event.served_request`, and in slice 3
  `event.install_done`, `event.wifi_done`, `event.work_due`; `declarations::events`). The cable's screen and its slot
  offer are **built but not submitted** (`declarations::cable_later`): the host would open a screen the plugin cannot
  show; the phone's are submitted since slice 3. *Wi-Fi transfer* (`transfer.open`) opens the screen.
- **Serving** lives on the screen (`network.md` 5.1): when the screen is shown for the first time the plugin writes
  the page and calls `Net::serve(token 1, port 8080, max_body 64 MiB)`; the host takes the next port when 8080 is
  in use and says which in `started`. Closing the screen stops the server (`stop_serving` on the close, which the
  host also does; idempotent) and removes the page file.
- **The server events** are the screen's states: *Starting …* until `started`; the address (text, and the QR code
  of the numeric address) while it runs; *No Wi-Fi* for `no_network` (the text says where to join a network where
  `core.device` `wifi` exists, else the system's); *The transfer could not start: {reason}* for a failed start
  and *The transfer stopped: {reason}* for a stop the network caused (a stop the plugin asked for says nothing).
  A `serve` the host refuses (`Busy`, `Denied`, `Unsupported`) is the same *could not start* with its reason.
- **The key** (`src/native_key.rs`): 16 letters of the 18 a person reads off the screen and types (the SDK's
  `served::key` rule, tested equal to it over the same bytes), drawn anew each time the server has started, from
  `core.secrets` `random` (`Secrets::random`, up to 256 bytes a call, bytes wiped). A draw that is not `strong` (the
  X4's generator before the radio is on) gives no key, never a weaker one: the transfer then says *This device has no
  source of random numbers …* and stops the server. A device with no source never starts the server (checked before
  `serve`).
- **The guard** is the SDK's `served::Guard` (not kernel-only; `network.md` 6 keeps it in the SDK): the key is
  compared in constant time, five wrong keys lock every request for a minute, a request without a key is refused and
  not counted (`/favicon.ico` is answered 404 before the key is asked for). The clock is the event's `now_ms`.
- **The page** (`src/native_page.rs`): `data/page.html` is built into the plugin (as in interface 6, so the
  firmware's transfer does not wait for a file on the card), the translated texts, the device's name
  (`core.device-name`) and the library folder (`core.library-folder`) go where it names them, and the finished page is
  written into the data folder (`page.html`) and sent with `Response::send_file` (an inline body holds 2,048 bytes).
  The page reads a new `T.noMove` that hides *Move* and *Rename* (see below); the interface-6 page never sets it.
- **The requests** (`src/native_web.rs`, over a `Card` trait; `src/native_files.rs` is the `core.files` root 1 one):
  `GET /` (the page), `GET /api/list` (an inline answer of at most 2,000 bytes; folders with more entries go on in
  parts with `next`, as before), `GET /api/file` (the file whole, `send_file` from the card: the host streams it),
  `PUT /api/file` (the body the host saved under `.received/<server>/<request>` is **moved** to the card under a
  free name, `name (2)`; refused with 507 when the body is longer than the free room `core.files` `space` says, the
  card's own *full* is 507 too), `POST /api/mkdir`, `POST /api/delete` (a folder with all in it, `remove_tree`), each
  with the key. Paths are plain names (no hidden, no `..`, no trailing dot or space); the host checks root 1 by
  identity (`plugins` and `.paperpaw` are never reached). `paperpaw.ini` can be fetched but not put, made over or
  deleted (any case). Nothing a computer sends can fail the plugin: an error while answering is answered 500 or 503
  and the callback ends well.
- **What the reader sees of a request** (`transfer.received`, `.sent`, `.folder`, `.deleted`, `.not-received`) is the
  line under the address, as before. Allocation is fallible (`try_reserve`) everywhere, the buffers of a request are
  on the heap (frames stay under 2,048 bytes; the packer checks), no float formatting.
- Tests: `native_web/tests.rs` (every route over an in-memory card: the key, the lock and its end, the listing in
  parts, hostile paths, the free name, the room check, the settings, mkdir, delete), `native_key.rs` (the SDK's key from
  the same bytes, weak draws, no source), `native_draw.rs` (the code's place and that its rectangles cover each dark
  module once), `native_page.rs` (the strings go where the page names them; every text the page reads is given),
  `tests/abi7_declarations.rs` (what `make` submits has no slot offer; the events), `tests/native_linked_entry.rs`,
  `tests/hosted_construction.rs` (`make` runs in a real registered scope: the host admits every family).

## Ported (slice 3: installing plugins, the phone's keyboard, the offers, the hint)

- **Installing plugins from the computer** (`src/native_install.rs` host-free, `src/native_install_host.rs` the
  host's part; the tests are `src/native_install/tests.rs` over a fake host). `GET /api/plugins` writes the list of the
  installed plugins (`core.registry` op 5) to `plugins.json` in the data folder and sends that file (an inline body
  holds 2,048 bytes; the list is cut at 24 KiB). `PUT /api/plugin/<id>/<path>?sha256=<hex>` moves the request's body
  to `staging/<id>/<path>` in the data folder if its SHA-256 is the one said (*422* `damaged` otherwise, the old
  file stays, the body is removed either way); paths follow `install::is_safe_path`, at most 64 files a plugin
  (`install::MAX_FILES`) and four plugins at a time. `POST /api/plugin/<id>` asks the host to install the folder
  (`CommandSender::install_plugin(request, Data, "staging/<id>", update, allow_older)`: `update` where the registry
  has the id, `allow_older` always, as a computer's own build may be older); the **host copies, checks and asks the
  reader** (decision 132), so the answer is *202* `{"state":"asking"}` and one plugin is asked about at a time
  (*409* for another). `install_done` kind 0 ends it: `installed` (with `restart` where its code runs after a restart)
  or `refused` with a reason word (`why`: `not approved`, `damaged`, `not trusted`, `built for another PaperPaw`,
  `no room`, `built in`, ...); `Denied` and `Unsupported` of the command are the answer at once (*422* `not
  allowed`, `not supported`); a full queue is *409* `busy` and the files stay for another try. A plugin built into the
  firmware is refused before the host is asked (`built in`). `GET /api/plugin/<id>` tells the state (`none`,
  `asking`, `installed`, `refused`); `DELETE` drops the files (not while the host asks: *409*). The staged files go when the
  screen closes or opens (but those the host is asked about). The device says on the screen *Installed: {name}*, *...
  It runs after a restart.* or *Not installed: {name}* (the interface-6 question for code, `transfer.code.*`, is the
  host's confirmation now).
  Differences from interface 6: (a) `origin` in the list is `built-in` or `installed`; the registry row has no
  store/local origin (`docs/usb-transfer.md` says so); (b) the checks of the folder (is it the plugin, is its code
  whole and built for this device) and the trust rules are the host's, so the refusals come as the host's reasons;
  (c) `uses.install` is declared (required).
- **The phone's keyboard** (`src/native_phone.rs`, `src/native_plugin/phone.rs`, `data/phone.html`). The screen
  `transfer.phone` (per parameters) and its offer for `core.remote-keyboard` are submitted. When the keyboard asks (the
  host opens the screen as the dialog), the screen reads the question (`core.ui`: the request from the handover
  parameters, then `question`: title, whether the text is a secret, the text to start from, at most 4,096 bytes), starts
  the **second server** (token 2, port 8080 or the next, `max_body` 4,096) with a key of its own and shows the address
  and the code. The page is written to `phone.html` and sent with `send_file`; a password question gets a masked field and
  never the text. `POST /send` (the key checked as the transfer's) answers *200*, then writes the text to the
  question's text and answers it (`Answer::Accept`), and the screen closes; a text too long is *413*, one that is not
  UTF-8 *400*; a screen closed without an answer is the host's cancel. A secret question's screen is a secret screen
  (`ScreenPolicy::secret`), and the text and the initial text are wiped. The keyboard offers *Phone* only where a
  screen fills the slot (it asks the registry), so the offer is what turns the function on.
- **The offer to go online** (`src/native_plugin/own.rs`): when the screen is shown in the offline power mode
  (`core.device` `power`, level Offline) it shows the rows *Go online* and *Not now* (the host's `core.offline.*`
  words) with the offline text below. *Go online* sends `go_online(600,000)` (in the input that chose it, the host's
  2 s window) and serves; *Not now* is the *No Wi-Fi* view.
- **The reader's own network** (*Without a router*, same file): on *No Wi-Fi* where PaperPaw runs the Wi-Fi (`core.device`
  `wifi` exists) a row *Without a router* draws a password of 15 letters from `core.secrets` (the SDK's
  `readable(15)`, tested equal), sends `wifi_on` and `wifi_host(request, ssid, password)` (the device's name as the
  name, in the input that chose it; `uses.wifi`, optional), and on `wifi_done` serves the page there. The upper half of the screen
  is the code that joins it (`wifi::join_code`) with the name and the password to type, the lower the address
  and its code. Closing the screen leaves the network (only where the radio is hosting, never the router's) and an exact
  timer (`core.work`, `at_ms`) closes the screen after ten minutes without a request. Differences from interface
  6: the row is on *No Wi-Fi* only, not on *Starting* and not while serving at a router (so a computer in another
  network is served by leaving the router's network first).
- **The page's hint for a book sent outside the library folder**: `T.books` is the extensions of the formats
  (`core.document` op 19, no permission; each once, at most 128), so the page names the library folder as before.
- **What the host admits**: every registration `make` submits is admitted (`event.install_done`, `event.wifi_done`
  and `event.work_due` too: `tests/hosted_construction.rs` constructs the plugin in the kernel's real scope).

## `paperpaw.ini`: not served, not changed (defence in depth)

In interface 7 `paperpaw.ini` never holds secrets (Kernel, decisions 138/140): Wi-Fi passwords, keys and tokens
live only in the sealed secret store `core.secrets` (AES-256-GCM); a value found in the card file for a secret
setting moves into it at load (`docs/abi/secrets.md` 4). A card that comes over from Beta 3 can still hold plain
secrets in the file until its first load moves them out, so the plugin refuses the file anyway (this is a change
from interface 6, which let the page fetch it): `GET /api/file` answers 403 for it, in any case, and `read_only`
still refuses uploading over it, deleting, moving and renaming it (403). Pinned by
`the_settings_are_not_served` and `the_settings_cannot_be_put_made_or_deleted_over`; the interface-6 parity test
compares `read_only` with the interface-6 rule.

## Blocked and next steps

1. **Moving and renaming files** (`POST /api/move`: 501; the page hides *Move* and *Rename*). The library keeps
   positions, notes and catalog data under a book's key, and a plain `core.files` move would leave them behind.
   `core.library`'s `move` (`library.md`: the file and everything kept for it follow) is not in the SDK
   (`native::library` has query, book, fields and annotations only). **Needs `Library::move` (and `delete`,
   `import`) from Services.** The same op is wanted for books uploaded (`import`: a journalled move that catalogues
   at once; today the walk finds the file) and deleted: *Delete* removes the file or folder through `core.files`, and
   what the library kept for a removed book (its notes) is left for the walk's clean-up (interface 6 emitted
   `BOOK_REMOVED` and told the moved).
2. ~~Installing plugins from the computer~~: native (slice 3).
3. **The cable** (`cable.rs`, the screen `transfer.cable`, slot `core.cable`): **missing: the stage declaration for
   `core.cable`** (a plugin cannot declare that it serves the host's cable stage; the server itself exists:
   `Server::cable(token)`, port 0), **and the dialogs** (#847/#854 are not merged: the reader is asked before anything
   is written, H2, and the offer's plugin and books are named in it). With them: the offer (`native_transfer::Offer`
   is ported and tested), the staged-files check (`staged_as_offered`, not ported) and the install by the same
   `install_plugin`. The screen and its slot offer are built (`declarations::cable_later`) but not submitted.
4. ~~The phone's keyboard~~: native (slice 3).
5. **The reader's own network**, what is not there: its button on *Starting* and while serving at a router (see
   above); the rest is native (slice 3). Everything else the offline mode needed is native.
6. ~~The page's hint for a book sent outside the library folder~~: native (slice 3).
7. **The screen's look**: the address screen is the list's header and an own layout (code, address, hint, last thing
   said, centred; the code takes what room is left, the text wraps in up to 4, 5 and 2 lines), not the SDK's
   `draw_address`, which draws on an interface-6 `Canvas`. A screen of this kind belongs in `paperpaw-ui-kit`
   (as update's page does).
8. Ignored acceptance cases for these are in `tests/acceptance_pending.rs` (they name what they wait for).

## Removal step (one sweep, with the reader port and Core #944)

1. `Transfer`, `TransferScreen`, `web.rs`, `plugins.rs`, `link.rs`, `phone.rs`, `cable.rs`, the interface-6 tests
   (`src/tests.rs`, `tests/serving.rs`, `tests/plugins.rs`, `tests/cable.rs`, `tests/upload_memory.rs`,
   `native_parity_tests.rs` once nothing is left to compare), the `HOW` table kept for the manifest; `data/page.html`
   and `data/phone.html` stay (the native page is built from the first).
2. The app's registration of `Transfer` (`crates/paperpaw-app/src/lib.rs`).
3. List `NativeTransfer` in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` only when blocked steps 1 to 6
   are native (every user-visible function works natively), and then submit `screens_later` and `offers`.

# Interface-7 port (slices 1 and 2: the decisions and declarations, the native entry and the transfer screen)

Version 1.0.4 (already above its Beta 3 version, 1.0.3; no further bump). Status: **not complete**: the native
entry (`NativeTransfer`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin `transfer`) serves the page
and shows its address, but the phone's keyboard, the cable, the offer to go online, the reader's own network,
installing plugins from the computer, and moving files are named blocked steps below, so the plugin is **gated**:
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
  menu entry, icon) and two event subscriptions (`event.server`, `event.served_request`; `declarations::events`).
  The phone's and the cable's screens and their slot offers are **built but not submitted** (`screens_later`,
  `offers`): the host would open a screen the plugin cannot show. *Wi-Fi transfer* (`transfer.open`) opens the screen.
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
  `tests/hosted_construction.rs` (`make` is refused as `Unsupported` until the host admits every family, Core #944).

## Blocked and next steps

1. **Moving and renaming files** (`POST /api/move`: 501; the page hides *Move* and *Rename*). The library keeps
   positions, notes and catalog data under a book's key, and a plain `core.files` move would leave them behind.
   `core.library`'s `move` (`library.md`: the file and everything kept for it follow) is not in the SDK
   (`native::library` has query, book, fields and annotations only). **Needs `Library::move` (and `delete`,
   `import`) from Services.** The same op is wanted for books uploaded (`import`: a journalled move that catalogues
   at once; today the walk finds the file) and deleted: *Delete* removes the file or folder through `core.files`, and
   what the library kept for a removed book (its notes) is left for the walk's clean-up (interface 6 emitted
   `BOOK_REMOVED` and told the moved).
2. **Installing plugins from the computer** (`GET /api/plugins`, `PUT /api/plugin/<id>/<path>`, `POST` and
   `DELETE /api/plugin/<id>`: 501). The way is `CommandSender::install_plugin(request, Data, <staged folder>, update,
   allow_older)`: the host copies, checks and **asks the reader** (decision 132) and the plugin takes `Denied` or
   `Unsupported` as the answer; the files would be staged under the data folder, each checked against its SHA-256.
   What is missing is not host-side but the plugin's own state machine: the per-plugin states (`asking`,
   `installed`, `refused` with the reason of `install_done` kind 0), `GET /api/plugins` (the installed plugins:
   `core.registry`), the staging rules (`paperpaw_sdk::install::local` is kernel-only and not ported) and the
   `uses.install` declaration. A slice of its own (about the size of `plugins.rs`, 277 lines).
3. **The cable** (`cable.rs`, the screen `transfer.cable`, `core.cable`): the stage has no declaration yet; the
   server for the cable is `Server::cable(token)` (port 0) and exists in the SDK. Its offers (`src/native_transfer.rs`'s
   `Offer`) need a dialog (the reader is asked before anything is written, H2) and the staged-files check
   (`staged_as_offered`, not ported). The screen and its slot offer are built but not submitted.
4. **The phone's keyboard** (`phone.rs`, `transfer.phone`, `core.remote-keyboard`): needs `core.sockets` or a second
   server with its page (`data/phone.html`), the keyboard stage's input and the key of its own. Not submitted.
5. **The offer to go online** in the offline mode and **the reader's own network** (*Without a router*, the code to
   join it, the 10-minute idle close): `core.net` `serve` refuses with `offline` (said as *could not start: offline*);
   the own network needs `Wifi` host commands (`WifiCommand::Host`, `Leave`) and `core.secrets` for its password,
   both of the Wi-Fi plugin's slice, and the offline offer a dialog in the stock look (`ui::offline` is interface 6).
6. **The page's hint for a book sent outside the library folder** reads the formats the library reads
   (`T.books`, from `env.formats()` in interface 6): `core.registry` formats are not read yet, so the list is empty and
   the hint is not shown.
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

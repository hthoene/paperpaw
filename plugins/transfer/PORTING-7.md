# Interface-7 port (slices 1 to 4: the decisions and declarations, the native entry and the transfer screen, installs, the phone's keyboard, the offers and the cable)

Version 1.0.4 (already above its Beta 3 version, 1.0.3; no further bump). Status: **complete** (named differences below): the
native entry (`NativeTransfer`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin `transfer`) serves the page
and shows its address, installs plugins sent from the computer, types for the phone's keyboard, offers to go online,
opens the reader's own network, moves, renames and deletes files through the library and answers a computer at the USB
cable. It is in `NATIVE_PLUGINS` (linked under `native-linked`) and in `tools/plugin-files7/complete.txt`. The interface-6
code is gone (see the removal step): the app no longer registers `Transfer`, so this lands with native activation.

## Ported (slice 1)

- `src/native_transfer.rs`: the address shown and the one the QR code carries (`urls`, given the
  `WifiState::reach` text instead of the state), the names a book may be put under (`is_book_name`), the read-only
  settings file (`read_only`) and the cable offer (`Offer::parse`, limits, plugin-name and safe-path checks, books
  kept by 16 bytes of the name's SHA-256). `src/native_transfer_tests.rs` pins each (the comparisons with the interface-6 code went with it).
- `declarations.rs`: the three screens (the transfer and the cable open one at a time, the phone's by parameters),
  the action `open`, its place in the tools (order 46), its icon and the offers for `core.remote-keyboard` (phone)
  and `core.cable` (cable). `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each
  with the interface-6 registration.

## Ported (slice 2: the native entry and the transfer screen)

- `src/native_plugin.rs` (`NativeTransfer`): `make` submits the transfer screen, the navigation (action `open`,
  menu entry, icon) and the event subscriptions (`event.server`, `event.served_request`, and in slice 3
  `event.install_done`, `event.wifi_done`, `event.work_due`; `declarations::events`). The phone's screen and slot offer
  are submitted since slice 3, the cable's since slice 4 (`declarations::cable`). *Wi-Fi transfer* (`transfer.open`)
  opens the screen.
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
  The page's `T.noMove` (hides *Move* and *Rename*) is no longer set: moving and renaming are native (see
  *Moving, deleting and uploading through the library*).
- **The requests** (`src/native_web.rs`, over a `Card` trait; `src/native_files.rs` is the `core.files` root 1 one):
  `GET /` (the page), `GET /api/list` (an inline answer of at most 2,000 bytes; folders with more entries go on in
  parts with `next`, as before), `GET /api/file` (the file whole, `send_file` from the card: the host streams it),
  `PUT /api/file` (the body the host saved under `.received/<server>/<request>` is **moved** to the card under a
  free name, `name (2)` (a book into the library folder is imported, see below); refused with 507 when the body is longer than the free room `core.files` `space` says, the
  card's own *full* is 507 too), `POST /api/mkdir`, `POST /api/delete` (a folder with all in it, `remove_tree`; a book through the library),
  `POST /api/move` (see below), each
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
  `tests/hosted_construction.rs` (the plugin is constructed in the real registered scope of the host; since the host admits `install_done`, `wifi_done` and `work_due` the construction is asserted, no longer refused).

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
- **What the host admits**: `event.install_done`, `event.wifi_done` and `event.work_due` are admitted now (the
  train has them): `tests/hosted_construction.rs` constructs the plugin in the real registered scope (slice 4 also
  with the cable's screen and slot offer and `core.library`); the interface-7 loader still has to activate native
  plugins (Core #944).

## Ported (slice 4: the cable)

Kernel's ruling (#244): `transfer.cable` stays a **plain native screen** offered on slot `core.cable`, no stage. The
host (Kernel #900, `origin/kernel/cable-native-228`, not in this branch) opens it when a computer talks over the USB
cable and no server runs, and serves the cable to the one plugin that serves port 0 from its showing screen. Ported
against that contract; what needs the host's cable is `tests/cable_native.rs`, ignored with
`needs native activation (#900 cable host)` (assertions kept).

- **The screen** (`src/native_plugin/cable.rs`, token `CABLE`, one at a time): when first shown it calls
  `Net::serve(Server::cable(3).max_body(64 MiB))`; the server's `event.server` is its state (`started`; `failed` says
  why, as the Wi-Fi's; `stopped` while it was running too). It shows what interface 6 showed (waiting / a computer is
  connected / asking / allowed / not allowed, and below it the last thing said: receiving, received, installing,
  installed, not installed) as the list's empty-state footer, en and de (new keys `transfer.cable.waiting`, `.linked`,
  `.asking`, `.asking.text`, `.allowed.title`, `.refused.title`, `.installing`). Closing the screen (host *Back* or the
  idle timer) ends the permission, stops the server (idempotent), removes what was staged and cancels its timers.
- **The protocol** (`src/native_cable.rs` host-free over a `CableHost` trait; `src/native_cable/host.rs` the host's
  part): `GET /api/hello`, `GET /api/plugins`, `POST`/`GET /api/offer`, `PUT /api/plugin/<id>/<path>`,
  `POST /api/plugin/<id>`, `PUT /api/book/<name>` with the interface-6 statuses: 202/409/507/400 for the offer, 403
  `not allowed on the device` before the permission and 404 after, 422 `not as offered`. The offer body is the saved
  body file, read a line at a time by `native_cable::read_offer` over `ui-kit`'s `Volume` (at most `MAX_FILES` lines
  of `MAX_LINE`, the file at most `MAX_OFFER` bytes; the line rules are `native_transfer::Offer::take`, now public
  with `Offer::finish`). The reader is asked with `Ui::ask(Question::confirm)` (title and text as `cable.rs`
  `question`, the yes word `transfer.cable.allow`, the dialog's own *No*); `event.answered` names the request, so it
  counts for that offer only, and a dialog closed without an answer (outcome cancelled) refuses.
  `staged_as_offered` is ported (`native_cable::staged_as_offered`: the staged files, found by a bounded walk of the
  cable's folder, are exactly the offered ones, each with its size and SHA-256, checked again at the install).
- **The files** go to the cable's own folder in the data folder (`cable/<id>/<path>`, apart from the Wi-Fi's
  `staging/<id>`, which the Wi-Fi screen cleans; the cable cleans its own when its screen opens and closes), through
  `HostPlugins::staging_in`. A book goes with `core.library` `import` (`Existing::FreeName`, `from` = the host-saved
  body file), after its SHA-256 was checked; offered once, taken once. `uses.library` is declared **optional**
  (`core.library` is in the metadata): without the grant a book is *501*. The install is the Wi-Fi's command
  (`install_plugin(request, Data, "cable/<id>", update, allow_older)`, requests from `0x7ffe_0000`).
- **Timers and busy answers**: an exact timer (`core.work`, `at_ms`, key `transfer.cable-idle`) closes the screen after
  five minutes without a request (not while the reader is asked or the host asks about an install: it counts again from
  the answer); a `respond` refused as `Busy` (the host's cable queue is full) is kept (at most 4) and sent again with
  the next request or the retry timer (`transfer.cable-retry`, every 250 ms, about 100 s), never lost silently.
- **Guard**: none (a computer at the cable; interface 6 had none); the permission is the reader's answer.
- Tests: `native_cable/tests.rs` (the permission state machine, answers for another offer, the offer's limits and
  refusals, files as offered, the install waiting for the host and its refusals, `staged_as_offered`, books, the
  library's refusals, close, and `fail_alloc::sweep` over the plugin flow, a book, the offer's file and the staged
  check), `native_cable/host.rs` (sizes with whole numbers), `tests/abi7_declarations.rs` (the cable batch and its
  slot decoded by the host's rules), `tests/cable_native.rs` (ignored: the hosted flow).
- **Differences from interface 6** (case by case against `src/cable.rs` and `tests/cable.rs`):
  (a) `hello`'s `fingerprint` is `null`: `core.device` has none in interface 7 (it has `abi_major`, `abi_draft` and
  `target` in `native`); `version` is the firmware's (`core.device` `firmware`), else the SDK's.
  (b) `POST /api/plugin/<id>`: the **host asks the reader** before it installs (decision 132), a second question after
  the one about the offer. The answer is held and sent when `install_done` ends the request (200 installed, 422 refused
  with the host's reason word: `not approved`, `damaged`, `not trusted`, `built for another PaperPaw`, ...); a reader
  slower than the host's 120 s answer time leaves the computer without an answer (`GET /api/plugins` shows the
  result). The checks of the folder, the code and the trust are the host's, not `Commit::approved`; a refused install
  drops the offer's plugin and its staged files.
  (c) `GET /api/plugins` over the cable is an inline answer (the cable takes no `file`): cut at 2,000 bytes with
  `"next":<n>` where more follow (`?skip=<n>`); `origin` is `built-in` or `installed`.
  (d) The question's text is at most 1,024 bytes (`core.ui`): names that do not fit are counted in *and N more*; sizes
  are written with whole numbers only (same words as before).
  (e) A question closed without an answer is the host's cancel (`event.answered`), not a scan of the stack: a screen
  shown over it never counted, and now nothing has to be asked. The *Back* that ends the permission closes the screen;
  the 403 `ended on the device` and the 10 s pause are the kernel's (#900).
  (f) Books are catalogued by `core.library` at once (no `library::added` event); the library's name rules are
  stricter than the offer's (a trailing dot, reserved device names): *422* `name` for such a name; a full card is *507*.
  (g) A question that cannot be asked now (`Full`, `Busy`: another is open) is *503* `cannot ask now`, new.
  (h) The Wi-Fi exclusion is one way: an offer for a plugin the Wi-Fi transfer holds (staged or asking) is *409*, but
  the Wi-Fi's `PUT` is not refused while the cable is offered the same plugin (the folders are apart; the host
  refuses a second install of the plugin while one runs).
  (i) A file of size 0 comes without a body and is *403* `not offered`, as in interface 6.

## Moving, deleting and uploading through the library (`native_web.rs`, `native_files.rs`)

- **`POST /api/move?path=&to=[&name=]`** (the page's *Move* and *Rename*; interface 6's rules, kept): the source is
  there and is not the top or `paperpaw.ini`; `to` is a folder that is there (the top for none); `name` is a plain
  name (the source's own when none); the target is never over what is there (`name (2)`, before the extension);
  a folder never goes into itself or below itself (*400*); `paperpaw.ini` is neither moved nor a target
  (*403*); the same place (as the card names it, case ignored) answers *200* and changes nothing, so a rename that only
  changes case is no change. The reader's line is *Moved: {name}* (`transfer.moved`, the name it has now; en and de).
  **A book file** (the library's preflight `can_delete` says it is one: the file-manager's rule, the format by
  extension or sniffing) is moved by `Library::move_to` (`replace` off), so its position, notes, fields and the
  plugins' data follow and its key changes (`library.md` 2.3 and 6). **A folder or any other file** is
  `core.files` `move` (root 1 to root 1, `replace` off): the host's library follows every move on root 1 by itself
  (`files.md` 2.6), books below a folder included, so the plugin sends no moved event (interface 6's
  `tell_moved` is gone). Differences from interface 6: the host's refusals are said (*409* `busy, try again` for
  `Busy`, which is also a move job running for another move, *409* `exists` for `State`, *403* `not allowed` for
  `Denied`, *507* for `Full`, as for any change; out of memory stays *503*).
- **`POST /api/delete`**: a book file is `Library::delete` (`paths=[path]`, `with_notes` on: interface 6 deleted
  everything kept for the book and the user asked on the page); a folder is `remove_tree` (the host forgets the books
  below it), any other file `remove_tree` too. The answers of the host are the same as for a move (a book the reader has
  open is `Busy`, *409*). Deliberate (Manager, #244): deleting a book from the page takes its notes with it, exactly
  like deleting in the library: one rule.
- **Cable answers (review fixes)**: `POST /api/offer` while a plugin installs is *409* `busy` (a new offer would forget
  the install); an answer that cannot be built for want of memory waits for the retry timer like a busy host (what it
  answers was done already, so a "try again" would make the retry *403*); where four answers wait, the oldest is tried
  once more before it goes.
- **`PUT /api/file`**: the body (a file of the data folder, root 0) is **imported** (`Library::import`, `from` the
  body's root-0 path, `folder` the part of the target below the library folder, `Existing::FreeName`, so the book is
  catalogued at once and the walk need not find it) when the target is a book name (an extension of the formats,
  `core.document` `formats`, as the page's hint does) inside the library folder (`core.library-folder`, read per
  upload; case ignored; an empty setting is the whole card). Everything else is moved with `core.files` as before.
  Where the library refuses the names (`Invalid`, nothing sent) the body is moved as before. The line says the name the
  host took.
- **Without the library** (`Library::open` refused: not declared or not allowed) books are files: the same calls go
  to `core.files`, which the host's library follows on root 1. `uses.library` is declared **optional** and
  `core.library` is in the services (`uses.storage` was already required, which move and delete need).
- Tests: `native_web/tests.rs` over the in-memory card, which logs whether the library or the card did each change
  (a file, a book, a folder, into itself, over what is there, the same place, the settings, missing and hostile
  names, the host's refusals, an allocation sweep of each route), `native_files.rs` (the library folder and the
  book names).

## Memory of the transfer screen on the X4 (#248)

- **Beta 3 (interface 6), measured** (emulator `--heap-report`, X4 profile, `wait:300,reset-peak,run:transfer.open,wait:600`
  on t10): the heap is 67.9 KiB at home and 89.8 KiB with the transfer screen open (+21.9 KiB); the peak since the reset
  is 103.3 KiB (+35.4 KiB over home).
- **Native: not measured yet, named.** The desktop emulator cannot run the native plugin end to end today: it does not
  back `uses.install` (a refusal at admission; a local, uncommitted measurement patch offering it let `make` pass), and a
  native plugin's `core.network.transfer` and `core.saving.transfer` settings are made at admission only with Services
  #894 (`NET-SETTINGS-NATIVE`), so the `transfer.open` action does nothing (`unknown setting`). The X4 packer accepts
  the file (`tools/plugin-files7/build.sh x4 --only transfer`: 250,784 bytes, every frame under 2,048 B). The gate of
  #248 for this screen stays open until the flow runs natively; what is kept from Beta 3: streamed transfers (a body is
  a file of the data folder, a download a `send_file`; no body in memory), an inline answer of at most 2,000 bytes, the
  listing in parts, the page written to the data folder, buffers of one request on the heap and gone with it, one
  `core.files` handle with its two buffers per request, fallible allocation everywhere (sweeps in the tests).

## `paperpaw.ini`: not served, not changed (defence in depth)

In interface 7 `paperpaw.ini` never holds secrets (Kernel, decisions 138/140): Wi-Fi passwords, keys and tokens
live only in the sealed secret store `core.secrets` (AES-256-GCM); a value found in the card file for a secret
setting moves into it at load (`docs/abi/secrets.md` 4). A card that comes over from Beta 3 can still hold plain
secrets in the file until its first load moves them out, so the plugin refuses the file anyway (this is a change
from interface 6, which let the page fetch it): `GET /api/file` answers 403 for it, in any case, and `read_only`
still refuses uploading over it, deleting, moving and renaming it (403). Pinned by
`the_settings_are_not_served` and `the_settings_cannot_be_put_made_or_deleted_over`.

## What is not there, and the rest (all named, none blocks the port)

1. ~~Moving and renaming files~~: native (see *Moving, deleting and uploading through the library*).
2. ~~Installing plugins from the computer~~: native (slice 3).
3. ~~The cable~~: native (slice 4).
4. ~~The phone's keyboard~~: native (slice 3).
5. **The reader's own network**, what is not there: its button on *Starting* and while serving at a router (see
   above); the rest is native (slice 3). Everything else the offline mode needed is native.
6. ~~The page's hint for a book sent outside the library folder~~: native (slice 3).
7. **The screen's look**: the address screen is the list's header and an own layout (code, address, hint, last thing
   said, centred; the code takes what room is left, the text wraps in up to 4, 5 and 2 lines), not the SDK's
   `draw_address`, which draws on an interface-6 `Canvas`. A screen of this kind belongs in `paperpaw-ui-kit`
   (as update's page does).
8. Ignored acceptance cases for these are in `tests/acceptance_pending.rs` (they name what they wait for).

## Removal step (done: held until native activation)

Removed: `Transfer`, `TransferScreen`, `web.rs`, `plugins.rs`, `link.rs`, `phone.rs`, `cable.rs`, the interface-6 tests
(`src/tests.rs`, `tests/serving.rs`, `tests/plugins.rs`, `tests/cable.rs`, `tests/upload_memory.rs`), the `HOW` table
(`how/` stays: the native metadata is built from it), the `kernel` feature of `paperpaw-sdk` and the app's registration of
`Transfer`. `native_parity_tests.rs` became `native_transfer_tests.rs` (the decisions pinned by values, and the
allocation-refusal sweeps). `data/page.html` and `data/phone.html` stay. What the interface-6 `tests/*.rs` pinned
through a kernel is in the native unit tests (`native_web`, `native_install`, `native_cable`, `native_key`) and, through
the production loader, in `tests/cable_native.rs` and `tests/acceptance_pending.rs` (ignored until native activation).
The app has no transfer until `native-linked` is on, so the PR is HELD until activation.

## `uses.install` is optional (#243)

The native declaration lists `uses.install` as an optional feature (`native/metadata.json`). The desktop
and the emulator do not offer it, and a required feature that is missing refuses the whole plugin at
admission (`make` ended with status -1, *not registered*). Without it the plugin registers and works;
a plugin sent from the computer (Wi-Fi page or cable) is refused by the host (*not allowed*) and the
computer is told so. Test: `hosted_construction::make_registers_where_the_host_offers_no_install_feature`.

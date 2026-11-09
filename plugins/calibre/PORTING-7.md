# Calibre wireless library on interface 7

Status: **native only.** The interface-6 plugin (session, screen, library bookkeeping, socket
commands) and its kernel tests are removed; the native plugin is linked where the device has it
(`tools/plugin-files7/complete.txt`). `calibre` leaves `STORE_PLUGINS` (35). Host acceptance waits for
the activation of native plugins and the host's sockets and library (`tests/acceptance_pending.rs`).

## What it does

- Declarations (`declarations.rs`, `build.rs`, `native/metadata*` by `tools/abi-metadata.py`): the
  *Address* setting (library group, advanced, in the part `calibre.plugin.name`), the single-instance
  screen `calibre.screen`, the action `calibre.open`, the Tools entry (order 47), the icon, and the
  subscriptions to socket and broadcast events. Services: `core.sockets`, `core.library`, `core.files`,
  `core.secrets`, `core.ui`, `core.device`, `core.document`; features `uses.network`, `uses.library`,
  `uses.storage`. En/de texts and "how it works" are unchanged except the four hints that told how to
  confirm (the screen's rows now say it).
- The wire format (`native_model.rs`, host-free): frames, framing in any chunking, members, the answers
  to `GET_INITIALIZATION_INFO` (with the SHA-1 password hash, wiped) and `GET_DEVICE_INFORMATION`, book
  paths that cannot leave the library folder or reach hidden data, the answer to the broadcast. A refused
  allocation is `NoMemory`; members are found without allocating and their text made by the kit's
  fallible reader (`paperpaw_ui_kit::json`, `raw_member` made public for it): the old "gate" is closed.
- The session (`native_session.rs`, host-free, tested in `tests/native_session.rs`): the state of the
  screen, what Calibre asks and the answers, the book list in parts of about 16 KiB, the receiving of a
  book (into the plugin's data, then put in the library), the deleting of books Calibre sent, a password
  asked for, the end of the connection. It returns *steps* for the plugin to do and asks the device what
  it needs through the `Backing` trait.
- The plugin (`native_plugin.rs`): the events of the broadcast (found, silent, no network) and of the
  socket (connected, bytes, file saved, send queue drained, ended); frames wait in a queue that is pumped
  as the host's send queue drains (`sent`); a message is read in exact pieces of at most 3,840 bytes
  (one read at a time), a character cut in two by a piece waits for the rest; a book is read with
  `read_file` into `incoming` of the plugin's data and put in the library with `Library::import` (`Replace`
  for a book Calibre sent before, `Refuse` otherwise: a file there that Calibre did not send is told,
  `calibre.not-replaced`); a book Calibre deletes goes through `Library::delete` (with its notes) or, for
  a file the library does not take for a book, from the card. The password is a secret (`core.secrets`,
  name `password`, wiped; shown nowhere); a device that keeps none says so (`core.secrets-unsupported`).
  The screen is a list on the kit's `ItemList` (status as the heading, the library and the books received
  as faint rows, *Connect* / *Try again* / *Type the password* as the row to choose, the hint in the footer).
- The records of the books Calibre sent, what it said about itself and the computer trusted are the
  plugin's data (`native_store.rs`): `books/<md5 of the lpath>.json` (metadata without the cover
  thumbnail, at most 64 KiB), `device.json`, `host.txt`.
- Limits by device class (`core.device`): a message is held whole up to 64 KiB on a small device (the X4),
  1 MiB elsewhere.

## Differences from interface 6

- No *go online* offer in offline mode: a connection refused because the device is offline, on hold or its
  network turned off says "no Wi-Fi" like any other missing network.
- After a delete, what the library kept for the book goes with it at once (interface 6 removed only the
  file and the host's next walk removed the rest): the same end state, sooner.
- The reasons a connection failed are short English words (`timed out`, `refused`, …) inside
  `calibre.failed`; interface 6 passed the kernel's words on.
- The text is read in pieces the plugin asks for (`exact` reads), not as a text read of the kernel.

## Not ported / not accepted yet

- **What the library learns from Calibre's metadata** (title, author, series, number, tags: interface 6
  `library::provide`) is not set: there is no `Library::provide` (LIB op 24) in the SDK facade yet (asked
  of Services). A received book shows what its file says until it exists.
- No scripted-host test drives `native_plugin.rs` (the steps, the pump of frames, the reads, the import);
  the session and the model are tested without a host (including a refusal sweep of every allocation of
  the session). Host acceptance (a real Calibre, a real book) waits for the activation of native plugins
  (Core #944, Host #417), the sockets wiring in the shells and the host's library import.
- X4 file size and peak memory (the 64 KiB message bound, the 4 KiB queues) are not measured.
- The frame of a hash answer is built by `native_model::frame` from copies that are not wiped (the
  digest's state and the hash text are; the frame itself is wiped once queued); the hash is bound to
  Calibre's challenge.

Version: 1.0.11 is above Beta 3 (1.0.7): no bump.

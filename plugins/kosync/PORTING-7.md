# Progress sync (kosync) on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (events, state, account and confirmation screens,
HTTP commands) is still in place beside the native one. It is removed in the PR that finishes the
port.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`)

- Declarations: the settings group and the three settings (user, *sync by themselves*, server).
  `build.rs` and `native/metadata*` are generated with `tools/abi-metadata.py`. En/de texts are
  the existing ones.
- Model (host-free, each part the interface-6 code made fallible and tested to give the same
  answers): the server address as a base URL, the password key (MD5), JSON strings, the body of a
  progress update (four decimals, ties to even, out of range cut), the server's answer read, the
  decision on a fetched position (`judge`: offer, hold quietly, up to date, with the declined and
  put-off memories and the manual sync) and the rule for a book left behind
  (`send_left_behind`). `tests/native_alloc_refusal.rs` refuses each allocation of the writers.
- The server's JSON is read with `paperpaw_sdk::json` until Kernel's fallible reader (#414) is on
  integration; it has no refusal path, so an answer is bounded first: `MAX_ANSWER` = 4,096 bytes
  (the request's `max_body`; the network host's default would be 45,056 on the X4 and 1,048,576
  elsewhere, `network.md`). `read_answer` and `remote_progress` refuse a longer text with `NoSpace`
  (tested); the reader itself is not in the sweep (marked in `native_model.rs`).
- The settings test (`src/native_parity_tests.rs`) compares each native setting with interface 6's
  field by field (name, title, group, help, kind, default, advanced, kept on reset) and the group.
  The one difference is the editor that opens the account screen: the native plugin declares none.
- `password_key` zeroes its hex array with `mem::wipe` (tested); the MD5 state wipes itself when
  dropped (#451). The 16 raw digest bytes are a temporary inside the digest
  crate's `hex_into` (it consumes the digest): the plugin has no array of them to wipe; wiping that
  temporary is for the digest crate (Kernel). The returned key is the credential, so the sync slice that sends it must
  `wipe_text` it afterwards.

## Slice 2: the sync engine (`src/native_sync.rs`, host-free)

Interface 6's `State` (`lib.rs`) as a state machine with every allocation fallible: which book is
open and where, what was sent and when (at most once a minute, again after a failure), the hold on
sending until the server's progress is read, the offer, *declined* and *not now* (`dismissed`, at
most 4 kept), the books left while their fetch was out (at most 5), the books switched from while
Offline (at most 4), the fetch at most once a minute per book, *Sync progress*. The caller gives
the host's facts (`Ctx`: clock, network, *auto*, signed in, online, radio on anyway) and carries out
the `Action`s: `Fetch`, `Send`, `Offer`, `Notice`, `PushLater` (declare the work). The decision on
an answer is `native_model::judge`/`send_left_behind`. `tests/native_sync.rs` runs the scenarios of
interface 6's `tests/sync.rs` (offline, failed fetch, offer, stay, not now, book left, once a minute,
closing, manual sync, signed out); `tests/native_sync_alloc.rs` refuses every allocation in turn.
Kept for the caller: the book's fingerprint and XPointer, the request and the account, the work
declaration, the device id. `progress_text` is the key of an offered position (`{progress}`; a very
small number in scientific form so it stays short).

The engine reports `NoMemory` and may have pushed some actions already: after an `Err` the caller
drops `out` and carries out none of it (the next event starts again). A `Fetch` or `Send` it cannot
make (the book does not read) is reported back with `not_fetched` and `not_sent`.

Differences from interface 6, all to make it bounded: the books left are capped at 5 (`MOST_LEFT`, the
4 switched from offline and the open one; interface 6 kept any number); the position's place is `(section, offset)` instead of a `Locator`.

## What the slice loses (each returns in a later slice or with its host piece)

- **Every sync**: fetching, sending, the 30-minute and one-minute timers, the offline queue, the
  hold on sending. They need the network host (`core.net`), secrets (the key), the book events and
  the document's position (XPointer): none is on integration for plugins yet.
- The account screen (sign in, create account, sign out, *More about this*) and the setting's
  editor that opens it; the confirmation overlay; the menu entry *Sync progress*; the icons.
- The device id (random seed), the key kept as a secret, moving an old key (`move_key`).

Each loss is an ignored case in `tests/acceptance_pending.rs`.

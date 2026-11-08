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
  integration; it has no refusal path, so its allocation is bounded by the network host's reply
  cap (marked in `native_model.rs`, not in the sweep).

## What the slice loses (each returns in a later slice or with its host piece)

- **Every sync**: fetching, sending, the 30-minute and one-minute timers, the offline queue, the
  hold on sending. They need the network host (`core.net`), secrets (the key), the book events and
  the document's position (XPointer): none is on integration for plugins yet.
- The account screen (sign in, create account, sign out, *More about this*) and the setting's
  editor that opens it; the confirmation overlay; the menu entry *Sync progress*; the icons.
- The device id (random seed), the key kept as a secret, moving an old key (`move_key`).

Each loss is an ignored case in `tests/acceptance_pending.rs`.

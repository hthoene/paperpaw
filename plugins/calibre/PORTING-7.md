# Calibre wireless library on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (session, screen, library bookkeeping, socket
commands) is still in place beside the native one. It is removed in the PR that finishes the port.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`)

- Declarations: Calibre's address setting (library group, advanced). `build.rs` and
  `native/metadata*` are generated with `tools/abi-metadata.py`. En/de texts are the existing ones.
- The wire format (`native_model.rs`, host-free): the interface-6 `protocol` made fallible and free
  of slicing that could panic, each part tested to give the same answers as the old one: frames,
  the framing (any chunking, hostile lengths and bodies refused as `Refused`, a refused allocation
  as `NoMemory`), members of the arguments, the answers to `GET_INITIALIZATION_INFO` (with the
  SHA-1 password hash) and `GET_DEVICE_INFORMATION`, book paths that cannot leave the library
  folder or reach hidden data, a book's metadata without its thumbnail, and Calibre's answer to the
  broadcast. `tests/native_alloc_refusal.rs` refuses each allocation of the writers and the framing.
- Members of the arguments are read with `paperpaw_sdk::json` until Kernel's fallible reader (#414)
  is on integration. It has no refusal path, so what it takes is bounded by the framing's longest
  message (64 KiB on a small device), not by this crate (marked in `native_model.rs`, not swept).

## What the slice loses (each returns in a later slice or with its host piece)

- **The connection**: finding Calibre by broadcast or address, the session, the book list, the
  books received. They need the sockets host and facade (`sockets.md` is a draft), library import
  (LIB `import`), the files facade (#384) and secrets (the password).
- The screen (connection, progress), the tools entry *Wireless library* and its icon, the
  settings section name, "how it works".
- What the library learns from Calibre's metadata (`provided`: title, author, series, tags).

Each loss is an ignored case in `tests/acceptance_pending.rs`.

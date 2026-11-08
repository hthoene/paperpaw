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
- The setting is compared with interface 6's field by field (`src/native_parity_tests.rs`: name,
  title, group, help, kind, default, advanced, kept on reset). The one difference is the section
  the setting is shown in (`calibre.plugin.name`): the native plugin declares none yet.
- **Gate before the session slice is wired:** `member`, `text`, `strings` and `without_member` read
  with `paperpaw_sdk::json` (`raw_member`, `unquote`, `string_array`), which allocates without a
  refusal path and without a bound of its own: a hostile 64 KiB key costs about 200 KiB for one
  message. The session slice must use Kernel's fallible reader (#414) or a bound proven on the X4
  first. These readers are not in the refusal sweep (`tests/native_alloc_refusal.rs`).
- `initialization` zeroes the hash's hex array and its quoted copy with `mem::wipe` (tested); the
  SHA-1 state wipes itself when dropped (#451). `Device`'s `Debug` never shows the password
  (tested). The raw SHA-1 bytes are wiped after the hex is made (the digest crate's own
  `finish()` temporary stays unwiped, as for kosync; Kernel todo 140 tracks it), and the quoted copy of the hash is
  zeroed in place (tested). The answer carries the hash by design; the sending slice must
  `wipe_text` it after the write.
- **Removal checklist:** the interface-6 `protocol::Device` derives `Debug` and holds the password;
  it goes when interface 6 is removed in the final slice (the native `Device` hides it).

## What the slice loses (each returns in a later slice or with its host piece)

- **The connection**: finding Calibre by broadcast or address, the session, the book list, the
  books received. They need the sockets host and facade (`sockets.md` is a draft), library import
  (LIB `import`), the files facade (#384) and secrets (the password).
- The screen (connection, progress), the tools entry *Wireless library* and its icon, the
  settings section name, "how it works".
- What the library learns from Calibre's metadata (`provided`: title, author, series, tags).

Each loss is an ignored case in `tests/acceptance_pending.rs`.

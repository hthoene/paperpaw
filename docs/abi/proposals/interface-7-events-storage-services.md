# Interface 7: events, storage, services and text contracts for the Battery port

Status: DRAFT proposal for the Kernel Engineer to review and own (#228, #745).
Nothing here is decided or implemented. Base: `origin/abi7/integration` 23086bb9.

Conventions. **[V]** is a verified fact with a `path:line` citation (lines on the
base above unless a ref is named). **[P]** is a proposal. **[Q]** is an open
question. Plugin-side facts come from `origin/samples/abi7-snapshot-settings-734`
(`plugins/battery`, `plugins/battery/tests/PORTING.md`). ADR 0034 is read from
`origin/fw/adr-flat-boundary-228:docs/adr/0034-stable-plugin-boundary.md` (cited
as ADR34:line).

Out of scope: declarative registration (manifest, translations, setting specs,
resources, actions, menus, icons, screens) is in
`docs/abi/proposals/declarative-registration.md` (another document, referenced by
name only). The slot 15 `command.set_setting` record (tag 4) is in progress on
Kernel's commands branch; its `docs/abi/commands.md` was not reachable from this
worktree (`origin/kernel/abi7-scoped-commands-228` does not exist on origin when
checked), so its content is taken from the coordinator's message only.

## 0. Shared rules used by all four sections

Verified starting point:

- [V] 20 host slots exist, 0 to 19 (`abi/abi.def`): heap 0-2, clock 3, env.snapshot 4,
  settings.read 5, i18n.copy 6, font.acquire 7, handle.release 8, canvas 9-12,
  text.measure 13, text.draw 14, commands.submit 15, registrations.submit 16,
  services.query 17, blob.read 18, blob.release 19.
- [V] Argument types are closed: `context pointer bytes buffer handle usize u16 u32
  u64 i32 f32 f64 out_ptr out_u64 out_handle out_f32` (`tools/abi-gen.py:21`). There is
  no `i64`, `u8` or `bool` argument. Wire field types are `u32 u64 i32 str bytes`
  (`tools/abi-gen.py:27`): no `i64` in records either. Each entry has one
  `max_bytes` (`abi.def`, `abi-gen.py:123`).
- [V] The record codec generator accepts only the three initial shapes
  (`registration.event`, `registration.draw`, `event.tick`):
  `tools/abi_records.py:10-14,50` ("unsupported initial record fields"). Every record
  proposed below needs generator work (str fields, more shapes, more tags).
- [V] Batch grammar: LEB128 count, then records `tag, len, body`; body = bitmap length,
  bitmap, positional fixed-width little-endian scalars; cap 4096 B and 8 records
  (`docs/abi/records.md`, `REGISTRATION_MAX_RECORDS`
  `crates/paperpaw-core/src/native_host/registrations.rs:17`).
- [V] Statuses: OK 0 and `ERR_*` -1..-9 (`abi/abi.def` `statuses`). The set is open and
  appended (`abi/README.md`).
- [V] The host table is append-only; a new slot is added at the end with `since` set
  to the unreleased minor 1 (`abi/README.md`, "Multiple additions may use the same
  unreleased minor"). ADR34:~"Entries are only appended".

Proposed shared rules **[P]**:

1. **Record tags.** Tags 1-3 exist, 4 is `command.set_setting`, 5-31 belong to the
   declarative-registration document. This document uses **32-63** only:
   events 32-43, i18n argument records 44-47, service response records 48-55,
   command records 56-62, 63 reserved. Tags are never reused.
2. **Event header.** Every event record's first required field is `now_ms: u64`
   (monotonic milliseconds at emission, same clock as `env.snapshot.monotonic_ms`),
   matching `event.tick {now_ms}` (`abi.def` record 3).
3. **New statuses** (appended): `ERR_NOT_FOUND -10` (no such file or key) and
   `ERR_FULL -11` (medium full or free-space reserve reached). Reason: `ERR_NO_SPACE`
   already means "output too small / batch too large" (`docs/abi/records.md`), so reusing
   it for a full card would be ambiguous. Mapping from the SDK's `StorageError`
   (`crates/paperpaw-platform/src/storage.rs:17-38`): NotFound to NOT_FOUND,
   InvalidPath/InvalidData to INVALID, Unsupported to UNSUPPORTED, Full to FULL,
   Io/TooLarge/Other(_) to IO (`CARD_GONE` is `Other(1)`, storage.rs:45; **[Q]** whether
   it deserves its own status).
4. **Validation before effects.** Every slot validates all pointers, lengths, UTF-8,
   path grammar, caps, owner and root before the first side effect; a refused call
   leaves output buffers untouched (as `docs/abi/snapshot.md` and `abi/settings.md` already
   require).
5. **No panics, no unbounded allocation.** Every variable-length input has a cap below;
   host scratch is stack or fixed.
6. **Closed schemas.** Unknown record tags in a position that requires support return
   `ERR_UNSUPPORTED`; only bounded appended optional fields extend a record
   (`docs/abi/records.md`).

## 1. Event payload grammar

(placeholder, extended below)

## 2. Storage slots

(placeholder)

## 3. services.query, plugin names, usage and work notes

(placeholder)

## 4. Text, font, i18n and command vocabulary

(placeholder)

## 5. Summary, open questions, estimates

(placeholder)

# Slot 15 command batches: SetSetting

`abi/abi.def` owns the record tag, field order, caps and the closed
`command_codec` descriptor. `tools/abi_commands.py` validates them and writes
identical modules to `abi/generated/commands_wire.rs` and
`crates/paperpaw-core/src/abi7/commands_wire.rs`. The module sits beside the
generated settings value codec and uses it for values. Lock format 7 pins the
emitted codec body.

## Wire

`commands.submit(ctx, batch)` takes one batch of at most 4,096 bytes. The batch
uses the framing in [records.md](records.md): a canonical u32 LEB128 count, then
records of tag, body length, presence bitmap and fields. Byte fields are a
canonical u32 LEB128 length followed by that many bytes.

| Tag | Record | Fields, in order |
| --- | --- | --- |
| 4 | `command.set_setting` | `key: str` (1 to 128 bytes, UTF-8), `value: bytes` (one complete settings value) |

`value` is exactly one value of the [settings value contract](../../abi/settings.md),
the same encoding slot 5 returns: Boolean, integer or text. An absent value is
refused; resetting a setting would be a separate command. A batch holds at most
8 commands.

Decoding statuses: an unknown command tag or an unknown value layout/kind returns
`ERR_UNSUPPORTED`. Malformed framing, an empty key or an absent value returns
`ERR_INVALID`. Invalid UTF-8 in a key or text returns `ERR_UTF8`. A batch, count
or key over its limit returns `ERR_NO_SPACE`. Unknown bitmap bits and trailing
fields inside a known record are accepted, as for registrations. The decoder
allocates nothing and retains no input. `commands_encode` checks every key,
value, limit and the output capacity before writing; on error the output is
unchanged.

## Host

`native_host::commands::CommandScope` is a pinned, single-threaded slot in the
plugin's host context. The host lends a `CommandStage` to it for exactly one
synchronous callback, as with the canvas. The stage holds the submitting
plugin's identity, which the host knows (never a claim in the bytes), the live
settings registry, and a limit on accepted commands.

`submit` decodes and validates the whole batch before staging anything:

1. Outside a callback it returns `ERR_STATE`; during another submission,
   `ERR_BUSY`. A `draw` callback is a render only and is lent no stage, so
   `submit` there returns `ERR_STATE` too (interface 6's `Screen::draw` had no
   `Cx` either); a plugin asks for a redraw or anything else from its input,
   event or show callbacks. The host drains a stage only when its callback
   returned a valid non-negative result. A callback that returns a negative
   status (or a value its family does not allow) loses everything it staged,
   even commands `submit` accepted with `OK` (`screens-input.md` 2.1).
2. Codec statuses as above. Nothing is staged.
3. If the batch would exceed the stage's command limit: `ERR_NO_SPACE`. One callback stages at
   most 64 commands and 4,096 bytes of keys and text over all its batches (the host's
   `CALLBACK_COMMANDS` and `CALLBACK_COMMAND_BYTES`, the same on every device).
4. For every command, without allocating: the setting must be registered, and
   Boolean and integer values must be accepted by its spec
   (kind and range). An unregistered or unaccepted setting returns
   `ERR_INVALID`, as does another plugin's guarded setting. The host-resolved
   settings editor and `core` are exempt from the guarded restriction. A secret setting returns `ERR_UNSUPPORTED`; secrets keep
   their own path. The bytes of keys and text staged in this callback must
   stay within the stage's byte budget, or the batch returns `ERR_NO_SPACE`.
   A setting a card plugin describes on demand is read from the plugin for
   this check: without room for that, the batch returns `ERR_NO_MEMORY`,
   never `ERR_INVALID`.
5. The stage reserves room for the whole batch fallibly (`ERR_NO_MEMORY`), then
   converts each command, copying text with a fallible reservation and
   checking it against the spec. If any conversion fails, the commands of this
   batch are removed again and the error is returned.

A batch is staged completely or not at all; batches accepted earlier in the same
callback stay. Staged commands are host-private: the kernel does not see them
before the host drains the stage after the callback (`into_commands`). The
kernel then applies them as `Command::SetSetting`. That includes book scopes,
a setting's own check and the change event. The normal plugin command path
refuses another plugin's guarded settings, with the same settings-editor and
kernel exceptions as interface 6, as defence in depth after slot 15's check.
The host obtains `Kernel::command_settings_authority` for the submitter before
lending each callback stage and passes it to `CommandStage::new`. The token
must never survive that callback: binding or uninstall can change the editor.
It uses the same
`Reserved` authority rule as `vet_reserved`; `None` exempts nobody.
The host-known identity records who submitted;
it does not restrict keys to that plugin's namespace. The next callback reads
the new value back through slot 5.

Queue space and text are reserved fallibly. Setting ids are not: a key equal
to the registered id clones it, and a placement key is parsed with `Id::parse`,
both through the global allocator. Each copy is bounded by the 128-byte key cap,
and keys and text together are bounded by the byte budget the host gives the
stage. Without that budget, 8 commands of about 4 KiB text each could retain
about 32 KiB per callback; a profile sets it within its boundary memory budget.

## Not covered

No other command kinds, and no `Notify`. Slot 15 allows writing another
plugin's unguarded, non-secret settings, as decided in owner decision 126
of 2026-11-06. This gains a capability inside the interface-7 draft; no existing
status changes meaning and the wire is unchanged. Foreign guarded settings
still get `ERR_INVALID`; only `OK` gains foreign unguarded settings. The scope is not installed
in a combined host table yet; activation, loader wiring and device qualification are separate gates.

```sh
python3 -m unittest discover -s tools/tests -p test_abi_commands.py -v
cargo test -p paperpaw-core --test abi7_scoped_commands
```

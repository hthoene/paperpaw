# Interface-7 port

Version 1.0.8. Native plugin: `src/native_plugin.rs` (`NativeKeys`), registry
reads in `src/native_data.rs`, the decisions that need no host in
`src/model.rs` (places, sections of inputs, what a change may take away). Built-in
descriptor `NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs` built
into the image by `build.rs`, metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`). Drawing uses `paperpaw-ui-kit` (`ItemList`).

## Ported

- List of inputs (place, gesture policy, sections for the inputs this device has,
  each with what it does; tap zones as a small picture), place chooser (places,
  tools that set a gesture policy, every other screen told apart by kind), action
  picker (groups, search with the system keyboard, *As standard*, *Nothing*).
- Bindings and inputs from the registry, changes with `bind_key`; the host decides
  what a binding is. *Reset* asks first (host question with the original labels) and
  takes back every user binding. Buttons-only devices refuse a change that leaves no
  button to choose, go back or move (message `keys.needed.*`), as before.
- en/de texts unchanged.

## Tests

Unit (`model`): bindings by layer order, sections for a buttons-only and a touch
device, the buttons-only refusal, places of the same name, tap-zone pictures; kit:
item list focus/paging/taps, word wrapping. Declaration parity with the interface-6
registration (`tests/abi7_declarations.rs`), linked entry.

## Acceptance pending on the host

There is no desktop golden for this plugin. `tests/keys.rs` and `tests/remap.rs`
pin its behaviour with the interface-6 plugin on a kernel. The native cases in
`tests/acceptance_pending.rs` are refusals (never pass): rebind applies (H1, H2,
H3, H4), buttons-only refusal (H1, H3, H4), reset all (H1, H2, H4).

## Not ported yet (explicit)

- Action icons in the picker (the registry rows carry none).
- A tool's own tap-zone layout (`[zones.<layer>]`) is not drawn; the nine standard zones are.
- Legacy `Plugin` registration and the app's `offered(.., Keys)` stay until the host activates
  native plugins; then they go together (no shim).

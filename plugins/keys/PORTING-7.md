# Interface-7 port

Version 1.0.9. Native plugin: `src/native_plugin.rs` (`NativeKeys`), registry
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
  button to choose, go back or move (message `keys.needed.*`), as before. A refused
  binding only notifies: the picker stays open on the same input (no `back`), nothing
  is bound.
- en/de texts unchanged.

## Tests

Unit (`model`): bindings by layer order, sections for a buttons-only and a touch
device, the buttons-only refusal, places of the same name, tap-zone pictures; kit:
item list focus/paging/taps, word wrapping. Declaration parity with the interface-6
registration (`tests/abi7_declarations.rs`), linked entry.

## Acceptance pending on the host

There is no desktop golden for this plugin. The native cases in
`tests/acceptance_pending.rs` are refusals (never pass): rebind applies (H1, H2,
H3, H4), buttons-only refusal (H1, H3, H4), reset all (H1, H2, H4).

## Goldens to re-bless at activation

While the native plugins are not registered in the app, the settings list has no
*Buttons and gestures* row. `crates/paperpaw-app/tests/snapshots/settings.txt` and the
desktop golden `boox-settings.png` were changed for that (a capability-loss
placeholder, todo 97). When the host activates native plugins they must be restored
(the row comes back with `keys.open`) and compared against Beta 3.

## Not ported yet (explicit)

- Action icons in the picker (the registry rows carry none).
- A tool's own tap-zone layout (`[zones.<layer>]`) is not drawn; the nine standard zones are.
- Legacy `Plugin` registration and the app's `offered(.., Keys)` stay until the host activates
  native plugins; then they go together (no shim).

## Robustness (review of PR #317)

- `core.ui` opens in the first live callback, not in `make` (construction has no
  frame); `tests/hosted_construction.rs` constructs in the kernel's real registered
  scope. The host admits every declaration the plugin makes (the registry-change
  event, 75, since #916), so `make` runs to the end there: a real pass, no pin.
- Every allocation on the callback paths is reserved first (`paperpaw_ui_kit::mem`,
  `text::concat`); allocator-failure sweeps (`plugins/test-support/fail_alloc.rs`)
  refuse each allocation in turn for the model/store code and the kit and require
  `NoMemory` and recovery. Views and dialog state that call the host are not covered
  by a sweep: that needs the host's UI service (H1-H4).
- `core.ui` calls use 4096-byte buffers (the schema's reply capacity). Registry
  readers allocate their buffers before opening the handle.
- Peak memory on the X4 (buffers, paint operations, rows) is not measured: device gate open.
- Search words are read whole (all parts, at most 256 bytes) or the dialog's answer is
  refused with `NoSpace`; nothing is searched on a prefix.
- The interface-6 plugin is removed: `Keys`, the legacy screens in `lib.rs`, the kernel tests that pinned them (`keys.rs`, `remap.rs`), the app's `offered(.., Keys)` wiring and
  `STORE_PLUGINS` entry, and the plugin-file build (`tools/plugin-files` bin, `plugins.txt`).
  It does not run in the app until the host activates native plugins. The behaviour it pinned
  is covered by the model/store unit tests above; the golden stays for the native acceptance.

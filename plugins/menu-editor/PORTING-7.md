# Interface-7 port

Version 1.0.8. Native plugin: `src/native_plugin.rs` (`NativeMenuEditor`),
built-in descriptor `NATIVE_ENTRY` (feature `linked`), declarations in
`declarations.rs` (built into the image by `build.rs`), metadata in `native/`
(`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, and the same
for `metadata.host.*`). Drawing uses `paperpaw-ui-kit`.

## Ported

- Four screens (menus, entries, entry options overlay, add) with the interface-6
  behaviour: submenus listed after their menu, *Changed* marks, *Reset all* /
  *Reset this menu*, protection of Settings and this editor, no submenu circles,
  candidates sorted by name with "taken out by <plugin>" notes.
- Registry reads through typed pages (one generation per snapshot), edits through
  `set_menu`; rows come from the host's answer after `RegistryChanged`.
- Parameters travel in handovers (`menu`, `index`); the one overlay screen is
  declared `overlay`.

## Tests

- Unit: menu graph, ordering, reachability, edits (property tests), navigation
  state, overlay input rules; kit list/input rules.
- `tests/abi7_declarations.rs`: ids and titles equal the interface-6 registration.
- `tests/linked_entry.rs`: descriptor and metadata.

## Acceptance pending on the host

| Case (golden) | Needs |
| --- | --- |
| `boox-menu-drag` (open, list, entry options) | H1 screen commands, H3 composition, H4 input |

Pending cases are `#[ignore]`d refusals in `tests/acceptance_pending.rs`; they never pass.

## Not ported yet (explicit)

- Dragging an entry by its grip (interface 6 `Reorder`): needs pointer contact
  forwarding to plugin screens (H4). Moving an entry is possible through its
  options (*Move up*, *Move down*), as before.
- Soft focus style (`core.focus.soft`): `Style::read` refuses `Unsupported`
  because the snapshot gives no display gray levels.
- Legacy `Plugin` registration and the app's `offered(.., MenuEditor)` stay until
  the host activates native plugins; then they are removed together (no shim).

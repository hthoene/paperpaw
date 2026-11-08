# Interface-7 port (slice 1: the declarations)

Version 1.0.2. Partial: what the plugin declares, without the host. No native screen and no
`NATIVE_ENTRY` yet; the interface-6 `PictureHome` and everything of it stays until the host
activates native plugins (then it goes together, no shim).

## Ported

- `declarations.rs`: the group (order 221, `layout-grid` icon), the 28 settings (the folder by the
  folder chooser slot, the picture by its own picker, the 4 pages by the dashboard, the bar's
  alignment shown only for the compact bar, hints as an advanced setting), the three screens, the
  `core.home` offer at the priority of a replacing plugin, the pages as a place of widgets in a
  grid, the bar's place of status items, and the no-touch need of the focus setting.
  `tests/abi7_declarations.rs` compares each with the interface-6 registration.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The screens (H1, H3 compose, H2/H4 input), the picture's decode and scale (image slots 36/37,
  `core.files`), and the bars drawn through the host's status frame.
- The bar's three item settings are edited on the screen `status-items` of this plugin
  (`EditorDecl::Screen`, one screen for the three, opened with the setting's id), built on
  `paperpaw_ui_kit::status_items` as home's footer items are (Kernel's ruling, todo 121). The
  declaration is compared with the interface-6 registration (only the editor's name differs); its
  callbacks wait for the native entry (H1/H4), and the finger's drag of a grip is a hold and a tap
  in the kit (`KIT-grip-drag`).
  The native entry will need `uses core.registry` in its metadata: the view reads the registered
  items with the kit's `status_items::view::read_entries`.

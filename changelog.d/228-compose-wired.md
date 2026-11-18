### Added

- **Slot 35 `ui.compose` is wired into the core frame** (#228). A plugin's `draw` can now draw a widget instance, status item or status line of another plugin into its own canvas: the handle is resolved in the device's `core.ui` table, the owner's `draw` runs on its own instance one level deeper (a child cannot compose again), and a failing child is told it was dropped while the caller's frame stays. A stack the frame cannot read hides everything below the guard. The X4 still answers `ERR_UNSUPPORTED` until its stack gate passes.

### Added

- **`core.registry` lists menus, key bindings, setup steps, widgets and status**
  (#228). The host side of `core.registry` now answers `menus` (every menu id,
  `flags` bit 5 where the user has a definition), `menu` (the rows the host
  would show now: the user's own definition, else the declared rows without
  hidden ones and ones the device cannot offer; `all` adds the hidden and the
  removed rows, `declared` ignores the user's definition; checked, enabled,
  switch, label and badge from the item's state), `bindings` (by layer or for
  all, with where each comes from), `setup` (every step, with `flags` bit 0 where it applies now),
  `widgets` (with kind, icon, cells and whether a place that shows now lists
  them) and `status` (status items and lines natively declared; empty where
  none is). A scope that names nothing is an empty page, as for the other
  lists. Settings, presets, values and `needs` follow.

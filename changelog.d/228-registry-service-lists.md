### Added

- **`core.registry` answers its first ops** (#228). The host side of
  `core.registry` reads the plugin list (ops 5 and 6, the host first), the
  actions, screens, setting groups, slots and slot offers (ops 4, 7, 8, 9, 10)
  and the inputs a device and its plugins have (op 14), paged as the contract
  says (up to 32 whole rows and one `end`, a generation that never is 0, a
  stale one is `ERR_STATE`). Settings, presets, menus, bindings, setup, widgets,
  status and `needs` follow in later slices.

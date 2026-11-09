### Added

- **The registry's menu list tells which menus the user redefined** (#228). `core.registry` op 11 (menus) will set `flags` bit 5 for a menu the user has a `command.set_menu` definition for, also an empty one or one equal to the declared rows, and clears it after a reset. The host side is in place and tested for all four states; it lists the user's own menus without allocating.

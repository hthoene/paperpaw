### Added

- Native plugins can change a screen's policy at run time with `command.screen_policy`: `guards`, `secret` and `holds_edges` on their own screens. `guards` can be turned on only for their own top screen. The kernel applies the change before the callback's other commands, and it stays when the screen is made anew.

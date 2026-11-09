### Fixed

- Hints: the plugin no longer registers for `event.screen_shown`, which the host refuses, so it can be installed; it follows other screens through `event.screen_ready` and `event.stack_changed`.

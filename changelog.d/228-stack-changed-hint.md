### Added

- **`event.stack_changed` (81) is raised** (#228): a plugin that subscribed to it
  hears a hint after the screen stack changed (a screen opened, closed or came
  to the top), at most one pending per plugin, between callbacks. It carries
  the time only; the plugin reads the stack with `core.ui` op 1 (`ui.md` 5.1). A plugin that
  starts subscribing is not told of earlier changes.

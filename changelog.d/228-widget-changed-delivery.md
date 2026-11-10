### Added

- A widget's owner can say an instance shows something new with `command.widget_changed` (its own instances only), and the plugin whose screen made the instance gets `event.widget_changed` (86) between callbacks, once however often it was said, while the instance lives.

### Changed

- **Home (interface 7): a widget that changed is redrawn at once** (#243). Home subscribes to `event.widget_changed`, so a widget's new content shows on the home screen without waiting for the next minute tick.

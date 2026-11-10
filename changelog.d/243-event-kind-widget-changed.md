### Added

- **`EventKind::WidgetChanged`** (#243). A native plugin can subscribe to `event.widget_changed` (86) in its declarations, so a widget's owner refreshes its own view when an instance says its content changed.

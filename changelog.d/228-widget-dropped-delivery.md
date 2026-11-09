### Added

- **A widget's maker hears of its drop** (#228). When the screen that showed a widget closes, the plugin that made it gets `event.widget_dropped` at the widget's token, once: the entry ends at the first delivery attempt whatever came of it, and at once when the plugin cannot be called (stopped, unloaded, switched off), so a dead plugin never takes up room in the widget table.

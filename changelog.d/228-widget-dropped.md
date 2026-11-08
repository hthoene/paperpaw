### Changed

- **Widget owners hear of dropped instances** (#228). When a widget instance goes (its maker dropped it, its screen closed or its maker stopped), the plugin that declared the widget gets `event.widget_dropped` (85) at its widget's token, so it can free what it kept for the instance. A drop waits until the first delivery attempt is over, whatever its outcome, and is dropped at once when its owner stops; live instances and undelivered drops share the widget cap, and at it no new instance is made (`ERR_FULL`).

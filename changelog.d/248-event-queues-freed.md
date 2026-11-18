### Changed

- Native plugins: a plugin's pending-event queue is freed as soon as its events are told, so an idle device holds no event queue (about 2 KiB less heap with all plugins linked). Delivery order and limits are unchanged.

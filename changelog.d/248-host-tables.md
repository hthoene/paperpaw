### Changed

- **Less heap held by the native host while it is idle** (#248). The event pump keeps a buffer of 256 bytes, not 1,280, and makes the larger one only for the pass that has an event of that size; the places of the `core.format` calls (1 KiB) are made at the first call; the lookup and `core.ui` services are boxed where they are wired; and `Kernel::start` gives back the room the registries and the host's tables kept while the native plugins were hosted one by one (the plugin list, the menus, the subscriptions, the entries, owner rows, setting queues and linked ids).

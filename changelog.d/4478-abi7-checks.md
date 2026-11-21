### Fixed

- **Plugins: a batch of commands without a menu change asks the menus nothing** (#4478). Checking a batch for submenu cycles read the menus even where the batch changed none; that took memory unasked and could stop the device when memory was short.

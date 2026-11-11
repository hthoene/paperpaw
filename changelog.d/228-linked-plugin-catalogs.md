### Fixed

- **A linked native plugin's texts are found** (#228). A plugin built into the program has no plugin file to carry its translation files, so nobody added them: its titles and labels (the keyboard's, the menu's) were not found, and the screen failed. `paperpaw_app::add_native_catalogs` adds the `i18n` files of every linked plugin to the translator before the start (`register_native_plugins` does it itself); `Kernel::add_catalog` and `Kernel::reserve_catalogs` are the kernel's two calls.

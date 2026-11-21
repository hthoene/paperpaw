### Changed

- `paperpaw-dev check` looks at every first-party plugin: Browser, Edge light, File sync, RTF, Joplin, KOReader sync, Picture frame, Read later, Readwise, Wallabag and Wikipedia are in the emulator's app (switched off, as store plugins ship), a store plugin named with `--plugin` is switched on by the check, the screens that need a parameter or a question (`keyboard.ask`, `transfer.phone`, `settings.secret`) are measured, and the declared ways to the store's letters and the font page work without `--network fixtures`.

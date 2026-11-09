### Added

- **A plugin's own pictures now run** (#228). `command.image_prepare` and `command.image_release` are admitted in the command stage (root 1 needs `uses.storage`, at most the plugin's room) and applied after the callback; the kernel decodes **one request a turn** between callbacks and tells the plugin `event.image_ready` (never inside a callback). The cache folder is swept once before the first decode. A request the table cannot take for now is kept (at most 16) and applied at a later pass.

### Changed

- **The plugin id `core` is the host's own** (#228). A card plugin's `plugin.ini`, a plugin
  bundle, the store's and transfer's installers and the registration of linked or native plugins
  refuse it, so no plugin's action can pass for a host `core.*` action while the lock is up.
  Ids such as `core-tools` stay free. A card manifest's id may also be at most 64 bytes.

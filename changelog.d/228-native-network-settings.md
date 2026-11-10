### Added

- **A native plugin that uses the network gets its network settings** (#228). Every admitted native plugin with the feature `uses.network` gets `core.network.<id>` and `core.saving.<id>`, the same specs, ids and defaults as an interface-6 plugin, with the reader's kept values applied, and the network policy of `core.net` and `core.sockets` follows them. A setting that cannot be defined is a warning, never a refusal of the plugin.

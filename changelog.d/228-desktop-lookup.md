### Added

- **The desktop and emulator wire `core.lookup` for linked plugins** (#228). `register` names `HostWiring::lookup()`: a linked plugin that needs `core.lookup` is admitted and its lookups reach the providers other linked plugins declared. Nothing else changes on the desktop; `native-linked` stays off by default.

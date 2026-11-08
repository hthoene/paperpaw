### Added

- **Native SDK: the UI language tag** (#228). `native::i18n::language` reads the complete UI
  language tag from the `core.language` setting into a buffer the plugin owns, without
  allocating.

### Changed

- **A format declaration is encoded by its own record type** (#228). `Declarations::format`
  hands the typed `FormatSpec` row to the encoder instead of the generic record enum,
  which linked the encoder of every record into a plugin that declares a format. Nothing
  a plugin calls changes.

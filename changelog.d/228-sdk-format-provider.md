### Added

- **Format provider requests and replies in the native SDK** (#228). A plugin that
  reads a file format through `core.format` decodes a stage request with
  `native::format::FormatCall::decode` and encodes borrowed metadata, text,
  preparation state and scalar replies into caller-owned output, without heap
  allocation. The `format.spec` declaration is `Declarations::format`.

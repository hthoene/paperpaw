### Added

- **Reading and status font roles in the native SDK** (#228). `native::fonts::FontRole` gains `ReadingBold`, `ReadingItalic`, `ReadingBoldItalic` and `Status`, which acquire `role:reading-bold`, `role:reading-italic`, `role:reading-bold-italic` and `role:status` as `docs/abi/screens-input.md` section 4 contracts; the host falls back to the reading font (bold italic to bold) where none is bound.

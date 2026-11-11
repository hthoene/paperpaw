### Added

- **`Declarations::reader` in the SDK** (#243): a native plugin declares a reading app (`ReaderDecl`: name, its own screen, the formats it reads; `reader.spec`), as interface 6 `Registrar::add_reader` did.

### Fixed

- **The native reader is known as a reading app** (#243). It declares `reader.app` with its reading screen, so the host counts the books it opens: reading progress, "Continue reading" and the book events work again.

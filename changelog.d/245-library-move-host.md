### Added

- **A plugin can move a book in the library on the device** (#245). `core.library` `move` (op 11) hands the move to the card's journalled move job: the file, its details, its position, its annotations and the plugins' book data follow it. A name that a document, the reader, a transfer or the move job holds answers `ERR_BUSY` and nothing is written.

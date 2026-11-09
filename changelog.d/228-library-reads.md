### Added

- **The library service reads the catalog on the card** (#228). `core.library` answers state, book, text, position, annotations, anchors, fields and the book key (and op 2's SDK page) over `paperpaw_sdk::library`'s fallible reads: a short heap is an error, never an abort, and a book's file is looked up for the `missing` flag only for the one book asked.

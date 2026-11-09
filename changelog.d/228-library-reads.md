### Added

- **The library service reads the catalog on the card** (#228). `core.library` answers state, book, text, position, annotations, anchors and the book key over `paperpaw_sdk::library`'s fallible reads: a short heap is an error, never an abort, a book's change log is read once and its line in one scan, and a book's file is looked up for the `missing` flag only for the one book asked. Ops 2 (query, facets) and 22 (fields) still answer `ERR_UNSUPPORTED` in production for now; their host sides follow.

### Added

- **`core.library` op 2 `query` on the production host** (#228). The books a plugin lists (the home shelf, the recent books, the files list) are found on the card: show, words, author, series, tag, folder and day filters, the five orders (a tie is the path), descending, and paging by cursor under the catalog generation. The catalog is read a line at a time and nothing the size of the result is held, so the home screen no longer gets "unsupported" for its start-up reads. Not yet: the book badges and the archive setting's value are the shells' (`lib_entry.badges` is empty).

### Added

- **Favourites and book metadata in the native SDK** (#228). `Library::set_favourite` (LIB op 8,
  status bit 1) mirrors `set_finished`. `Library::book_text` with `BookTextQuery` reads a book's
  whole title, author, series, tags and plugin fields in parts (LIB op 4). `Documents::metadata`
  and `Documents::text` with `DocTextQuery` read a document's title, language, series,
  direction, the counts of authors and subjects, and every text whole, description included
  (DOC ops 3 and 4).

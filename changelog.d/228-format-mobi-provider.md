### Changed

- **Mobipocket books get their interface-7 format provider** (#228). The
  `format-mobi` plugin can now answer the host's `core.format` calls (open,
  preparing the book in steps, metadata, text blocks, contents, links, covers and
  pictures). Its reading of a book no longer aborts when memory is short, and a
  card that fails in the middle of a book is now an error instead of the book's
  end.

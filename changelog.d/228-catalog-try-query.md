### Added

- **A page of the catalog's query without an abort on a memory shortage** (#228).
  `catalog::try_query` and `try_query_in` find the same books as `query` and
  `query_in`, in the same order, and reserve the page they hold at once: a
  device short of memory gets `data::Error::OutOfMemory` for the page.

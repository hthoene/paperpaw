### Changed

- **`core.library` op 22 `fields` pages** (#228). A book's fields come in
  pages of whole rows under the book's field generation, as annotations do:
  a later page with a stale generation is `ERR_STATE`, and a book with many
  fields no longer needs them to fit one reply.

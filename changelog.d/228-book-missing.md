### Added

- **`native::library::Book::missing`** (#228). A plugin reading a book with library op 3 can tell
  that its file is gone from the card (`lib_book.flags` bit 8) without `uses.storage`, so home's
  recent shelf can leave such books out as Beta 3 did. Interface 7 draft 34.

### Fixed

- **Pages in a book with several languages turn as quickly as the
  others** (#483). Where a third language had no room for its hyphenation
  table, the device read the card for every word of it on every page;
  it now tries again only once there can be room.

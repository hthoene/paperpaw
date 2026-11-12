### Changed
- Less memory while the library reads a book's details on its own: the host holds a reply buffer sized to what the format answers (a book's state, its details) instead of 4 KiB (#248).

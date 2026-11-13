### Fixed

- **Bookmarks, highlights and the reading position are kept in a library's first session** (#228, #257). The native reader keeps them through the library service, which looked the book up in the catalog, and the catalog is made only once a view of the library asks for it. So a book opened before then could keep no mark ("could not be kept"). Its own data (annotations, their texts and anchors, the position) is kept by its path, as in Beta 3, when the card holds the file and the catalog does not know it yet; what the catalog answers (status, fields, details) still needs it.

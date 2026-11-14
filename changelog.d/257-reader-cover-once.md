### Fixed

- **Reader: a book that opens on a picture draws its page once** (#257). The native reader drew the opening page a second time before its picture, and asked for the picture's box twice, so the cover came in an extra full-screen refresh. Now the page is drawn once and only the picture's box follows, as on a page turn.

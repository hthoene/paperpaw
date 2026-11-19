### Changed

- **More memory while reading a book opened from the library** (#344). The kernel now tells a native screen when a full screen covers it (`event.screen_hidden`), and the library list gives its covers back while a book is open over it. It asks for them again when it shows, as it did before.

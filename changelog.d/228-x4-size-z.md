### Changed

- **The X4 firmware is built smaller** (#2575, #2593). Everything but the code a page turn, a book's opening, the start and the radio lean on is built for size (`opt-level = "z"`): the owner's test image is about 330 KB smaller. The text, layout, picture, archive and FAT code, the drivers, the allocator, the start's checks, the reader and the shell itself stay as before. The device's speed (page turn, opening) is measured before this goes into a release.

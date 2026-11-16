### Fixed

- Turning a page in the reader asks the font for far fewer letter widths: they are kept for the whole book (about 2 KiB while it is open) instead of being asked again for every page. In the emulator's 30 page turns of a sample book the font was asked 9 times a turn on average (41 at most), against 137 (224 at most). If there is no room for the table the reader goes on without it.

### Changed

- **Back to Home is faster** (#267). The home screen keeps the page it laid out while a book or another screen is over it and shows it again at once, where it laid out every widget anew at every return (about 2.7 s of 4.4 s on the X4). It is laid out again only if a setting or a plugin changed meanwhile, the screen was turned, or the recent shelf came or went; what a book changed (the book on the card, its progress, the recent books) shows as before. The page costs about 1.3 KiB of memory while a book is read.

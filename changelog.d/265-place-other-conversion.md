### Fixed

- **A book keeps its reading place after an update** (#265). A place saved for another conversion of the book, for example by Beta 3 or with another picture size, was dropped and the book opened at its start. The reader now takes the place if its progress still fits the stored one, else the place that progress is; a place it cannot check opens at the start. Nothing is written to the card when the book opens.

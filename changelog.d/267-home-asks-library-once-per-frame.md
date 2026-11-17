### Changed

- **Home asks the library for its books once for each frame, not once for each band of it** (#267). On the X4 a library query is some twenty card reads, and the first frame of Home after "Back" asked five times (the card and the shelf, band after band) and made 1.3 s of its 2.4 s of host calls. The card and the shelf now keep what the library listed for the bands of one frame and ask again for the next frame and after every event, tap or tick; a cover that is ready stays known when the catalog's generation changes (a book was read) and the tick finds the books changed.

### Changed

- **USB developer tools row: a shorter description** (#12), so it fits its two lines in English and German.
- **The store plugin fits the S3's frame limit again** (#214): the page it installs from and the dialog buffers live on the heap.
- **Translations past the read limit are read in a small window**, not by a 2 KiB block, so the peak of a book in Thai or Hindi stays where it was.

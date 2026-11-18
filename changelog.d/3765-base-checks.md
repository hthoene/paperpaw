### Changed

- **USB developer tools row: a shorter description** (#12), so it fits its two lines in English and German.
- **The store plugin fits the S3's frame limit again** (#214): the page it installs from and the dialog buffers live on the heap. On X4 Pro and X4 Classic a store file packed as a plugin takes no community source's signature (an Ed25519 check needs about 4 KiB of stack there), so such a source's list is not read; the firmware's linked store is as before.
- **Translations past the read limit are read in a small window**, not by a 2 KiB block, so the peak of a book in Thai or Hindi stays where it was.

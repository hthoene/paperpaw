### Fixed

- **No room while a picture is made never aborts or marks it** (#228). Naming a picture's cache file in `prepare_image`, `prepare_image_larger` and the EPUB cover, and in a FictionBook's cover, no longer allocates without a way to refuse, so a short heap is an answer, not an abort. A FictionBook cover that found no room to be looked up is no longer marked undecodable (which hid it until its book's cache went). Sweeps over a refused allocation in each of these calls show the picture whole on the next read.

### Changed

- **The native settings read the registry a page at a time** (#228). Opening Settings no longer builds the list of every setting at once, which cut the peak heap of opening it on the X4 by about 94 KiB; vectors in plugin lists grow in small steps instead of doubling.

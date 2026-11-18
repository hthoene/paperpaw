### Changed

- **Catalog text buffers are made when first needed** (#228). A native plugin's `Messages` no longer fills a 4 KiB and a 1 KiB buffer when it is made, which cut the heap peak at the X4's start by about 5 KiB.

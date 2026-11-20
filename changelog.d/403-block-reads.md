### Changed

- **Fewer card reads per page** (#403). The EPUB, HTML and FB2 formats read up to 2 KB of a
  chapter's converted text at once and serve the blocks after it from memory. Before,
  each block was read from the card by itself. On the X4 every read goes through so much
  code that the flash cache misses most of it, about 1.7 ms each. Alice in Wonderland is
  now read through with 246 card reads instead of one per block (866). The kernel also
  looks up what it has already read in one step instead of comparing long paths.

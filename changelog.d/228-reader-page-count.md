### Changed

- **The reader on interface 7: the page count** (#228). The pages of the open book are counted in the
  background, a few at a time, into a small file kept per book and layout (8 bytes a page, as before),
  and the host's reading report carries the page and the number of pages once the count is complete.
  Another layout counts again; the file of the layout left is removed. The native reader takes over
  once the host runs native plugins.

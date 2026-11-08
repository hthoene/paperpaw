### Changed

- **Readwise on interface 7, first slice** (#228). Reading a book's exported notes and writing a
  highlight for Readwise are rewritten to refuse instead of aborting when memory runs short, for the
  native plugin interface. Highlights are sent as before; the native plugin takes over once the host
  runs native plugins.

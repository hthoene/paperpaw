### Changed

- **Plain text books get their interface-7 format provider** (#228). The
  `format-txt` plugin can now answer the host's `core.format` calls (open,
  prepare, metadata, sections, blocks, progress) from the book and the
  document cache, in fragments for long paragraphs. It is not active yet:
  the host switches to it with the other interface-7 plugins.

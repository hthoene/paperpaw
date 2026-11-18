### Fixed

- EPUB 1.0.5: zipped FictionBooks preserve out-of-memory and storage errors
  instead of reporting a malformed book. Unsupported or damaged unrelated ZIP
  members still allow a readable FictionBook to open (#228).

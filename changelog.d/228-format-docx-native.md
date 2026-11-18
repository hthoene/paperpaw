### Changed

- **Word documents are an interface-7 plugin** (#228). `format-docx` declares its format (`docx`,
  converted into the cache on the first open) and answers the host's `core.format` calls from a
  native entry, with the same pages, contents, links and pictures as before.

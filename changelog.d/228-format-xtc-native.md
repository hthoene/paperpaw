### Changed

- **XTC and XTCH books are an interface-7 plugin** (#228). `format-xtc` declares its format (`xtc`,
  `xtch`, recognised by their first bytes too) and answers the host's `core.format` calls from a
  native entry, with the same pages, chapters and covers as before.

### Changed

- **Word documents get the interface-7 format provider** (#228). `format-docx` answers the host's
  `core.format` calls (open, prepare, text, contents, links, pictures) with the same blocks as
  before, and its converter no longer stops on a full heap: a refused allocation makes the host try
  again instead of leaving a document without its styles or its links.

### Fixed

- **A Word document with a link address that has a multi-byte letter among its first characters**
  (#228) no longer stops the reader with a panic while its links are read.

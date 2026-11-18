### Added

- **A format can list the folder of its book** (#228). Inside a call of a
  `folder` or `siblings` format, `core.files` `list` and `stat` now take
  root 3: the book's folder and everything below it, read only, with no
  `uses.storage` needed. A single-file format, or a call outside a format
  call, still gets "unsupported"; changing anything there is "denied".

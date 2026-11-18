### Added

- **Which format reads a file, in the native SDK** (#228). `Documents::format_for` (DOC op 20: by
  extension, or by sniffing the file's first bytes) answers a `Format`, and `Documents::formats`
  (op 19) lists every format in pages (`FormatPage`, `FormatCursor`), for a file manager's Open and
  Open with.

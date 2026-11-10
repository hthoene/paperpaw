### Added

- **A file no format claims by extension is sniffed** (#228, `docs/abi/document.md` section 2). When no format claims a file's extension, `doc_open` and `format_for` ask the formats that declared `sniffs`, in priority order, with the file's first 512 bytes read through the card storage; the first one to say yes gets the file. A claimed extension never sniffs, and a file that cannot be read or that nobody wants stays unsupported.

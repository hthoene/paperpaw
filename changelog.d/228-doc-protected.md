### Changed

- **A book, a CSS file or a file to sniff on the card's top never names the kernel's or the plugins' folders** (#228, `docs/abi/document.md` section 1). On root 1 a path whose first segment is `plugins`, `.paperpaw` or `paperpaw.ini` (in any letter case) is `ERR_INVALID` for `doc_open`'s `path` and `css_path` and for `format_for`, the same rule as the file service.

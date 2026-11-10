### Added

- **A format lists its book's folder** (#228). `Files::list` and `Files::stat` take the book root (`Root::Book`, wire root 3) as `docs/abi/files.md` section 1.1 allows: inside a `core.format` call of a folder or siblings document a format lists the folder and stats its entries, such as a book's sibling pictures; the host answers Unsupported outside one. Every changing op still refuses the book root before any call.

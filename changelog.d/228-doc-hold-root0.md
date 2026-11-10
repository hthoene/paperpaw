### Added

- **A book of the opener's own data folder (root 0) is held while its document is open** (#228, `docs/abi/document.md` section 2). Like a root-1 book it is named to the in-use table by its card path (`.paperpaw/plugins/<id>/…`), so a move or removal of the file waits until the document closes, a failed open gives it back, and a refused allocation holds nothing and makes no document.

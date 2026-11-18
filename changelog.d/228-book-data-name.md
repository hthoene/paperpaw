### Changed

- **`book_data.spec` names are one folder name** (#228). A plugin's book-data name is 1 to 96 bytes of `a-z`, `0-9` and `-`, and a plugin names a folder once whatever its kind; anything else (empty, too long, an upper-case letter, `/`, `.`, `..`) is refused at admission with `ERR_INVALID`.

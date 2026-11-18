### Changed

- **Checking a file path is cheaper** (#250). Every storage call checks its path, and the layers it passes check it again: about 185 checks for one page turned in the reader, each reading the path four times over. The path is now read once, byte by byte, with the same answer for every path.

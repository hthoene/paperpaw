### Fixed

- **The table of contents of a book a plugin handed to the reader** (#228). The reader's table-of-contents screen opens the same file again, but its hand-over (`root` `5`) was refused, so a text such as the licences page had no table of contents (Beta 3 had one). A reader instance that holds a handed file now hands it on with `root` `5` and the same path: the host gives the new screen the same file of the same plugin, and nothing else (another path or an instance without a handed file is refused). The maker stopping still ends the document.

### Fixed

- **A flash card list saves again** (#257). Saving a list on the card (Vocabulary's *SD card* lists) ended with "The file could not be written": the save renames two files in a row, and the first rename started the library's move job, which made the second one wait. The host now moves a single file that is no book (by its name: no format reads it, nor its `.bak`) with the card's plain rename, without the library's journal and job; folders and books move as before. No interface change.

### Changed

- **For plugin authors: a single file that is no book moves on the user's files without the library** (#257, `docs/abi/files.md` 2.6). A `move` or `storage.rename` on root 1 of one file that no format reads (by its name, under the old or the new one) and that the library keeps no details for is the card's rename and nothing else: no book-moved event and no `event.library_changed` is sent, and nothing kept by the file's path (book keys) is carried. A plugin that keeps data by the path of such a file moves it itself. Folders, books and files the library keeps details for (a book whose format plugin is off) move as before, with the library's job.

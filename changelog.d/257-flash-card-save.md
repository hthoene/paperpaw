### Fixed

- **A flash card list saves again** (#257). Saving a list on the card (Vocabulary's *SD card* lists) ended with "The file could not be written": the save renames two files in a row, and the first rename started the library's move job, which made the second one wait. The host now moves a single file that is no book (by its name: no format reads it, nor its `.bak`) with the card's plain rename, without the library's journal and job; folders and books move as before. No interface change.

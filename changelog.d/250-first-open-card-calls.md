### Changed

- **A book's first opening makes no empty cache files** (#250). Each chapter converted for the first time had its seven cache files made empty before any content, each a whole write to the card (21 writes for the three chapters a first opening converts). A file is now made by the first bytes appended to it, and one without content is not there. What an interrupted conversion left is removed before the chapter is converted again, and the end mark is still written last.
- **An EPUB's file list is read in pieces, not entry by entry** (#250). Indexing the archive took two reads per file in it (358 for a book of 179 files), each a call to the host; the list is now read 2 KiB at a time, and a file is looked up in one read instead of two.

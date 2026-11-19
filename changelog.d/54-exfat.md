### Added

- **exFAT memory cards** (#54, experimental). The reader now reads and writes cards formatted as exFAT (how many cards of 64 GB and more are sold), with the same safety as on FAT32: every change to a folder is one block's write, so a power cut leaves the files as they were or as they are now. Mounting a 64 GB card takes four card commands and about 14 KB of the heap. The first time an exFAT card is used the reader says that exFAT support is experimental and that FAT32 is the safest; FAT32 cards work as before. The card's exFAT code is written from hadris (MIT), see About PaperPaw.

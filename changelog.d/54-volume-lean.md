### Changed

- **Fewer card commands for folders and reads** (#54). A new folder is cleared in one command instead of one for each block, and a block of a file read twice is not asked of the card again: the Library's first start and walk of 14 books sends about a fifth fewer commands. The firmware no longer mounts FAT12 and FAT16 volumes (cards under 2 GB); FAT32 is unchanged.

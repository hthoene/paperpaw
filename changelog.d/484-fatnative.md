### Fixed

- **Appends written back after the card refused them** (#484). Where the SD card refused to write back what was appended to a file (a timeout), the next sync said all was written while the file's entry on the card kept its old size; the next sync now writes it.
- **A card that could not be read is no longer called unformatted** (#484). Where the SD card stopped answering while its file system was read (a glitch on its bus), the X4 said it held neither FAT nor exFAT and should be formatted; it now says the card could not be read.
- **Hyphenation tables are charged the memory they take** (#484). Opening a hyphenation table counted 8 bytes a block for its index and fences where about 6.25 are kept (4 KB too much for German), so a second language could be refused for memory it would not have used.

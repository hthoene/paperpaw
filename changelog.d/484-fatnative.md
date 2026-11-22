### Fixed

- **Appends written back after the card refused them** (#484). Where the SD card refused to write back what was appended to a file (a timeout), the next sync said all was written while the file's entry on the card kept its old size; the next sync now writes it.
- **A card that could not be read is no longer called unformatted** (#484). Where the SD card stopped answering while its file system was read (a glitch on its bus), the X4 said it held neither FAT nor exFAT and should be formatted; it now says the card could not be read.

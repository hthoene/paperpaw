### Fixed

- **Appends written back after the card refused them** (#484). Where the SD card refused to write back what was appended to a file (a timeout), the next sync said all was written while the file's entry on the card kept its old size; the next sync now writes it.

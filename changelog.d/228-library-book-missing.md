### Added

- **A book's file that is gone from the card is told in the library's book
  answer** (#228). `core.library` op 3 sets flag bit 8 `missing` after one
  lookup of the file, so a plugin needs no storage access of its own to tell
  a gone book; a card that fails is an error, never "missing".

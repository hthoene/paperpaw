### Changed

- **Plain text reads with less memory** (#248). `format-txt` reserves the decoded text at the size it
  takes (not three times the file bytes), writes the records of a block straight into the reply (no
  64-record list), and no longer reads 4 KiB ahead: the held window is about 9 KiB, not 13 KiB.
  Reading a TXT book on the X4 emulator takes 4.0 KiB less live and 10.3 KiB less at the peak.

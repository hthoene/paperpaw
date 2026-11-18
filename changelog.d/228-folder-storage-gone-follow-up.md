### Fixed

- **A pulled card is also noticed on the file read last** (#228). A folder
  volume (desktop emulator card, Android shared storage and SD cards) that
  is gone says "card gone" on the next read of the book that was open, and so
  does a volume whose access was revoked, where it answered "unsupported".

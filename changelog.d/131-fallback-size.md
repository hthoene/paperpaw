### Fixed

- A large clock in Persian or Urdu digits is drawn at one size: where the interface's font is scalable (Android, the emulator) and the digits come from the Arabic letters pack, which has no such large size, the colon is no longer drawn far larger than the digits beside it; the whole time is set at the pack's largest size (#131). Numbers in Latin digits and all text of letters are drawn as before.

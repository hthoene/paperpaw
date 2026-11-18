### Changed

- **Opening a screen with parameters takes no extra memory** (#228). The
  handover of up to 32 parameters is encoded straight into the plugin's own
  buffer instead of through a 3,328-byte temporary list, which was most of
  the 4 KiB the X4 may add. A test counts zero allocated bytes.

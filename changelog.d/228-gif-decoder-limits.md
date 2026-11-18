### Changed

- **The GIF decoder refuses what the host refuses** (#228). `paperpaw-image`
  refuses a picture larger than its screen and a screen over 8 megapixels
  itself (before, only the host checked, so a plugin's own decode could still
  take frames up to 64 megapixels), and keeps its colour tables on the heap so
  its stack frames stay small on the X4.

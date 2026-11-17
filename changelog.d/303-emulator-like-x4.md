### Fixed

- **The Home clock and date sat too low on the X4, and the Home card was
  pushed down with them** (#303). Where the host does not tell the ink of
  text, the clock took the digits for as tall as the size says, but the
  X4's fonts stop growing at their largest size, so the block was about
  130 px too high. It now takes the size the font really draws.

### Changed

- The emulator's X4, X4 Pro, X4 Classic and X3 profiles no longer answer
  `text.ink`, as the firmware does not, so they draw what the device
  draws (their screens in the snapshots moved with it).

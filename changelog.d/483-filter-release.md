### Fixed

- **Buttons answer right after a plugin used a press of them** (#483).
  Where a plugin's input filter (a keyboard, a lock) took the release of a
  button, the device thought it still held: the next press did nothing, or
  a long press came that nobody made.

### Fixed

- **A slow computer on the USB cable no longer loses an answer** (#483).
  When the device's answers waited to be sent, the next one was dropped and
  the computer waited for it in vain; now the plugin answers again once
  there is room.

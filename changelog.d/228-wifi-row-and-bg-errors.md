### Fixed

- The Wi-Fi settings row reads as in Beta 3 again (`Wi-Fi, Connected to <network>`, `Wi-Fi, Off`): it is told as a switch and no longer adds the word *On* before where the radio is.
- The Wi-Fi status item answers the host's status ask: its name shows in the status bar while a network is joined (before, the ask ended with an error in the background, at every refresh).
- The power plugin reads the device's power policy and identity into a reply buffer of the size the host requires (4,096 bytes); with 512 the host refused the call with `NoSpace` at start and for the status item.

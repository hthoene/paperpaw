### Fixed

- `paperpaw-dev perf` over all plugins ends without an error again: the Wi-Fi plugin's list of networks goes back while the radio is off (as on a device, where the page only offers it with the radio on), and the emulator starts with the radio off, so it was reported as not measured. A screen that closes at once is now tried once more on a second emulated device with the radio on and joined, and only an error if it still closes there.

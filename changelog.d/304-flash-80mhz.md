### Changed

- **The boot reads the flash at 80 MHz on the X4, the X4 Pro and the X4 Classic** (#304). The X4's flash chip (Puya PY25Q128HA, rated 104 MHz for DIO reads) is now clocked at 80 MHz by the boot loader instead of 40 MHz, which shortens the start; the X4 Pro and the X4 Classic verify and boot at 80 MHz too (the app loads 430 to 460 ms earlier). The X3 keeps 40 MHz until its flash chip is known.
